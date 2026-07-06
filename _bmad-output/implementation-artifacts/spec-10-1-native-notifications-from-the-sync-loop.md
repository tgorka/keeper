---
title: 'Native Notifications from the Sync Loop'
type: 'feature'
created: '2026-07-06'
status: 'done'
baseline_revision: 'dab9d94cc68aed98c33818d7eace2fa1f01e7812'
final_revision: '4f502d18b2e54e50d59c5dc214d70abed7c53f8d'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** keeper never surfaces new messages while backgrounded — there is a `Platform::notify` port stub and an `AD-18` mandate for a `keeper-core::notify` module, but no code posts native notifications, so a backgrounded app cannot be trusted.

**Approach:** Add a `keeper-core::notify` module that taps the account-wide post-decryption message stream (the same `OriginalSyncRoomMessageEvent` handler pattern `register_archive_handler` uses), applies minimal rules (skip own messages, skip pre-session backlog, gate on message type), and posts sender + Chat + preview through the existing `Platform::notify` port. Wire the desktop `Platform::notify` to `tauri-plugin-notification`, add a persisted "message previews" toggle in Settings, and keep every notification on-device.

## Boundaries & Constraints

**Always:**
- Notification content originates **only** from the local decrypting sync loop and is delivered **only** through the `Platform::notify` port → OS. No push gateway, no third-party/project-operated push, no network egress in `notify.rs` (NFR-11).
- All notification decision + formatting logic lives in `keeper-core::notify` (AD-18); never duplicate it in TypeScript or the shell.
- Best-effort and non-blocking: a `Platform::notify` failure is logged at `warn` and swallowed — it must never block sync, panic, or abort the account (matches the archive handler's error posture).
- Never log message bodies (NFR-9); `account_id`/`room_id`/`event_id` are safe to log.
- Rust rules hold: no `.unwrap()`/`.expect()` in production paths, `?` + `thiserror`, `tracing`, `cargo deny` passes for any new dependency.
- The previews toggle persists in the existing `settings` k/v table and survives restart; default = previews **enabled**.
- The notify handler is per-account and is removed on sign-out so a signed-out account produces no further notifications.

**Block If:**
- `tauri-plugin-notification` (or any new crate) fails the `cargo deny` license firewall — HALT (it must not be forced past the firewall).

**Never:**
- No mute / mention-only / DND / per-chat rules (Story 10.2), no background-window/dock-badge/quit semantics (Story 10.3), no click-through payload/routing or grouping/bridge-health notifications (Story 10.4). Out of scope here.
- No inline notification quick-reply (v1.x, explicitly out of scope).
- No new global **mutable** state in `keeper-core` — the previews flag lives on `AccountManager` (an `Arc<NotifyConfig>`), not a `static`. The shell's write-once `OnceLock<AppHandle>` is the only permitted global (Tauri's app handle, mirroring `sidecar_path` reaching process state).
- Do not route media bytes or full event content anywhere new; the preview is a short derived string only.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| New text msg, previews on | live `m.room.message` (text), sender ≠ self, `origin_ts ≥ baseline` | one `Platform::notify` with Chat in title and `"{sender}: {preview}"` body | No error expected |
| Previews off | same, previews disabled | `Platform::notify` shows sender + Chat, **no** message content (body e.g. `"New message"`) | No error expected |
| Own echo | sender == this account's own user id | no notification | No error expected |
| Startup backlog | `origin_ts < baseline` (initial-sync history) | no notification (no launch storm) | No error expected |
| Media msg, previews on | image/file/audio/video/location | body preview is a type descriptor (e.g. `"Photo"`), never a filename/URL/body leak | No error expected |
| Notifier fails | `Platform::notify` returns `Err` | logged at `warn`, sync continues | Swallow error; never panic/propagate |
| Non-notifying type | verification-request / server-notice message type | no notification | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/notify.rs` -- **new**: `NotifyConfig` (holds `AtomicBool previews_enabled`), pure rule fns (`should_notify`, `preview_for`, `format_notification`), a testable `dispatch(&dyn Platform, &NotifyConfig, ctx)`, and `register_notify_handler(&Client, account_id, Arc<dyn Platform>, Arc<NotifyConfig>) -> EventHandlerHandle`.
- `src-tauri/crates/keeper-core/src/lib.rs` -- add `pub mod notify;`.
- `src-tauri/crates/keeper-core/src/registry.rs` -- add `get_notify_previews`/`set_notify_previews` over `settings` key `notify.previews_enabled` (mirror `get_incognito_global`/`set_incognito_global`, default `true`).
- `src-tauri/crates/keeper-core/src/account.rs` -- thread notify: add `notify: Arc<NotifyConfig>` field to `AccountManager` (seed from `registry::get_notify_previews` in `new`); add `notify_config` param to `activate`, register the handler there, extend the `ActivatedAccount` tuple (l.212) with `notify_handler`; add `notify_handler: EventHandlerHandle` to `AccountHandle` (l.226); wire all **8** activate/`AccountHandle` sites (destructure + construct) exactly like `archive_handler`; drop `notify_handler` in `shutdown` (l.~4107); add `notify_previews_get()/notify_previews_set(&Platform, bool)` methods.
- `src-tauri/crates/keeper-core/src/platform.rs` -- `Platform::notify` already declared; no signature change.
- `src-tauri/Cargo.toml` + `src-tauri/crates/keeper/Cargo.toml` -- add `tauri-plugin-notification = "2"` (workspace) / `{ workspace = true }`.
- `src-tauri/crates/keeper/src/lib.rs` -- register `tauri_plugin_notification::init()`; in `setup` store the app handle via `ipc::set_notify_app_handle(app.handle().clone())` and best-effort request notification permission; add the two new commands to `invoke_handler!`.
- `src-tauri/crates/keeper/src/ipc.rs` -- module `static NOTIFY_APP: OnceLock<AppHandle>` + `set_notify_app_handle`; implement `DesktopPlatform::notify` via `tauri_plugin_notification::NotificationExt` (honest `Unsupported` when the handle is unset, e.g. headless); add `notify_get_preview_enabled`/`notify_set_preview_enabled` commands (mirror `incognito_get_global`/`incognito_set_global`).
- `src-tauri/crates/keeper/capabilities/default.json` -- add `"notification:default"`.
- `src/lib/ipc/client.ts` -- add `notifyGetPreviewEnabled()` / `notifySetPreviewEnabled(enabled)` (mirror `incognitoGetGlobal`/`incognitoSetGlobal`).
- `src/components/settings/settings-dialog.tsx` -- add a "Notifications" section with a `Switch` ("Show message previews"), loaded on open and toggled through the new client wrappers.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/notify.rs` -- create the module: `NotifyConfig` with `new(previews_enabled: bool)`, `previews_enabled() -> bool`, `set_previews_enabled(bool)` over an `AtomicBool`; pure `should_notify(is_self: bool, event_ts_ms: u64, baseline_ms: u64, notifies: bool) -> bool`; `preview_for(&MessageType) -> String` (text/notice/emote → body; media types → descriptor; non-notifying types → `notifies=false`); `format_notification(chat: &str, sender: &str, preview: &str, previews_enabled: bool) -> (String, String)`; `dispatch(&dyn Platform, &NotifyConfig, ctx)` running rule→format→`platform.notify`, swallowing errors with `tracing::warn!`; `register_notify_handler(...)` extracting `(sender, chat, ts, msgtype)` best-effort (member display name → localpart fallback; `room.display_name().await` → room_id fallback; own id via `client.user_id()`), capturing `baseline_ms` at registration -- rationale: AD-18 single home for notification logic, SDK glue thin over pure testable core.
- [x] `src-tauri/crates/keeper-core/src/lib.rs` -- `pub mod notify;` -- rationale: expose the module.
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- add `get_notify_previews(&Path) -> Result<bool>` (default `true`) and `set_notify_previews(&Path, bool)` over key `notify.previews_enabled` -- rationale: persist the toggle across restarts using the existing settings table.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- thread `notify_handler`/`NotifyConfig` through `AccountManager`, `activate`, the `ActivatedAccount` tuple, `AccountHandle`, all 8 construction sites, and `shutdown`; add `notify_previews_get`/`notify_previews_set` -- rationale: per-account handler lifecycle + app-wide toggle without global mutable state.
- [x] `src-tauri/Cargo.toml`, `src-tauri/crates/keeper/Cargo.toml` -- add `tauri-plugin-notification` dep; run `cargo deny check` -- rationale: native notifier sink; license firewall must pass.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- `NOTIFY_APP` OnceLock + setter, real `DesktopPlatform::notify`, two toggle commands -- rationale: wire the port to the OS notifier and expose the setting to the UI.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register plugin, set app handle in setup, best-effort permission request, register commands -- rationale: shell glue only.
- [x] `src-tauri/crates/keeper/capabilities/default.json` -- add `notification:default` -- rationale: grant the notification capability.
- [x] `src/lib/ipc/client.ts` -- add the two typed wrappers -- rationale: typed IPC surface for the UI.
- [x] `src/components/settings/settings-dialog.tsx` -- add Notifications section with previews `Switch` -- rationale: the Settings preview control the ACs require.
- [x] `src-tauri/crates/keeper-core/src/notify.rs` (tests) -- unit-test every I/O-matrix row: `should_notify` (self/backlog/type gates), `preview_for` per `MessageType`, `format_notification` (previews on ⇒ content present; off ⇒ sender+Chat, no content), and `dispatch` against a capturing `Platform` double (records `(title, body)`; assert own/backlog/failure behavior) -- rationale: the SDK-free core carries the correctness contract.
- [x] `src/components/settings/settings-dialog.test.tsx`, `src/lib/ipc/client.test.ts` -- cover the toggle load/set and the new wrappers -- rationale: frontend coverage gate.

**Acceptance Criteria:**
- Given a backgrounded (running, window-closed) account whose live sync delivers a new decrypted `m.room.message` from another user, when the notify handler runs, then exactly one native notification posts through `Platform::notify` carrying sender + Chat (+ preview) sourced only from the local decrypting loop, and the code path contains no push/network egress.
- Given previews are disabled in Settings, when a notification posts, then it shows the sender and Chat but no message content, and the setting persists across an app restart.
- Given a message the local account itself sent, or an event from the pre-session initial-sync backlog, when the handler runs, then no notification is posted (no self-notify, no launch storm).
- Given the account is signed out, when subsequent events would arrive, then its notify handler has been removed and it produces no further notifications.
- Given `cargo deny check`, `bun run check:rust`, `bun run test:rust`, and `bun run check` all run, then all pass.

## Design Notes

- **Backlog suppression (baseline):** capture `baseline_ms` (client clock) when the handler is registered at activation; notify only when `event.origin_server_ts >= baseline_ms`. This drops cold-launch history (desired — the inbox already shows it) while still notifying messages that arrive during a live background session or after a reconnect (the handler is registered once per account lifetime, not per reconnect). Accepted MVP edge: gross client-ahead clock skew could drop a genuinely-new notification; finer control is deferred.
- **Testable seam:** keep SDK extraction (`register_notify_handler`) thin and push all decisions into pure fns + `dispatch`, so a capturing `Platform` double covers the matrix without a homeserver (existing `FakePlatform` doubles in `account.rs`/`auth.rs` are the model; the test double records calls into an `Arc<Mutex<Vec<(String,String)>>>`).
- **Notification shape (golden):** with previews on, `format_notification("Alice", "Alice", "hey there", true)` → `("Alice", "Alice: hey there")` (collapse title when sender == Chat, i.e. DMs); with a group Chat → title = Chat, body = `"{sender}: {preview}"`; previews off → body = the **sender name** (title still carries the Chat), so both sender and Chat show with no content — a fixed `"New message"` would drop the sender in a group Chat, violating the AC. Exact strings follow the sentence-case, no-exclamation voice.
- **Not a new message:** an edit (`m.replace`) arrives as a fresh `m.room.message`; the handler skips events whose `relates_to` is a `Replacement` (mirrors the archive handler), and a whitespace-only/empty body is treated as non-notifying. Empty resolved display names fall back to localpart/room-id.
- **Shell handle:** `DesktopPlatform` stays a unit struct; `NOTIFY_APP: OnceLock<AppHandle>` is set once in `setup`. When unset (headless/CI), `notify` returns `CoreError::Unsupported` honestly rather than panicking.

## Verification

**Commands:**
- `cd src-tauri && cargo deny check` -- expected: no license/ban violations for `tauri-plugin-notification`.
- `bun run check:rust` -- expected: `cargo fmt --check` clean + `clippy --all-targets -- -D warnings` clean.
- `bun run test:rust` -- expected: all cargo-nextest tests pass, including the new `notify` unit tests.
- `bun run check` -- expected: biome + tsc + vitest pass, including settings-dialog/client tests.

**Manual checks (if no CLI):**
- Notification permission prompt appears once on first run; with previews toggled off in Settings, a delivered message shows sender + Chat but no body text.

## Review Triage Log

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 2, low 2)
- defer: 2
- reject: 9
- addressed_findings:
  - `[medium]` `[patch]` Notifications fired on message edits (`m.replace`) with the `* edited text` body — added a `Relation::Replacement` skip in the handler, mirroring the archive handler.
  - `[medium]` `[patch]` Previews-off dropped the sender in group Chats, violating the AC and the shipped Settings copy ("show the sender and chat, never the text") — previews-off body now carries the sender name.
  - `[low]` `[patch]` A whitespace-only/empty body still notified as `"{sender}: "` — empty text/notice/emote is now non-notifying.
  - `[low]` `[patch]` `Ok("")`/empty resolved display names bypassed the fallbacks — empty sender/Chat names now fall back to localpart/room-id.

## Auto Run Result

Status: done

**Summary:** Added `keeper-core::notify` (AD-18): an account-wide post-decryption tap on the sync loop that posts native macOS notifications (sender + Chat + preview) through the existing `Platform::notify` port — now wired to `tauri-plugin-notification` — with a persisted, Settings-toggleable message-previews control. All content stays on-device (no push infrastructure). Rules: skip own echoes, suppress pre-session backlog (baseline captured at registration), skip edits/non-notifying types, honor the previews toggle. Handler is per-account and removed on sign-out.

**Files changed:**
- `src-tauri/crates/keeper-core/src/notify.rs` (new) — NotifyConfig + pure rules (`should_notify`/`preview_for`/`format_notification`) + testable `dispatch` + `register_notify_handler` SDK glue; 23 unit tests.
- `src-tauri/crates/keeper-core/src/lib.rs` — `pub mod notify;`.
- `src-tauri/crates/keeper-core/src/registry.rs` — `get/set_notify_previews` over `settings` key `notify.previews_enabled`.
- `src-tauri/crates/keeper-core/src/account.rs` — threaded `notify_handler`/`NotifyConfig` through `ActivatedAccount`, `AccountHandle`, all 8 activate/construct sites, `activate`, `shutdown`, plus `notify_previews_get/set`.
- `src-tauri/Cargo.toml`, `src-tauri/crates/keeper/Cargo.toml` — `tauri-plugin-notification = "2"`.
- `src-tauri/crates/keeper/src/ipc.rs` — `NOTIFY_APP` OnceLock + setter, real `DesktopPlatform::notify`, `notify_get/set_preview_enabled` commands.
- `src-tauri/crates/keeper/src/lib.rs` — plugin registration, app-handle set in setup, best-effort permission request, command registration.
- `src-tauri/crates/keeper/capabilities/default.json` — `notification:default`.
- `src/lib/ipc/client.ts` (+ test) — `notifyGetPreviewEnabled`/`notifySetPreviewEnabled`.
- `src/components/settings/settings-dialog.tsx` (+ test) — Notifications section with previews `Switch`.

**Review findings breakdown:** 4 patches applied (2 medium: edit-notification skip, previews-off sender retention; 2 low: empty-body suppression, empty display-name fallbacks). 2 deferred (see deferred-work). 9 rejected as cosmetic/accepted/unreachable (clock-skew drop direction — documented accepted edge; self-notify when `user_id()` is None — unreachable post-restore; emote phrasing; `Relaxed` ordering; permission re-check; config-set race; DM-collapse false positive; test-lock `expect`; `display_name` cost). No intent_gap, no bad_spec — no repair loopback (`review_loop_iteration` = 0).

**Verification:** `bun run check:rust` exit 0 (fmt + clippy `-D warnings`); `bun run test:rust` 700/700 nextest; `bun run check` green (biome + tsc + 892 vitest + tauri-free guard); `cargo deny check licenses bans` → `bans ok, licenses ok` (the advisory-only failure is 39 pre-existing gtk-rs RUSTSEC advisories transitive through Tauri, identical on baseline — not introduced here).

**Residual risks:** New-vs-backlog classification uses a client-clock baseline vs server `origin_server_ts` and does not track read markers, so a long offline gap can re-notify messages already read elsewhere (deferred). No focus/foreground suppression yet — a notification can fire for the Chat currently on screen (deferred). Notification permission is requested best-effort; a denied grant means the OS silently drops notifications.
