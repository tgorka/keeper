---
title: 'Mutes, Mention-Only, and Do-Not-Disturb'
type: 'feature'
created: '2026-07-06'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '913b47935821117b2c876a29c42395f5241a81ba'
final_revision: 'fb08eff7007441003c2e5ee565e9b62945f992e7'
context: []
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** Story 10.1 shipped the notification pipeline but it notifies on *every* qualifying message. keeper has no way to quiet noise: no per-Chat mute, no mention-only mode, no per-Network mute, and no global do-not-disturb (FR-52). A backgrounded app that cannot be quieted is as untrustworthy as one that cannot notify.

**Approach:** Extend `keeper-core::notify` with a granular suppression layer. Per-Chat mute and mention-only map to **Matrix push rules** (via matrix-sdk `NotificationSettings`, so they sync across devices and survive restarts); the notify handler honors them by treating "the room's synced push rules elected to notify this event" (`Room::event_push_actions` → contains `Action::Notify`) as a gate. Global DND and per-Network mute are **keeper-local** state on `NotifyConfig`, persisted in `keeper.db`, evaluated locally. Suppression never touches unread accumulation — muted Chats keep updating in the inbox. Surface controls in the chat context menu, network chip menu, sidebar-footer DND toggle, single-key `m`, and the command palette; show a mute glyph on affected rows.

## Boundaries & Constraints

**Always:**
- Suppression is notification-only. Muted / mention-only / DND-silenced Chats **keep updating in the inbox and keep accumulating unread + mention state** — mute never writes read markers or touches unread (FR-52). No new logic in the inbox unread path beyond reading mute state to render the glyph.
- Per-Chat mute and mention-only are persisted **as Matrix push rules** through `client.notification_settings().set_room_notification_mode(...)` (representable → synced + durable). Global DND and per-Network mute are keeper-local, persisted in the `settings`/`muted_networks` tables in `keeper.db` and survive restart.
- All suppression decisions live in `keeper-core::notify` (AD-18) — never duplicated in TypeScript or the shell. The frontend only reads/writes state through typed IPC and renders the glyph.
- The notify handler stays best-effort and non-blocking: a push-rule/network/mode read failure is logged at `warn` and treated as "notify" (fail-open to the 10.1 behavior) — it must never block sync, panic, or abort the account.
- Never log message bodies (NFR-9); `account_id`/`room_id`/`network_id`/`event_id` are safe to log. No push/network egress in `notify.rs` beyond the matrix-sdk push-rule writes that ride the existing account session (NFR-11).
- Rust rules hold: no `.unwrap()`/`.expect()` in production paths, `?` + `thiserror`, `tracing`; no new global mutable state (DND + muted-networks live on the existing `Arc<NotifyConfig>`, not a `static`).
- Voice/tone: Settings/menu copy is sentence case, no exclamation marks, honest state narration, Glossary-capitalized nouns (Chat, Network).

**Block If:**
- matrix-sdk 0.18 does not expose `NotificationSettings::set_room_notification_mode` / `get_user_defined_room_notification_mode` / `unmute_room` or `Room::event_push_actions` as expected during implementation — HALT with status `blocked` (the per-Chat-rule mechanism is load-bearing; do not hand-roll a divergent push-rule writer).

**Never:**
- No click-through payload/routing, per-Chat grouping, or bridge-health notifications (Story 10.4). No background-window / dock-badge / launch-at-login / quit semantics (Story 10.3). No focus/foreground active-room suppression (deferred, Story 10.3 territory). No changes to the previews toggle behavior (Story 10.1).
- No detail-panel per-Chat mute controls this story — `detail-panel.tsx` is still a placeholder stub; building it out is a separate UI story. Mute is reachable via context menu + single-key `m` + palette instead. (Log to deferred-work.)
- No keeper-invented push rules beyond what `set_room_notification_mode`/`unmute_room` write; do not fan out custom server-side rules.
- Global DND is a single on/off silence-everything switch — not a scheduler, not per-account, no time windows.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Normal msg, no rules | live `m.room.message`, room mode default, DND off, network not muted | one notification (unchanged 10.1 behavior) | No error expected |
| Chat muted | room `RoomNotificationMode::Mute`; `event_push_actions` has no `Action::Notify` | no notification; inbox row still updates + unread still accrues; row shows mute glyph | No error expected |
| Chat mention-only, non-mention | room `MentionsAndKeywordsOnly`; event is a plain message (no `Action::Notify`) | no notification; row shows mention-only glyph | No error expected |
| Chat mention-only, mention/reply | room `MentionsAndKeywordsOnly`; event mentions/ replies-to user (`Action::Notify` present) | one notification | No error expected |
| Network muted | room's `room_bridge_network` id is in the muted-network set | no notification; row shows mute glyph; unread still accrues | No error expected |
| Global DND on | `NotifyConfig.dnd_enabled` true | no notification for any account/Chat; unread everywhere still accrues; no per-row glyph from DND alone | No error expected |
| Own echo / backlog / non-notifying type | (10.1 gates) | no notification | No error expected |
| Push-rule read fails | `event_push_actions` returns `Err` | fail-open: notify as if unmuted (10.1 behavior); logged at `warn` | Swallow; never panic |
| Set mode / mute network | IPC set command with unknown room / inactive account | best-effort; returns `timelineUnavailable` envelope only on unknown room/inactive account | Propagate typed error |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/notify.rs` -- extend `NotifyConfig` with `dnd_enabled: AtomicBool` + `muted_networks: RwLock<HashSet<String>>` (+ `dnd_enabled()`/`set_dnd_enabled()`/`is_network_muted()`/`set_network_muted()`/`replace_muted_networks()`); add `room_push_notifies` + `network_muted` fields to `NotifyContext`; extend pure `should_notify(...)` with `room_push_notifies`, `dnd_enabled`, `network_muted` gates; in `register_notify_handler`, add a `raw: RawEvent` handler-context param (matrix-sdk `event_handler::RawEvent`, derefs to the raw JSON), resolve the room's network (`bridge::room_bridge_network`) and evaluate `room.event_push_actions(&Raw::from_json(raw.0.clone()))` → `Action::Notify` present, fail-open on error; feed both into `dispatch`.
- `src-tauri/crates/keeper-core/src/registry.rs` -- add `get_dnd_global`/`set_dnd_global` (settings key `notify.dnd_global`, default `false`); new `muted_networks(network_id TEXT PRIMARY KEY)` table with `get_muted_networks(&Path) -> Vec<String>` and `set_network_muted(&Path, network_id, bool)` (insert/delete row). Mirror the `notify.previews_enabled` / `chat_incognito` patterns.
- `src-tauri/crates/keeper-core/src/account.rs` -- seed `dnd_enabled` + `muted_networks` into the shared `NotifyConfig` in `AccountManager::new` (from registry); add methods `dnd_get()`, `dnd_set(&Platform, bool)`, `network_mute_get(network_id)`, `network_mute_set(&Platform, network_id, bool)` (persist-then-apply, like `notify_previews_set`); add `chat_notify_mode_get(account_id, room_id) -> ChatNotifyMode` and `chat_notify_mode_set(account_id, room_id, ChatNotifyMode)` that reach the account's `Client` and call `notification_settings()` (mirror how `archive_room`/`set_is_low_priority` resolve a room). Define `ChatNotifyMode { All, MentionOnly, Mute }` ⇄ `RoomNotificationMode`.
- `src-tauri/crates/keeper-core/src/inbox.rs` -- add `mute_state` to the emitted room view model: read `get_user_defined_room_notification_mode` per room (Mute → `muted`, MentionsAndKeywordsOnly → `mention_only`, else check muted-network set → `muted`, else `none`); fail-open to `none` on error. Must not alter unread computation.
- `src-tauri/crates/keeper-core/src/vm.rs` (or wherever `InboxRoomVm` is defined) -- add `mute_state: MuteState` (`ts-rs`-exported enum `none|muted|mention_only`).
- `src-tauri/crates/keeper/src/ipc.rs` -- add commands: `chat_notify_mode_get/set`, `network_mute_get/set`, `dnd_get_global/set_global` (mirror the `notify_get/set_preview_enabled` command shape).
- `src-tauri/crates/keeper/src/lib.rs` -- register the six new commands in `invoke_handler!`.
- `src/lib/ipc/client.ts` -- typed wrappers: `chatNotifyModeGet/Set`, `networkMuteGet/Set`, `dndGetGlobal/SetGlobal`.
- `src/lib/ipc/gen/InboxRoomVm.ts` + `MuteState.ts` -- regenerated bindings carrying `muteState`.
- `src/components/chat/chat-row.tsx` -- render a mute glyph (lucide `BellOff` for `muted`, `AtSign`/`BellDot` for `mention_only`) near the name; add a "Notifications" submenu to the context menu (Mute / Mentions only / All) reflecting `room.muteState`.
- `src/components/layout/networks-group.tsx` -- add a per-Network mute affordance (context menu / small menu button) calling `networkMuteSet`; show muted state on the chip.
- `src/components/layout/account-footer.tsx` -- add a global "Do not disturb" toggle row to the footer dropdown menu (read `dndGetGlobal`, set `dndSetGlobal`), mirroring the Story 8.1 incognito-global toggle pattern.
- `src-tauri/crates/keeper-core/src/palette.rs` -- register `mute-chat` / `mention-only-chat` / `unmute-chat` actions (toggle group) with single-key hint `m`.
- `src/components/command-palette/actions.ts` -- map the three action ids to `chatNotifyModeSet(...)` dispatches using the active-chat context.
- `src/hooks/` (new `use-chat-list-verbs.ts` or extend the focused-list handler) -- single-key `m` on the focused list opens the mute menu (mute / mention-only / unmute) for the active Chat.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/notify.rs` -- extend `NotifyConfig` (DND + muted-networks), `NotifyContext` (`room_push_notifies`, `network_muted`), pure `should_notify` gates, and `register_notify_handler` push-actions + network resolution (fail-open) -- rationale: AD-18 single home for suppression; keep decisions in pure testable fns.
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- `get/set_dnd_global` + `muted_networks` table with `get_muted_networks`/`set_network_muted` -- rationale: keeper-local rules persist across restarts.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- seed config from registry; add `dnd_get/set`, `network_mute_get/set`, `chat_notify_mode_get/set` + `ChatNotifyMode` ⇄ `RoomNotificationMode` -- rationale: thread state + reach the per-account `Client` for push-rule writes without global mutable state.
- [x] `src-tauri/crates/keeper-core/src/inbox.rs` + `vm.rs` -- add `mute_state` to `InboxRoomVm`, computed from room mode + muted-network set (fail-open `none`), unread path untouched -- rationale: the mute glyph the AC requires.
- [x] `src-tauri/crates/keeper/src/ipc.rs`, `src-tauri/crates/keeper/src/lib.rs` -- six new commands + registration -- rationale: shell glue exposing the settings to the UI.
- [x] `src/lib/ipc/client.ts` (+ regenerated `gen/`) -- typed wrappers for the six commands and the `MuteState` binding -- rationale: typed IPC surface.
- [x] `src/components/chat/chat-row.tsx` -- mute glyph + Notifications context submenu -- rationale: primary per-Chat control + the row affordance (FR-52).
- [x] `src/components/layout/networks-group.tsx` -- per-Network mute affordance -- rationale: the per-Network mute entry point.
- [x] `src/components/layout/account-footer.tsx` -- global DND toggle in the footer menu -- rationale: the sidebar-footer DND toggle the AC names.
- [x] `src-tauri/crates/keeper-core/src/palette.rs` + `src/components/command-palette/actions.ts` + focused-list `m` handler -- mute actions + single-key `m` mute menu -- rationale: palette parity + the single-key verb.
- [x] `src-tauri/crates/keeper-core/src/notify.rs` (tests) -- unit-test every I/O-matrix row: `should_notify` with the new gates (chat-muted, mention-only non-mention vs mention, network-muted, DND, fail-open), against the existing `CapturingPlatform` double; `dispatch` honoring the new `NotifyContext` fields -- rationale: SDK-free core carries the correctness contract.
- [x] Frontend tests -- `chat-row` glyph + Notifications submenu, `account-footer` DND toggle, `client.ts` wrappers, and `mute_state` rendering -- rationale: frontend coverage gate.

**Acceptance Criteria:**
- Given a Chat muted via its context menu (or single-key `m` / palette), when new messages arrive for it, then it produces zero notifications while its row keeps updating in the inbox and keeps accumulating unread + mention state, and the row shows a mute glyph; the mute persists across an app restart (it is a synced Matrix push rule).
- Given a Chat set to mention-only, when a plain message arrives it does not notify, but when a message mentions the user or replies to the user it notifies exactly once.
- Given a Network muted from the network chip menu, when messages arrive in that Network's Chats, then no notifications post while those Chats keep updating and accumulating unread, and their rows show the mute glyph; the muted-Network state persists across restart.
- Given the global Do-Not-Disturb toggle in the sidebar footer menu is on, when any message arrives on any account, then no notification posts while all Chats keep updating and accumulating unread; turning it off restores notifications; the DND state persists across restart.
- Given `cargo deny check`, `bun run check:rust`, `bun run test:rust`, and `bun run check` all run, then all pass.

## Design Notes

- **Two-tier rule model (the crux of "mapped to push rules where representable, evaluated locally otherwise"):** per-Chat mute and mention-only are *representable* as Matrix push rules, so they go through `NotificationSettings` and the handler reads the verdict back via `event_push_actions` — this makes them sync across devices and survive restart for free, and folds mention/reply detection into the standard ruleset (`.m.rule.is_user_mention`, reply rules) instead of hand-parsing `m.mentions`. Global DND (a whole-app silence) and per-Network mute (no Matrix "network" concept) are *not* representable, so they are keeper-local `NotifyConfig` state gated in the pure decision fn.
- **Decision fn (golden):** `should_notify(is_self, ts, baseline, notifies, room_push_notifies, dnd_enabled, network_muted)` = `!is_self && notifies && ts >= baseline && room_push_notifies && !dnd_enabled && !network_muted`. The three 10.1 gates are unchanged; the three new gates AND in. `room_push_notifies` = `event_push_actions(&Raw::from_json(raw.0.clone()))?.unwrap_or_default().iter().any(|a| matches!(a, Action::Notify))` (the `RawEvent` context arg supplies the JSON; the handler already takes `OriginalSyncRoomMessageEvent` + `Room`), defaulting to `true` on any read error (fail-open — never silently drop a genuine notification because a rule lookup failed).
- **Fail-open, not fail-closed:** every new read (push actions, room mode, network resolution) that errors is logged at `warn` and treated as "would notify" / "not muted". Mute is a comfort feature; dropping a real notification on a transient error is worse than an occasional over-notify.
- **`mute_state` on the row ≠ the notify decision:** the glyph reflects durable per-Chat/per-Network mute intent (mute / mention-only), computed at inbox emit; it deliberately does *not* reflect global DND (DND is shown once in the footer, not stamped on every row) and does not gate unread. Keep the inbox unread computation byte-for-byte unchanged.
- **Reaching the per-account Client for push-rule writes:** mirror the existing `archive_room` path (`Room::set_is_low_priority`) — resolve `(account_id, room_id)` to the live `Room`/`Client`, then call `notification_settings()`. `ChatNotifyMode::All` → `set_room_notification_mode(AllMessages)`, `MentionOnly` → `MentionsAndKeywordsOnly`, `Mute` → `set_room_notification_mode(Mute)`; "unmute" resolves to `All`.

## Verification

**Commands:**
- `cd src-tauri && cargo deny check` -- expected: no new license/ban violations (no new crates added; matrix-sdk `NotificationSettings` is already in-tree).
- `bun run check:rust` -- expected: `cargo fmt --check` clean + `clippy --all-targets -- -D warnings` clean.
- `bun run test:rust` -- expected: all cargo-nextest tests pass, including the new `notify` suppression tests.
- `bun run check` -- expected: biome + tsc + vitest pass, including chat-row/account-footer/client tests and regenerated bindings.

**Manual checks (if no CLI):**
- Mute a Chat → its row shows the glyph, unread badge still climbs on new messages, no notification fires; restart the app → still muted. Toggle footer DND → no notifications anywhere until toggled off.

## Spec Change Log

(No bad_spec loopback — empty.)

## Review Triage Log

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 1, medium 1, low 1)
- defer: 2
- reject: 10
- addressed_findings:
  - `[high]` `[patch]` `resolve_mute_state` opened a fresh SQLite connection (WAL + ~12 `CREATE TABLE`) via `registry::get_muted_networks` once **per room per VectorDiff** on the inbox hot path (a Reset over N rooms → N DB opens on the producer task). Threaded the in-memory `Arc<NotifyConfig>` through `run_producer`/`run_inbox_producer`→`diff_to_op`→`map_vector_diff`→`room_item_to_vm`→`resolve_mute_state` (replacing `data_dir`) and now read `config.is_network_muted(net)` — zero DB I/O on the stream.
  - `[medium]` `[patch]` The glyph path read the muted-Network set from disk while the notify path read the in-memory `NotifyConfig`, so the two could disagree transiently. The same fix unifies both surfaces onto the single in-memory `NotifyConfig` source, and the misleading `network_mute_set` doc comment (claimed the glyph updates "immediately") was corrected to state the glyph refreshes at the next inbox emit.
  - `[low]` `[patch]` The notify handler evaluated `event_push_actions` (raw-JSON clone + push-rule eval) and `room_bridge_network` (state-store read) **before** the cheap self/backlog/type/DND early-out, so every own-echo and backlog message paid work the 10.1 handler avoided. Reordered so the I/O-free gates short-circuit first; the push-rule and Network reads run only for surviving candidates.

## Auto Run Result

Status: done

**Summary:** Extended `keeper-core::notify` (Story 10.2, AD-18) with a granular suppression layer over the 10.1 pipeline. Per-Chat **mute** and **mention-only** persist as Matrix push rules (`NotificationSettings::set_room_notification_mode`, modes `Mute`/`MentionsAndKeywordsOnly`/`AllMessages`), so they sync across devices and survive restart; the notify handler honors them by gating on `Room::event_push_actions` containing `Action::Notify` (which also folds in mentions/replies). **Global DND** and **per-Network mute** are keeper-local `NotifyConfig` state (`AtomicBool` + `RwLock<HashSet<String>>`), persisted in `keeper.db` (`settings` k/v + a `muted_networks` table) and evaluated locally. Suppression never touches unread — muted Chats keep updating and accumulating unread/mention state. Controls surfaced in the chat context menu (Notifications submenu), the network chip menu, a sidebar-footer global DND toggle, single-key `m` (cycles All→Mentions only→Mute), and the command palette; a mute glyph (`BellOff`/`AtSign`) marks affected rows via a new `mute_state` on the room view model.

**Files changed:**
- `src-tauri/crates/keeper-core/src/notify.rs` — `NotifyConfig` gains DND flag + muted-network set (+ accessors, poison-recovering); `NotifyContext` gains `room_push_notifies`/`network_muted`; `should_notify` ANDs the three new gates (fail-open); handler takes a `RawEvent`, evaluates push actions + Network, with cheap gates short-circuiting first.
- `src-tauri/crates/keeper-core/src/registry.rs` — `get/set_dnd_global` (`notify.dnd_global`) + `muted_networks` table with `get_muted_networks`/`set_network_muted`/`is_network_muted`.
- `src-tauri/crates/keeper-core/src/account.rs` — seed DND + muted set into the shared `NotifyConfig`; `dnd_get/set`, `network_mute_get/set` (persist-then-apply), async `chat_notify_mode_get/set` (reach the account `Client`); `mute_state` computed in `room_item_to_vm` from the in-memory config; producer chain threads `NotifyConfig` (not `data_dir`).
- `src-tauri/crates/keeper-core/src/vm.rs` — ts-rs enums `MuteState` (`none|muted|mention_only`) + `ChatNotifyMode` (`all|mention_only|mute`); `mute_state` on `RoomVm`/`InboxRoomVm`.
- `src-tauri/crates/keeper-core/src/inbox.rs` — copy `mute_state` through the inbox merge.
- `src-tauri/crates/keeper-core/src/palette.rs` — `mute-chat`/`mention-only-chat`/`unmute-chat` actions (`M`); parity test updated.
- `src-tauri/crates/keeper/src/ipc.rs`, `.../keeper/src/lib.rs` — six commands + registration.
- `src/lib/ipc/client.ts` (+ `gen/MuteState.ts`, `gen/ChatNotifyMode.ts`, `gen/InboxRoomVm.ts`, `gen/RoomVm.ts`) — typed wrappers + bindings.
- `src/components/chat/chat-row.tsx` — mute glyph + Notifications context submenu.
- `src/components/layout/networks-group.tsx` — per-Network mute affordance + glyph.
- `src/components/layout/account-footer.tsx` — global DND toggle row.
- `src/components/command-palette/actions.ts`, `src/components/layout/chat-list-pane.tsx` — palette handlers + single-key `m` cycle.
- Test fixtures across `src/**` gained `mute_state`; new unit/component tests for the suppression matrix, registry CRUD, wire contracts, glyph, submenu, DND toggle, and client wrappers.

**Review findings breakdown:** 3 patches applied (high: per-room SQLite open on the inbox hot path → in-memory `NotifyConfig`; medium: glyph/notify source unification + honest doc; low: notify-handler gate reordering). 2 deferred (stale per-Network glyph until next diff — needs pins-style live re-emit infra; context-menu radio + `m` cycle conflate network-mute with per-Chat rule — needs a small UX decision). 10 rejected as guarded/by-design/accepted-MVP/single-window (m-in-input guarded by list scope; Network keyed by display name; push-rule sync lag; unmute→explicit-All; RwLock poison handled fail-open; DND in-memory multi-window; two-footer DND divergence; best-effort swallow). No intent_gap, no bad_spec — no repair loopback (`review_loop_iteration` = 0).

**Verification:** `bun run check:rust` exit 0 (fmt + clippy `-D warnings`); `bun run test:rust` 718/718 nextest; `bun run check` green (biome + tsc + 909 vitest across 90 files + tauri-free guard); `cargo deny check bans licenses sources` → `bans ok, licenses ok, sources ok` (no new crates; the advisory-only failures are the pre-existing gtk-rs RUSTSEC advisories transitive through Tauri, identical on baseline). Patches re-verified: check:rust + test:rust green after re-run.

**Residual risks:** Per-Network mute keys on the Network display label (NetworkVm carries no stable id cross-account), so same-named Networks across accounts share mute state — documented, matches the Networks-sidebar selector. Per-Network glyph freshness lags the (immediate, correct) notification suppression until a room next emits a diff (deferred). Per-Chat controls misrepresent a Network-sourced mute as a Chat rule on that edge (deferred). Push-rule-based per-Chat modes trail a `set_room_notification_mode` write by sync latency (inherent to the representable-as-push-rules design). The 10.1 client-clock backlog baseline and the lack of focus/active-room suppression remain accepted/deferred from prior stories.
