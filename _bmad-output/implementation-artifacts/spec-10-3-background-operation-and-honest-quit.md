---
title: 'Background Operation and Honest Quit'
type: 'feature'
created: '2026-07-06'
status: 'done'
baseline_revision: 'd392c4f73546bb141d30c30e24d1e2049723abea'
final_revision: 'b9a093e4340d8ad10074d05471a9cb2e3eda2125'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-10-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** keeper's sync lives in Rust tasks that today are only reachable while the window is open, and there is no window/quit lifecycle wiring: closing the window may terminate the process, ⌘Q does not stop sync any differently than ⌘W, there is no dock badge, no menu-bar presence, and no launch-at-login — so "running in the background" and "quit" are both unsubstantiated claims (Story 10.3, FR-53, UX-DR17, AD-18/AD-25).

**Approach:** Make the process — not the window — own sync. Intercept window-close (⌘W) to hide the window while every account's `SyncService` and the notification pipeline keep running unchanged; intercept app-quit (⌘Q / native Quit) to gracefully stop all account sync and then fully exit. Add a Rust-computed dock badge (all-unreads / mentions-only / off) that stays correct while the window is hidden, an opt-in menu-bar (tray) presence, and opt-in launch-at-login via `tauri-plugin-autostart`. Add honest Settings copy that states ⌘Q stops syncing and never promises push-while-quit.

## Boundaries & Constraints

**Always:**
- Sync lifetime is bound to the process, never to window visibility. Closing/hiding the window MUST NOT stop, pause, or degrade any account's `SyncService` or native-notification posting — background behavior is byte-for-byte identical to foreground.
- Window-close (`WindowEvent::CloseRequested`) hides the main window (`prevent_close` + `hide`), keeping the webview and all sync alive. The window is re-shown/focused on macOS dock-click (`RunEvent::Reopen`), via the existing global hotkey (Story 9.4), and via the tray menu when present.
- App-quit (⌘Q and the native Quit menu item → `RunEvent::ExitRequested`) MUST fully terminate the process, after a best-effort, time-bounded graceful `shutdown_all()` that awaits each account's `sync.stop()`. No hidden background process survives a quit.
- The dock badge count is computed in Rust from the full cross-account unread/mention state (never from the windowed view models and never in TypeScript), so it stays correct while the window is hidden. Mode `all` → count of unread rooms; `mentions` → total `mention_count`; `off` → no badge. A zero total clears the badge.
- Dock-badge mode persists in `keeper.db` `settings` and survives restart; default `all`. Launch-at-login and menu-bar presence are opt-in, OFF by default; never enable either without an explicit user toggle. Launch-at-login state is authoritative in the autostart plugin.
- Settings copy about quitting states plainly that ⌘Q stops syncing and notifications; it must never promise notifications/push while quit (egress-honesty).
- Respect project invariants: no `.unwrap()`/bare `.expect()` in prod paths; `keeper-core` stays platform-free (OS dock badge / tray / autostart reached through the `Platform` port or the `keeper` shell crate); no Matrix/state logic in TypeScript; commit on the current branch only.

**Block If:**
- The inbox merger does not actually retain full cross-account room state (only windowed slices), so an accurate background dock-badge total cannot be produced without new full-state infrastructure. HALT rather than ship a silently-inaccurate badge — blocking condition `dock-badge needs full-state aggregation not present`.
- Adding the autostart or tray capability requires an entitlement / bundling change that alters the app's security posture beyond the plugin's default permission and would collide with the Epic 11 signing/notarization pipeline. Blocking condition `autostart capability needs security decision`.

**Never:**
- Never route unread/badge state or notifications through any push service, and never keep a background helper process alive after ⌘Q (egress + honest-quit invariant).
- Foreground-focus / active-room notification suppression stays OUT of scope — it contradicts the foreground↔background parity guarantee and remains a deferred product decision (deferred-work item from Story 10.1).
- No inline notification quick-reply (v1.x). Do not compute the badge in the webview or from the `InboxRoomVm` stream. Do not enable launch-at-login or menu-bar presence by default. Do not create branches, push, or rewrite history.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Background parity | Window closed via ⌘W, message arrives on any account | Window hidden, process + all account syncs alive; native notification posts within the latency bar exactly as foreground; badge updates per mode | Hide failure logged at warn; process must not exit |
| Re-summon | Window hidden, user clicks dock icon / presses global hotkey / clicks tray "Show keeper" | Main window shown and focused | If window missing, log warn; no panic |
| Honest quit | ⌘Q or native Quit selected | `shutdown_all()` awaited (bounded), every `SyncService` stopped, then process exits; no lingering process | If shutdown exceeds bound, still exit; log warn |
| Badge all | mode=`all`, N rooms unread across accounts | Dock badge shows N; N=0 clears badge | Platform handle unset (headless) → no-op |
| Badge mentions | mode=`mentions`, total mention_count=M | Dock badge shows M; M=0 clears badge | as above |
| Badge off | mode=`off`, any unread state | Badge cleared, never shown | as above |
| Mode change | User picks a new dock-badge mode in Settings | Persisted to settings; badge immediately recomputed and reapplied | Persist error surfaces as IpcError; UI reverts |
| Launch-at-login | Fresh install (no prior toggle) | Autostart is disabled; toggling on enables the LaunchAgent, toggling off removes it | Plugin error → IpcError; UI reverts |
| Menu-bar presence | Toggle on | Tray icon appears with Show keeper + Quit; toggle off removes it; choice persists across restart | Tray build failure logged at warn; app continues |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- add `DockBadgeMode { All, Mentions, Off }` (`#[derive(...,TS)]`, `#[serde(rename_all="snake_case")]`, `#[ts(export)]` → `"all"|"mentions"|"off"`) with `as_registry_str`/`from_registry_str`, mirroring the `ChatNotifyMode` enum pattern (l.~1336).
- `src-tauri/crates/keeper-core/src/registry.rs` -- add `get_dock_badge_mode`/`set_dock_badge_mode` (key `notify.dock_badge_mode`, default `All`) and `get_menu_bar_presence`/`set_menu_bar_presence` (key `system.menu_bar_presence`, default `false`), mirroring `get_notify_previews`/`set_notify_previews` (l.438-457) over the existing `settings` table.
- `src-tauri/crates/keeper-core/src/platform.rs` -- add `fn set_badge_count(&self, count: Option<u32>) -> Result<(), CoreError>;` to the `Platform` trait (l.14-50).
- `src-tauri/crates/keeper-core/src/badge.rs` (**new**, or a section in `notify.rs`) -- `BadgeConfig` holding the current `DockBadgeMode` (atomic/lock); pure `badge_count(mode, unread_rooms: u32, mention_total: u32) -> Option<u32>` (All→`(unread_rooms>0).then_some(unread_rooms)`; Mentions→`(mention_total>0).then_some(mention_total)`; Off→`None`); `apply(&dyn Platform, mode, unread_rooms, mention_total)`. Unit-tested. Register `pub mod badge;` in `lib.rs`.
- `src-tauri/crates/keeper-core/src/account.rs` -- seed `BadgeConfig` from `registry::get_dock_badge_mode` in `AccountManager::new` (l.588); add `dock_badge_mode_get()` and `dock_badge_mode_set(&Platform, DockBadgeMode)` (persist-then-apply-then-recompute, like `notify_previews_set`); add `menu_bar_presence_get(&Platform)`/`menu_bar_presence_set(&Platform, bool)` wrapping the registry; add `shutdown_all()` iterating every active `account_id` and awaiting `shutdown(id)` (reuse the existing per-account `shutdown` at l.4260 which already awaits `sync.stop()`).
- `src-tauri/crates/keeper-core/src/inbox.rs` -- in the `InboxMerger` update path (owns the full cross-account room set), after each merged-state change compute cross-account `unread_rooms` (count of `is_unread`) and `mention_total` (sum of `mention_count`) over ALL rooms and call `badge::apply(&*platform, config.mode(), ...)`. Give the merger an `Arc<dyn Platform>` + `Arc<BadgeConfig>`. Must not alter unread computation.
- `src-tauri/Cargo.toml` + `src-tauri/crates/keeper/Cargo.toml` -- add `tauri-plugin-autostart = "2"` (workspace + `{ workspace = true }`).
- `src-tauri/crates/keeper/src/lib.rs` -- register `tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None)`; add `.on_window_event` on the `main` window: `WindowEvent::CloseRequested` → `api.prevent_close()` + `window.hide()`; capture `RunEvent` in `.run(...)`: `ExitRequested` → best-effort `tauri::async_runtime::block_on` a bounded `state.accounts.shutdown_all()` then allow exit, `Reopen` → show+focus `main`; build the tray icon at `setup` per `menu_bar_presence` (menu: Show keeper + Quit), store its handle; store the AppHandle for badge (reuse/extend the `NOTIFY_APP` OnceLock pattern, l.240); register the 6 new commands in `invoke_handler!`.
- `src-tauri/crates/keeper/src/ipc.rs` -- implement `DesktopPlatform::set_badge_count` via the stored AppHandle → `main` `WebviewWindow::set_badge_count(count.map(i64::from))`, honest no-op when the handle is unset (headless/tests); add commands `dock_badge_mode_get`/`dock_badge_mode_set` (via `state.accounts`), `launch_at_login_get`/`launch_at_login_set` (via the autostart plugin's `ManagerExt::autolaunch().is_enabled()/enable()/disable()`), `menu_bar_presence_get`/`menu_bar_presence_set` (persist via `state.accounts` + create/destroy the tray live through the AppHandle). Mirror the `notify_get_preview_enabled`/`notify_set_preview_enabled` command shape (l.2483).
- `src-tauri/crates/keeper/capabilities/default.json` -- add autostart permissions (`autostart:allow-enable`, `autostart:allow-disable`, `autostart:allow-is-enabled`) and any tray/badge core permission the build requires.
- `src/lib/ipc/gen/DockBadgeMode.ts` -- regenerated ts-rs binding (`"all"|"mentions"|"off"`).
- `src/lib/ipc/client.ts` -- add typed wrappers `dockBadgeModeGet/Set`, `launchAtLoginGet/Set`, `menuBarPresenceGet/Set`, and re-export the `DockBadgeMode` type (mirror `notifyGetPreviewEnabled`/`dndGetGlobal`, l.1436-1464).
- `src/components/settings/settings-dialog.tsx` -- in the Notifications section (or a new "Background & dock" subsection): a Dock-badge-mode `RadioGroup` (All unreads / Mentions only / Off), a "Launch at login" `Switch` (off by default), an optional "Keep in menu bar" `Switch`, and honest-quit copy; use the existing load-on-open + revert-on-error pattern (l.221-257).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add the `DockBadgeMode` ts-rs enum + registry-string conversions -- typed, exportable mode shared across the IPC boundary.
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- add dock-badge-mode and menu-bar-presence settings get/set -- durable, restart-surviving config.
- [x] `src-tauri/crates/keeper-core/src/platform.rs` -- add `set_badge_count` to the `Platform` port -- keeps core platform-free while reaching the OS dock.
- [x] `src-tauri/crates/keeper-core/src/badge.rs` -- pure `badge_count` + `apply` + `BadgeConfig`; unit-test all three modes and the zero-clears-badge edge -- testable badge logic in core.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- seed `BadgeConfig`, add mode/menu-bar get-set methods and `shutdown_all()` -- wiring + honest-quit teardown.
- [x] `src-tauri/crates/keeper-core/src/inbox.rs` -- aggregate cross-account unread/mention totals in the merger and apply the badge on every merged-state change -- background-correct badge without touching unread compute.
- [x] `src-tauri/Cargo.toml` + `src-tauri/crates/keeper/Cargo.toml` -- add `tauri-plugin-autostart` -- launch-at-login backend (AD-25).
- [x] `src-tauri/crates/keeper/src/lib.rs` -- window-close-hides, `ExitRequested` graceful `shutdown_all` + exit, `Reopen` re-show, tray build per setting, autostart plugin, register commands -- the lifecycle spine.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- implement `set_badge_count` and the 6 new commands -- IPC surface for the frontend.
- [x] `src-tauri/crates/keeper/capabilities/default.json` -- grant autostart (+ any tray/badge) permissions -- capability allowlist.
- [x] `src/lib/ipc/gen/DockBadgeMode.ts` + `src/lib/ipc/client.ts` -- regenerated binding + typed wrappers -- typed frontend access.
- [x] `src/components/settings/settings-dialog.tsx` -- badge-mode radio, launch-at-login switch, menu-bar switch, honest-quit copy -- user-facing controls and the honesty surface.

**Acceptance Criteria:**
- Given the app is running and the window is closed with ⌘W, when messages arrive on any account, then sync and native notifications behave identically to foreground, the process stays alive, and the dock badge updates per its mode (FR-53).
- Given launch-at-login offered in Settings, when the app is freshly installed and untouched, then it is disabled by default, and toggling it enables/disables the LaunchAgent through the autostart plugin (FR-53, AD-25).
- Given the user presses ⌘Q (or picks native Quit), when the app quits, then every account's `SyncService` is stopped via `shutdown_all()` and the process fully exits with no lingering background process (FR-53).
- Given the honest-quit requirement, when the user reads the Settings notifications/background copy, then it states ⌘Q stops syncing and nowhere promises push or notifications while quit (FR-53, UX-DR17).
- Given menu-bar presence is enabled, when the window is hidden, then the tray icon keeps keeper reachable (Show keeper / Quit) and the choice persists across restart; when disabled (default) no tray appears.

## Spec Change Log

_No spec amendments — no bad_spec loopback occurred during review._

## Review Triage Log

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 4: (high 0, medium 0, low 4)
- reject: 21
- addressed_findings:
  - none

_Notes: Blind Hunter + Edge Case Hunter ran in parallel with no prior context. All findings landed at low severity for the app user. Four real-but-out-of-scope robustness items were deferred to `deferred-work.md` (tray-build-failure setting divergence; sequential `shutdown_all` under a shared 3s quit bound; absent shell-integration test coverage; silent unknown→All settings coercion). The remainder were rejected: several rested on premises falsified against the code — the Story 9.4 global hotkey **does** un-hide a hidden window (`hotkey.rs::toggle_main_window` calls `show()`+`set_focus()`), tray "Quit" **is** graceful (`app.exit(0)` routes through `RunEvent::ExitRequested`), `mention_count` is unread-scoped (read rooms contribute 0, so Mentions-mode is correct), and `is_unread` includes `marked_unread` by design (AD-20); the rest were idiomatic-macOS design choices, unreachable numeric/lock edges, or behavior consistent with the project's established settings/testing patterns._

## Design Notes

- **Window is a view, the process is the app.** `SyncService`, notifications, drafts, and the archive already run as process-scoped Rust tasks under `AccountManager` (they are not tied to the webview). So foreground↔background parity is *free* once ⌘W stops destroying the process: intercept `CloseRequested`, `prevent_close()`, and `hide()` the window. The webview stays alive (so any JS timers keep ticking), but nothing load-bearing depends on that — all guarantees live in Rust.
- **The ⌘W / ⌘Q split is the whole story.** ⌘W → `WindowEvent::CloseRequested` → hide (keep syncing). ⌘Q / native Quit → `RunEvent::ExitRequested` → do NOT prevent; run a bounded `block_on(shutdown_all())` (each `shutdown` already awaits `sync.stop()` and tears down handlers/tasks — account.rs l.4260) then let the process exit. This is the honest-quit guarantee made mechanical; the Settings copy states it in words.
- **Badge in Rust, driven by merged state.** The badge must be right while the window is hidden and must decrement on reads, not just increment on arrivals — so it can't be event-handler-only and can't live in the webview. The `InboxMerger` already reflects unread/read changes across all accounts; recompute the aggregate there and push through the new `Platform::set_badge_count`. Keep the arithmetic in a pure `badge_count(mode, unread_rooms, mention_total)` fn for unit testing; `all` counts unread *rooms* (the available signal — `RoomVm.is_unread` is a bool, not a per-room message count), `mentions` sums `mention_count`, `off`/zero clears.
- **Shell owns the OS, core owns the rules.** Dock badge *mode* is core state (persisted, drives Rust aggregation) reached via `state.accounts`; the *act* of setting the OS badge, the tray, and autostart are shell/`Platform` concerns so `keeper-core` stays platform-free. Launch-at-login is authoritative in the autostart plugin (`is_enabled`/`enable`/`disable`) — do not shadow it with a second source of truth.
- **Re-summon paths.** Hidden window is re-shown by dock-click (`RunEvent::Reopen`, macOS), the existing global hotkey (Story 9.4), and the tray's "Show keeper". Notification click-through is Story 10.4 — not here.

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + `clippy --all-targets -- -D warnings` passes (no `.unwrap()` in new prod paths).
- `bun run test:rust` -- expected: new `badge_count`/registry round-trip unit tests pass under cargo-nextest.
- `bun run check` -- expected: biome + tsc + vitest pass, including the regenerated `DockBadgeMode` binding.

**Manual checks:**
- Build/run the app; close the window with ⌘W and send a message from another client → native notification still fires and the dock badge updates; the process is still alive (visible in Activity Monitor).
- Toggle each dock-badge mode → badge reflects all-unreads / mentions / cleared; mark rooms read → badge decrements.
- Enable launch-at-login, then confirm a `~/Library/LaunchAgents/*keeper*.plist` appears; disable → it is removed.
- Enable menu-bar presence → tray icon with Show keeper / Quit appears and survives restart; Quit exits fully.
- Press ⌘Q → process terminates with no lingering background process.

## Auto Run Result

Status: done

**Summary of implemented change:** The process — not the window — now owns sync. ⌘W (`WindowEvent::CloseRequested`) hides the main window while every account's `SyncService` and the notification pipeline keep running (background parity is free because sync is process-scoped). ⌘Q, the native Quit item, and tray Quit (`app.exit(0)`) all route through `RunEvent::ExitRequested`, which runs a bounded (3s) graceful `shutdown_all()` and then fully exits — no hidden background process survives. Added a Rust-computed macOS dock badge (all-unreads / mentions-only / off, default all) aggregated over the full cross-account room set in the inbox merger and pushed through a new `Platform::set_badge_count` port, so it stays correct while the window is hidden. Added opt-in menu-bar (tray) presence and opt-in launch-at-login via `tauri-plugin-autostart`, plus honest Settings copy that states ⌘Q stops syncing and that keeper never runs a background push service.

**Files changed:**
- `src-tauri/crates/keeper-core/src/vm.rs` — `DockBadgeMode { All, Mentions, Off }` ts-rs enum + registry-string conversions.
- `src-tauri/crates/keeper-core/src/registry.rs` — `get/set_dock_badge_mode` (default `All`) and `get/set_menu_bar_presence` (default false) + round-trip tests.
- `src-tauri/crates/keeper-core/src/platform.rs` — added `set_badge_count(Option<u32>)` to the `Platform` port.
- `src-tauri/crates/keeper-core/src/badge.rs` (new) — `BadgeConfig` + pure `badge_count` + `apply`; unit tests for all modes and zero-clears.
- `src-tauri/crates/keeper-core/src/account.rs` — seed `BadgeConfig`; `dock_badge_mode_get/set`, `menu_bar_presence_get/set`, `shutdown_all()`; thread platform+badge into the merger.
- `src-tauri/crates/keeper-core/src/inbox.rs` — cross-account unread/mention aggregation over the full merged set → `badge::apply` on every merged-state change (+ `reapply_badge`, badge integration test); unread compute unchanged.
- `src-tauri/crates/keeper-core/src/{lib.rs,notify.rs,auth.rs}` + `tests/archive_survives_sign_out.rs` — module registration and `set_badge_count` no-op on test Platform mocks.
- `src-tauri/Cargo.toml` + `src-tauri/crates/keeper/Cargo.toml` — add `tauri-plugin-autostart`; `tray-icon` feature.
- `src-tauri/crates/keeper/src/lib.rs` — window-close→hide, `ExitRequested` graceful quit, `Reopen` re-show, autostart plugin, tray-at-setup, 6 new commands.
- `src-tauri/crates/keeper/src/ipc.rs` — `DesktopPlatform::set_badge_count` (via `BADGE_APP` handle, honest no-op when unset) + 6 commands.
- `src-tauri/crates/keeper/src/tray.rs` (new) — opt-in tray (Show keeper / Quit), live create/destroy, `show_main_window`.
- `src-tauri/crates/keeper/capabilities/default.json` — autostart permissions.
- `src/lib/ipc/gen/DockBadgeMode.ts` (new) + `src/lib/ipc/client.ts` — regenerated binding + 6 typed wrappers.
- `src/components/settings/settings-dialog.tsx` (+ test) — "Background & dock" section: dock-badge `RadioGroup`, launch-at-login `Switch` (off), menu-bar `Switch` (off), honest-quit copy.

**Review findings breakdown:** Blind Hunter + Edge Case Hunter ran in parallel (session model). 0 intent_gap, 0 bad_spec, 0 patch applied, 4 deferred (all low), 21 rejected. No code was changed by review. Deferred (see `deferred-work.md`): tray-build-failure setting divergence; sequential `shutdown_all` under a shared 3s quit bound; absent shell-integration test coverage; silent unknown→`All` settings coercion.

**Follow-up review recommendation:** false — the final review pass made no review-driven code changes.

**Verification performed:** Implementation gates all green — `bun run check:rust` (rustfmt + clippy `-D warnings` clean), `bun run test:rust` (729 passed), `bun run check` (909 passed). Key seams independently re-verified on disk: ⌘W → `prevent_close`+`hide`; ⌘Q → bounded `shutdown_all` then exit (no `prevent_exit`); `Reopen` → show+focus; `hotkey::toggle_main_window` un-hides; tray Quit routes through `ExitRequested`; badge aggregates over the full merged set; Settings copy contains no push-while-quit promise.

**Residual risks:** the four deferred low-severity robustness items above. Window/dock/tray/copy carry macOS-first assumptions consistent with the project's stated macOS-first posture; cross-platform labeling is future work when non-macOS support lands.
