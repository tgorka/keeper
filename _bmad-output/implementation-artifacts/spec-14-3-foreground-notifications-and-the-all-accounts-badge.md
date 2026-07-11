---
title: 'Foreground Notifications and the All-Accounts Badge'
type: 'feature'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: ['oversized']
baseline_revision: '0e19bc550e0e6cc893fb9de55514c694c35c079e'
final_revision: '06066b1c112c1599c6552d1208ebd5b398b56c53'
---

<intent-contract>

## Intent

**Problem:** On iOS the notification/badge surfaces are half-built. The Unified-Inbox unread aggregate is computed in Rust and pushed through `Platform::set_badge_count`, but `IosPlatform::set_badge_count` is an honest **no-op** — so the app-icon badge never actually shows, while Story 14.2's `BADGE_NOT_LIVE_SENTENCE` already advertises a badge and 14.2 hid the only badge-mode control (all/mentions/off) on the reduced tier (deferred-work: re-surface it here). Foreground-local notifications post through the shared engine, but that engine has **no visible-Chat suppression** — Story 10.1 explicitly deferred focus suppression, so a banner still fires for the very Chat on screen. And there is no permission-denied surface: when iOS notification permission is off, nothing tells the user, and the badge silently needs that same permission.

**Approach:** Point `IosPlatform::set_badge_count` at the already-computed aggregate via Tauri's `WebviewWindow::set_badge_count` (sets iOS `applicationIconBadgeNumber`; needs the notification permission already requested at startup) — no native code, no second count (AD-20). Add visible-Chat suppression to the shared `should_notify` rules engine (AD-18), fed by an active-chat signal the iOS shell reports from `roomsStore.selected` — reported only on the reduced tier so desktop notification behavior is byte-for-byte unchanged. Re-surface an iOS "App icon badge" mode control in Settings → Notifications (reusing the shared `DockBadgeMode` setting). Add a persistent permission-denied inline state there with an Open-iOS-Settings deep link (through a Rust opener command), noting the badge needs the same permission, never self-re-prompting.

## Boundaries & Constraints

**Always:**
- The badge value is the Unified-Inbox unread aggregate already computed by `inbox::emit`/`reapply_badge` per `DockBadgeMode` (AD-20) — never compute a second count; iOS only makes the existing value reach the OS.
- All notification decisions stay in the shared `keeper-core::notify` engine (`should_notify`/`dispatch`); iOS reuses it (AD-18). `should_notify` stays a pure function; state (active chat, DND) is resolved in `dispatch`.
- Honesty rule (FR-53, FR-62): no surface implies a live badge or notifications while suspended; the badge is only ever updated while the app runs (sync completion + foreground resume). Suppress notifications for the currently-visible Chat.
- No native Swift/ObjC; `unsafe_code = "deny"`; badge via the Tauri window API, notifications via `tauri-plugin-notification` (both already mobile-capable and wired in `setup()`). No `.unwrap()`/bare `.expect()` in prod paths.
- The active-chat suppression signal is reported **only on the reduced-capability tier**, so desktop notification behavior is unchanged (the 10.1 focus-suppression deferral stays open for desktop).
- Frontend: no `any`, `import type` for types; reuse the existing `dockBadgeMode*` IPC pattern, the `useIsReducedCapabilityPlatform()` gate, and the `void openUrl(...).catch(() => {})` best-effort idiom.

**Block If:**
- `tauri-plugin-notification` exposes no permission-state read on the iOS target (i.e., reporting OS notification-permission status would require a new native plugin) — HALT rather than adding native code.

**Never:**
- Never add push/APNs/NSE/background sync; never post iOS notifications while backgrounded (14.1 pauses sync there); never re-prompt for notification permission from the UI (UX-DR28).
- Never change desktop notification or dock-badge behavior; never wire the active-chat signal on the desktop tier.
- Never widen the opener JS capability scope for `app-settings:` — the deep link goes through Rust's `Platform::open_url`, which bypasses the JS scope.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Message for a non-visible Chat, app foreground | active chat = other/none | `should_notify` passes; local notification posts (10.1/10.2 rules unchanged) | notifier failure logged at warn, swallowed |
| Message for the currently-visible Chat | active chat == message room | `should_notify` returns false — suppressed | none |
| Active chat set then message arrives, then chat closed | `active_chat_set(a,r)` → later `active_chat_set(null)` | suppressed while open; notifies again after clear | best-effort IPC; ignore rejection |
| Badge recompute, mode=all | aggregate = N unread rooms | `set_badge_count(Some(N))` → iOS icon badge = N | handle/window unset → honest no-op |
| Badge recompute, aggregate = 0 or mode=off | zero unread / `Off` | `set_badge_count(None)` → badge cleared | no-op if unset |
| Foreground resume | `app_lifecycle_changed(Foreground)` | `sync_now` runs and the badge is re-asserted from the current aggregate | infallible, best-effort |
| Notification permission denied | `permission_state() == Denied` | Settings → Notifications shows persistent inline "off" state + Open-Settings deep link + badge-needs-permission note; no re-prompt | read failure ⇒ treat as `Unknown`, hide the inline state |
| Open-Settings tapped | reduced tier, permission denied | `ios_open_app_settings` opens `app-settings:` via `Platform::open_url` | opener failure swallowed (best-effort) |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/src/ipc.rs` -- `IosPlatform::set_badge_count` (605) → mirror `DesktopPlatform::set_badge_count` (437): `BADGE_APP` → main webview window → `window.set_badge_count(count.map(i64::from))`. Add `#[tauri::command]`s: `active_chat_set`, `notification_permission_state` (reach `NOTIFY_APP` → `app.notification().permission_state()`), `ios_open_app_settings` (calls `state.platform.open_url(IOS_APP_SETTINGS_URL)`). Mirror `dock_badge_mode_*` (2900/2909) for delegation.
- `src-tauri/crates/keeper-core/src/notify.rs` -- `NotifyConfig` (47) add `active_room: RwLock<Option<(String, String)>>` + `set_active_room`/`clear_active_room`/`is_active_room`; `should_notify` (198) add an `is_active_room: bool` gate (suppress when true); `dispatch` (291) resolve `config.is_active_room(account_id, room_id)` and pass it. Update `#[cfg(test)]` callers.
- `src-tauri/crates/keeper-core/src/account.rs` -- `AccountManager` (`notify: Arc<NotifyConfig>`, 575): add `set_active_room`/`clear_active_room` delegating to `self.notify` (mirror `set_previews_enabled` 3487); in the foreground path re-assert the badge (call the inbox merger's `reapply_badge`, as `dock_badge_mode_set` does).
- `src-tauri/crates/keeper/src/lifecycle.rs` -- `Foreground` branch (57): after `sync_now`, re-assert the badge on resume (iOS-scoped; desktop never invokes this command).
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `NotificationPermission` enum (`Granted`/`Denied`/`Unknown`, `#[ts(export)]`, serde lowercase), mirroring `DockBadgeMode` (1433).
- `src-tauri/crates/keeper/src/lib.rs` -- register the three new commands in `generate_handler!` (near `dock_badge_mode_*`, 205-ish).
- `src/lib/ipc/client.ts` -- add typed wrappers `activeChatSet(selection)`, `notificationPermissionState()`, `iosOpenAppSettings()` (mirror `dockBadgeModeGet/Set`).
- `src/hooks/use-active-chat-reporter.ts` -- NEW: on the reduced tier, subscribe to `roomsStore.selected` and call `activeChatSet(selection)` on change / `activeChatSet(null)` on unmount; no-op on desktop.
- `src/App.tsx` -- mount `useActiveChatReporter()` alongside the other lifecycle hooks (`useAppLifecycle`).
- `src/components/settings/settings-dialog.tsx` -- `NotificationsSection` (257): on the reduced tier render (a) an "App icon badge" mode radio reusing `DOCK_BADGE_OPTIONS` + `dockBadgeModeGet/Set` (mirror `BackgroundSection`'s radio at 456), and (b) a permission-denied inline state (query `notificationPermissionState()` on open; when `Denied` show the fixed sentence + a note the badge needs the same permission + an Open-Settings button calling `iosOpenAppSettings`). No re-prompt.
- `src/lib/stores/rooms.ts` -- `selected: RoomSelection | null` (67) / `selectRoom` (200): the source of truth the reporter observes.
- `src/lib/stores/capabilities.ts` -- `useIsReducedCapabilityPlatform()` gate for the reporter and the new Settings blocks.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `NotificationPermission { Granted, Denied, Unknown }` (`Serialize`, `TS`, `#[serde(rename_all = "lowercase")]`, `#[ts(export)]`). -- typed permission state for the UI.
- [x] `src-tauri/crates/keeper-core/src/notify.rs` -- add `active_room: RwLock<Option<(String, String)>>` to `NotifyConfig` with `set_active_room(account_id, room_id)`, `clear_active_room()`, `is_active_room(account_id, room_id) -> bool`; add the `is_active_room: bool` gate to `should_notify` (suppress when true); resolve and pass it in `dispatch`. -- visible-Chat suppression in the shared engine.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- `AccountManager::set_active_room`/`clear_active_room` delegating to `self.notify`; re-assert the badge on foreground resume via the inbox merger's `reapply_badge` (same call `dock_badge_mode_set` uses). -- account seam + resume-refresh.
- [x] `src-tauri/crates/keeper/src/lifecycle.rs` -- in the `Foreground` branch, after `sync_now` re-assert the badge (iOS-scoped). -- badge refreshes on resume without a second sync truth.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- implement `IosPlatform::set_badge_count` (mirror desktop: `BADGE_APP` → main window → `set_badge_count(count.map(i64::from))`); add commands `active_chat_set(account_id: Option<String>, room_id: Option<String>)` (both `Some` ⇒ set, both `None` ⇒ clear), `notification_permission_state() -> Result<NotificationPermission, IpcError>` (`NOTIFY_APP` → plugin `permission_state()`; unset/err ⇒ `Unknown`), `ios_open_app_settings()` (`state.platform.open_url(IOS_APP_SETTINGS_URL)`, `IOS_APP_SETTINGS_URL = "app-settings:"`). -- the real badge + three seams.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register `active_chat_set`, `notification_permission_state`, `ios_open_app_settings` in `generate_handler!`. -- wires them.
- [x] `src/lib/ipc/client.ts` -- add `activeChatSet(selection: RoomSelection | null)`, `notificationPermissionState(): Promise<NotificationPermission>`, `iosOpenAppSettings(): Promise<void>`. -- frontend seams.
- [x] `src/hooks/use-active-chat-reporter.ts` (new) -- reduced-tier-only hook that mirrors `roomsStore.selected` to `activeChatSet`, clearing on unmount; desktop no-op. -- feeds the suppression signal.
- [x] `src/App.tsx` -- mount `useActiveChatReporter()` with the other lifecycle hooks. -- activates the reporter.
- [x] `src/components/settings/settings-dialog.tsx` -- in `NotificationsSection`, reduced tier only: add the "App icon badge" mode radio (reuse `DOCK_BADGE_OPTIONS` + `dockBadgeModeGet/Set`) and the permission-denied inline state (`notificationPermissionState()` on open; `Denied` ⇒ fixed sentence + badge-needs-permission note + Open-Settings button → `iosOpenAppSettings`). -- re-surfaces the control + adds the honest denied surface.
- [x] `src-tauri/crates/keeper-core/src/notify.rs` (tests) -- cover `should_notify` active-room suppression (on/off) and the `NotifyConfig` active-room set/clear/`is_active_room` round-trip. -- guards the suppression gate.
- [x] `src/hooks/use-active-chat-reporter.test.ts` (new) -- reports selection on the reduced tier, clears on `null`/unmount, no-op on desktop. -- guards the reporter's gating.
- [x] `src/components/settings/settings-dialog.test.tsx` (extend) -- reduced tier: badge-mode radio present; `Denied` shows the inline state + Open-Settings calls `iosOpenAppSettings`, `Granted`/`Unknown` hides it; desktop shows neither. -- audit guard.

**Acceptance Criteria:**
- Given the iOS tier with unread across accounts and the app running, when sync completes or the app foregrounds, then the app-icon badge equals the Unified-Inbox aggregate per the badge mode (sourced from `inbox`, never a second count) and is set only while running.
- Given the iOS tier viewing Chat A, when a message arrives in Chat A, then no notification posts; when a message arrives in another Chat, then a notification posts with the same content/preview/mute/mention-only semantics as desktop.
- Given the iOS tier with notification permission denied, when Settings → Notifications opens, then a persistent inline state shows "Notifications are off for keeper in iOS Settings." with a working Open-Settings deep link and a note the badge needs the same permission, and nothing re-prompts.
- Given the desktop tier, when the app runs, then notification and dock-badge behavior is unchanged and no active-chat signal is sent.
- Given the full change, when the quality gates run, then `bun run check`, `bun run check:rust`, and `bun run test:rust` all pass.

## Spec Change Log

<!-- Append-only. Empty until the first bad_spec loopback. -->

## Review Triage Log

<!-- Append-only. Populated by step-04 on every review pass. -->

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 0
- reject: 9
- addressed_findings:
  - `[low]` `[patch]` Redundant `activeChatSet` IPC on value-equal re-selection (Blind Hunter #2 = Edge Case Hunter #6): `selectRoom`/`requestFocus` store a fresh `{accountId, roomId}` literal, so re-selecting the on-screen Chat changed `selected`'s identity (not its value) and fired a redundant IPC. Added a value-equality guard in `use-active-chat-reporter.ts` (first report still fires, so Rust's active-room seeding is unchanged) + a dedupe test.
- notes:
  - Reviewers found **zero** high/medium functional defects; Blind Hunter confirmed the Rust machinery holds (no dropped inbox handle across pause/resume, badge honors `DockBadgeMode`, `Some(0)`→`None`, faithful desktop mirror, handles set unconditionally, no `unwrap`/`any`/push/native-Swift, desktop path unchanged).
  - Rejected (9): (1) `active_chat_set` partial-pair → clear — defined, documented, safe behavior, unreachable from the sole TS caller (always both-or-both-null); (2) iOS badge "Off" vs `BADGE_NOT_LIVE_SENTENCE` — the fixed 14.2 canonical sentence stays truthful (a disabled badge is trivially "not a live count"); fragmenting it per-mode is worse; (3) "Never re-prompts" doc-comment — accurate for the current code (a read, not `request_permission`); a hypothetical-refactor concern, not a defect; (4) Foreground `reassert_badge` re-pushes the pre-pause aggregate — confirmed intended AD-20 posture by both reviewers, self-corrects on the next `emit`; (5) stale `active_room` after sign-out — handled by `use-sign-out.ts` (`selectRoom(null)` on signing out the active account → reporter clears), not reachable; (6) permission staleness while Settings stays open — AC is satisfied ("when Settings renders"), self-heals on reopen, a foreground re-probe is beyond-AC polish; (7) badge-mode radio enabled under denied permission — the mode is a valid persisted preference that applies once permission is granted, and the inline "off" state already discloses it; (8) reporter tier-flip ordering — capabilities hydrate once, the mount-once reporter never sees a real flip; (9) set-vs-dispatch async gap — inherent async; a brief banner for a just-opened Chat is a safe over-notify, never a drop.

## Design Notes

- **The badge plumbing already exists — iOS just terminated in a no-op.** `inbox::emit`/`reapply_badge` compute `unread_rooms`/`mention_total` and call `badge::apply` → `platform.set_badge_count` on every merged-state change (AD-20). Making iOS real is a one-method change (mirror desktop); verified `WebviewWindow::set_badge_count` maps to iOS `applicationIconBadgeNumber` via `tao`, and both `BADGE_APP` and the startup permission request are already unconditional in `setup()`. This is exactly "as far as iOS allows": updatable only while running, hence AD-30's "never live while suspended" (already disclosed by 14.2's `BADGE_NOT_LIVE_SENTENCE`).
- **Visible-Chat suppression is built here, in the shared engine.** AD-18 names "reused desktop visible-Chat suppression logic," but 10.1 deferred it — so this story adds the gate to `should_notify` (kept pure) with the active-chat state resolved in `dispatch`. On iOS this is unambiguous: 14.1 pauses sync in background, so notifications only fire while foreground, and `roomsStore.selected` is exactly what's on screen. The signal is reported **only on the reduced tier**, so desktop notifications are untouched and the 10.1 desktop deferral remains open (desktop can wire the same `active_chat_set` command later — cheap).
- **Permission read and Settings deep link go through Rust.** The opener JS default scope only permits `mailto/tel/http(s)`, so `app-settings:` would be blocked from JS; routing it through `Platform::open_url` (the Rust opener call) avoids widening the JS scope. Permission state is read from the notification plugin in Rust and surfaced as a typed enum, keeping notification concerns in one place and testable.
- **Reuse the shared badge-mode setting.** The iOS "App icon badge" control writes the same `notify.dock_badge_mode` (`DockBadgeMode`) the desktop dock badge uses — a user is on one platform per device, so one setting is correct and keeps the disclosure copy and the available control honest and consistent (closes the 14.2 deferral).

## Verification

**Commands:**
- `bun run check:rust` -- expected: `cargo fmt --check` + clippy `-D warnings` clean over the new `NotifyConfig` active-room state, `should_notify` gate, `NotificationPermission`, and the three commands.
- `bun run test:rust` -- expected: cargo-nextest green, including the active-room suppression and round-trip tests.
- `bun run check` -- expected: Biome + tsc + Vitest green, including the active-chat-reporter and extended settings-dialog tests.

## Auto Run Result

Status: done

**Summary:** Made the iOS notification/badge surfaces real and honest (FR-62, AD-18, AD-20, AD-30). `IosPlatform::set_badge_count` now mirrors the desktop port (reaches the already-wired `BADGE_APP` → main webview window → `set_badge_count`), so the Unified-Inbox aggregate already computed by `inbox::emit`/`reapply_badge` finally drives the iOS app-icon badge (`applicationIconBadgeNumber`) — no native code, no second count. Visible-Chat suppression was added to the shared `should_notify`/`dispatch` engine (a new `is_active_room` gate over `NotifyConfig.active_room`), fed by an active-chat signal the iOS shell reports from `roomsStore.selected` on the reduced tier only — desktop notification behavior is byte-for-byte unchanged. The badge is re-asserted on foreground resume (lifecycle `Foreground` → `reassert_badge` after `sync_now`). Settings → Notifications re-surfaces an "App icon badge" mode control (reusing the shared `DockBadgeMode` setting, closing 14.2's deferral) and adds a persistent permission-denied inline state — "Notifications are off for keeper in iOS Settings." + a badge-needs-the-same-permission note + an Open-Settings deep link routed through Rust `Platform::open_url("app-settings:")` (bypassing the opener JS scope), never self-re-prompting.

**Files changed:**
- `src-tauri/crates/keeper-core/src/vm.rs` — new `NotificationPermission { Granted, Denied, Unknown }` enum (ts-rs exported).
- `src-tauri/crates/keeper-core/src/notify.rs` — `NotifyConfig.active_room` (+ set/clear/is_active_room, poison-recovering, fail-open); `should_notify` gains the `is_active_room` gate; `dispatch` resolves and passes it; tests for the gate, round-trip, and suppress-then-resume.
- `src-tauri/crates/keeper-core/src/account.rs` — `AccountManager::set_active_room`/`clear_active_room` + `reassert_badge` (reuses the inbox merger's `reapply_badge`).
- `src-tauri/crates/keeper/src/lifecycle.rs` — `Foreground` re-asserts the badge after `sync_now`.
- `src-tauri/crates/keeper/src/ipc.rs` — real `IosPlatform::set_badge_count`; commands `active_chat_set`, `notification_permission_state`, `ios_open_app_settings`.
- `src-tauri/crates/keeper/src/lib.rs` — registered the three commands.
- `src/lib/ipc/client.ts` + `src/lib/ipc/gen/NotificationPermission.ts` — typed wrappers + generated type.
- `src/hooks/use-active-chat-reporter.ts` (+ test) — reduced-tier reporter mirroring `roomsStore.selected` to `activeChatSet` (value-deduped), clearing on unmount.
- `src/App.tsx` — mounts `useActiveChatReporter()`.
- `src/components/settings/settings-dialog.tsx` (+ test) — iOS "App icon badge" radio + permission-denied inline state.

**Review findings breakdown:** 0 intent gaps, 0 spec repairs. 1 patch (low): value-equality dedupe of the active-chat reporter so re-selecting the on-screen Chat no longer fires a redundant IPC (+1 test). 0 deferred. 9 rejected — reviewers found zero high/medium functional defects; the rejects were intended postures (AD-20 badge reassert, fail-open notify, canonical 14.2 copy), already-handled paths (sign-out clears `active_room` via `use-sign-out.ts`), beyond-AC self-healing polish (permission staleness while Settings stays open), or unreachable/benign edges. See the Review Triage Log for the full itemization.

**Follow-up review recommended:** false — the only review-driven change was one localized, low-consequence quality patch (a reporter IPC dedupe) with a new test and no behavior/API/security/data impact.

**Verification** (all re-run independently after the patch):
- `bun run check:rust` — PASS (`cargo fmt --check` + clippy `-D warnings` clean).
- `bun run test:rust` — PASS (775 tests, incl. the active-room suppression + round-trip tests).
- `bun run check` — PASS (Biome + tsc + Vitest 1192 tests incl. the new reporter/settings tests; core-tauri-free convention holds). The `#[cfg(target_os = "ios")]` `IosPlatform` badge path is validated by the separate iOS compile-check CI job (Story 12.5), not the desktop gates.

**Residual risks:** (1) permission-denied inline state can go stale if the user grants in iOS Settings and returns without closing the Settings dialog — self-heals on reopen (beyond-AC, rejected); (2) a message arriving in the microseconds between opening a Chat and the `active_chat_set` IPC landing can raise one banner for that Chat — a safe over-notify, never a drop; (3) the iOS badge/notification path itself is only Simulator/CI-compile-verified here — on-device badge and foreground-notification behavior fold into SM-8 dogfooding per the epic.
</content>
</invoke>
