---
title: 'Story 1.5 — Timeline View: Receive Text'
type: 'feature'
created: '2026-07-04'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '85dcdd36cddc751de2f58ad97e254356077c4c89'
final_revision: '3383e96c46a337839c35ebbe1bbdcf99f68ac591'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** After Story 1.4 the chat list populates and rows are clickable, but selecting a room does nothing — the conversation pane is a static "Select a conversation to start reading." placeholder. Epic 1's vertical slice (and Story 1.6 send) is blocked until a selected Chat streams its live message history, proving the per-room `Timeline` snapshot-then-diff seam (AD-4/AD-8/AD-9) on the real MSC4186 sync path.

**Approach:** Add a per-open-room timeline subscription to the existing single-account supervision layer. Clicking a chat row records a `selectedRoomId`; the conversation pane subscribes over a `tauri::ipc::Channel`. In `keeper-core`, `AccountManager::subscribe_timeline` reuses the live (or lazily-activated) `Client`, obtains the room's matrix-sdk-ui `Timeline` (`room.timeline().await`), and drives `Timeline::subscribe()` — emitting the initial `Vector` snapshot as a `Reset` op, then forwarding each `VectorDiff` batch verbatim as `TimelineOp`s inside `TimelineBatch`es. The frontend mirrors ops into an ordered zustand array (never re-ordering) and renders text messages as bubbles (incoming muted / outgoing primary, 14 px radius, consecutive same-sender grouped under one avatar), with the text column capped at 720 px and centered.

## Boundaries & Constraints

**Always:**
- All Matrix/timeline/crypto/persistence logic stays in `keeper-core`; the `keeper` shell stays IPC/platform glue only and gains no business logic. `keeper-core` gains no `tauri` dependency. (AD-6)
- **Ordering is authoritative from Rust (AD-8/AD-9/AD-20).** keeper forwards the SDK `Timeline`'s `VectorDiff` sequence one-to-one as index-based `TimelineOp`s; the TS store applies them to a plain array and **never sorts, re-sorts, filters, or re-indexes**. Each SDK `TimelineItem` maps to exactly one `TimelineItemVm` so indices stay aligned (virtual/date-divider/state items become an `Other` VM, not dropped).
- **Snapshot-then-diff, re-subscribe safe (AD-8).** `Timeline::subscribe()` returns the current `Vector` plus a diff stream; the producer emits that Vector as one `Reset` op first, then forwards diff batches. (Re)subscribing at any time yields a fresh `Reset` the store applies by *replacing* contents — never duplicating, even across a StrictMode remount or room re-open.
- **Reuse the account's live session; lazy-activate if needed.** `subscribe_timeline` gets the existing `AccountHandle` (Client + SyncService from Story 1.4) under the manager lock, activating it idempotently via the same `activate` path if not already live. It never builds a second `Client`/`SyncService`. `client.get_room(room_id)` supplies the room.
- **Supervised, leak-free teardown (AD-19).** The timeline producer task registers its `AbortHandle` in the same per-account subscription map used by the room list, under a fresh subscription id. `timeline_unsubscribe(account_id, id)` aborts exactly that task and drops its `Timeline` (matrix-sdk-ui's drop handle cancels the SDK's background timeline tasks) — leaving the room-list stream and any other timeline untouched. The frontend unsubscribes on room change and on unmount so switching chats and StrictMode double-mount never leak tasks or stack streams.
- **Mirror the room-list producer's hardened lifecycle.** `BatchSink` returns `bool` (`channel.send(..).is_ok()`); the producer breaks when the channel closes; a naturally-completed task removes its own subscription entry; registration aborts + errors if the handle vanished in the spawn→register gap.
- **Cached-first render (NFR-4).** Reopening a previously-synced room renders from the `Timeline`'s cached `Vector` snapshot with no network round-trip (SDK-provided); the frontend adds no artificial delay. React keys every bubble by the item's `unique_id` so a new remote message mounts only the new bubble, never re-rendering the list.
- **Secret containment (NFR-9).** `TimelineItemVm`/`TimelineBatch` carry only non-secret render data (a stable opaque `key`, sender user id, resolved display name, decoded **text** body of already-decrypted events, timestamp ms, `isOwn`). No tokens, session material, event raw JSON, or crypto state cross IPC. `tracing` logs carry no message bodies, tokens, or session data — room id (opaque) only.
- **Text VM.** `TimelineItemVm` is an internally-tagged enum: `Message { key, sender, senderDisplayName: string | null, body, timestamp, isOwn }` for `m.room.message` of msgtype text/notice/emote; `Other { key }` for everything else (non-text msgtypes, state/membership/profile, redacted, unable-to-decrypt, and virtual items). `timestamp` is `i64` ms since Unix epoch (never ISO). Body defensively truncated (≤ 4096 chars) before crossing IPC.
- **Error taxonomy (AD-21).** Add `keeper-core` `TimelineError { RoomNotFound, Build(String) }` rolling into `CoreError::Timeline`; map through the single shell `to_ipc_error` funnel to a new `IpcErrorCode::TimelineUnavailable` (`retriable: true`). A failed timeline subscribe surfaces an honest inline state in the conversation pane, not a silent spinner.
- TS: no `any`, `import type`, `@/` alias, 2-space/100-col/double-quote Biome, `cn()` for classes, reuse installed shadcn primitives (`Avatar`, `ScrollArea`, `Skeleton`) — never hand-write in `src/components/ui/`. Rust: no `.unwrap()`/bare `.expect()` in production paths, `?` + `thiserror`, clippy `-D warnings` clean, `tracing` not `println!`.
- Regenerate ts-rs bindings for the new VMs (`TimelineItemVm`, `TimelineOp`, `TimelineBatch`) and updated `IpcErrorCode` into `src/lib/ipc/gen/` and commit them to match `cargo` output.

**Block If:**
- matrix-sdk-ui 0.18 does not expose `RoomExt::timeline()`/`room.timeline().await` and `Timeline::subscribe()` returning `(Vector<Arc<TimelineItem>>, Stream<Item = Vec<VectorDiff<Arc<TimelineItem>>>>)` — a stack-anchor conflict with AD-4. (Verified present in the vendored 0.18.0 source during planning; only block if implementation proves otherwise.)

**Never:**
- No send / composer / local echo / outbox / send-state captions — that is Story 1.6. The conversation pane is read-only here; the composer stays absent or disabled.
- No media/thumbnails, images, files, replies, edits render, reactions, receipts, typing, redaction UI, or history/back-pagination (Epic 3 / Story 3.9). Non-text and non-message items render as nothing (skipped) — they are carried as `Other` VMs only to keep diff indices aligned. Do not render date dividers or read markers.
- No holding timeline state in a JS store as source of truth or re-deriving order in TS (AD-9/AD-20). No `matrix-js-sdk` or any Matrix JS lib; no crypto/token/decode logic in TS.
- No multi-room timeline cache in TS — one open room at a time; switching rooms tears down and re-subscribes. No changes to Story 1.4's room-list flow, the login flow, or at-rest DB encryption (`sqlite_store(dir, None)` unchanged).
- No scroll-anchored auto-follow tuning, jump-to-bottom button, or unread dividers (later polish); a plain bottom-anchored scroll region is sufficient.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Open a synced room | user clicks a chat row; account live | `selectedRoomId` set; pane subscribes; first batch is a `Reset` snapshot of the room's cached `Timeline` Vector (text bubbles render, others skipped), then diff batches as sync delivers | none |
| Empty room | room with no renderable messages | `Reset` with items that are all `Other` (or empty); pane shows "No messages yet." | none |
| Live incoming message | open room receives a new remote event | a diff batch (`PushBack`/`Insert`/`Set`) appends the bubble within ~2 s; TS applies the op; only the new bubble mounts; it does not re-order or re-render the list | none |
| Reopen previously-synced room | switch away then back within the session | cached `Reset` snapshot renders immediately (< 150 ms target, no network round-trip); prior subscription was torn down | none |
| Consecutive same-sender | ≥ 2 adjacent text messages from one sender | grouped: single avatar + name on the first, subsequent bubbles hide avatar/name; a different sender (or an interleaved `Other`) breaks the group | none |
| Incoming vs outgoing | `isOwn == false` / `true` | incoming bubble uses muted surface (left); outgoing uses primary (right); both 14 px radius; text column ≤ 720 px centered in wide panes | none |
| Non-text / state / redacted / UTD | image/file msg, membership, redaction, undecryptable, virtual date divider | mapped to `Other { key }`; not rendered as a bubble; indices stay aligned so surrounding text renders correctly | none |
| Switch rooms | select a different row while one is open | old timeline unsubscribed + `Timeline` dropped (no leaked SDK tasks); store cleared; new room subscribes with a fresh `Reset`; room-list stream unaffected | none |
| Room not found | `client.get_room(room_id)` returns `None` | subscribe fails; inline "Couldn't open this conversation." state | `TimelineError::RoomNotFound` → `TimelineUnavailable` (`retriable:true`); logged, no secrets |
| Timeline build fails | `room.timeline().await` errors | subscribe fails with the same inline state; no partial subscription retained | `TimelineError::Build` → `TimelineUnavailable`; logged with cause, no secrets |
| Unsubscribe | pane unmounts / room deselected | `timeline_unsubscribe` aborts exactly that task; other account streams untouched | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- add `TimelineItemVm` (internally-tagged `#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]` enum: `Message { key, sender, senderDisplayName: Option<String>, body, #[ts(type="number")] timestamp: i64, isOwn: bool }`, `Other { key }`); add `TimelineOp` mirroring the used `VectorDiff` variants over `TimelineItemVm` (`Reset{items}`, `Append{items}`, `Clear`, `PushFront{item}`, `PushBack{item}`, `PopFront`, `PopBack`, `Insert{index,item}`, `Set{index,item}`, `Remove{index}`, `Truncate{length}` — indices `#[ts(type="number")] u32`); add `TimelineBatch { ops: Vec<TimelineOp> }`; add `IpcErrorCode::TimelineUnavailable`; serde round-trip tests for each new type.
- `src-tauri/crates/keeper-core/src/error.rs` -- add `TimelineError { RoomNotFound, Build(String) }`; add `CoreError::Timeline(#[from] TimelineError)`; secret-free messages.
- `src-tauri/crates/keeper-core/src/timeline.rs` -- NEW. Pure seams: `fn text_body(msgtype: &MessageType) -> Option<String>` (text/notice/emote → body; else `None`; unit-tested via ruma constructors); `fn item_to_vm(item: &TimelineItem) -> TimelineItemVm` (Event+`MsgLike::Message` with text → `Message` using `unique_id().0`, `sender()`, `sender_profile()` `Ready(Profile)` display name fallback to sender id, `body()`, `timestamp()` ms, `is_own()`; everything else → `Other{key}`); pure `fn timeline_diff_to_op(diff: VectorDiff<TimelineItemVm>) -> TimelineOp` (unit-tested across all variants); `async fn run_timeline_producer(client: Client, room_id: OwnedRoomId, sink: BatchSink)` (get_room→`TimelineError::RoomNotFound`; `room.timeline().await`→`Build`; `subscribe()`; emit `Reset` from initial Vector; `tokio::pin!` the stream; loop `diffs → diff.map(|i| item_to_vm(&i)) → timeline_diff_to_op → TimelineBatch`, `sink(..)` breaking on `false`; keep `Timeline` alive across the loop; remove own subscription entry on natural completion). Reuse `BatchSink` from `account`/`vm`.
- `src-tauri/crates/keeper-core/src/account.rs` -- `AccountManager::subscribe_timeline(&self, platform, account_id, room_id: &str, sink: TimelineSink) -> Result<u64, CoreError>`: get-or-`activate` the handle under the lock, parse `room_id` to `OwnedRoomId` (invalid → `TimelineError::RoomNotFound`), spawn `timeline::run_timeline_producer(handle.client.clone(), room_id, sink)`, register the `AbortHandle` in the shared `subscriptions` map (same registration-gap guard as room list); add `unsubscribe_timeline(&self, account_id, subscription_id)` delegating to the shared abort helper (extract the room-list abort body into a private `abort_subscription` reused by both). Define `pub type TimelineSink = Box<dyn Fn(TimelineBatch) -> bool + Send + Sync>`.
- `src-tauri/crates/keeper-core/src/lib.rs` -- `pub mod timeline;`.
- `src-tauri/crates/keeper/src/ipc.rs` -- add `#[tauri::command] async fn timeline_subscribe(state, account_id, room_id, channel: Channel<TimelineBatch>) -> Result<u64, IpcError>` (sink = `move |b| channel.send(b).is_ok()`) and `timeline_unsubscribe(state, account_id, subscription_id) -> Result<(), IpcError>`; extend `to_ipc_error` for `CoreError::Timeline` → `TimelineUnavailable` (`retriable:true`) + mapping unit test.
- `src-tauri/crates/keeper/src/lib.rs` -- register both commands in `generate_handler!`.
- `src/lib/ipc/gen/` -- regenerated: NEW `TimelineItemVm.ts`, `TimelineOp.ts`, `TimelineBatch.ts`; updated `IpcErrorCode.ts`.
- `src/lib/ipc/client.ts` -- add `subscribeTimeline(accountId, roomId, onBatch): Promise<number>` and `unsubscribeTimeline(accountId, id): Promise<void>`; re-export `TimelineItemVm`, `TimelineOp`, `TimelineBatch`.
- `src/lib/stores/vector-diff.ts` -- NEW generic `applyDiffOp<T>(arr: T[], op): T[]` reducer (range-guarded, never sorts) extracted so both timeline and room-list stores share one tested implementation; refactor `rooms.ts`'s `applyOp` to delegate (its existing tests guard the behavior).
- `src/lib/stores/timeline.ts` -- NEW vanilla zustand store `{ items: TimelineItemVm[], applyBatch(batch), clear() }`; `applyBatch` folds ops via `applyDiffOp`, `Reset`/`Clear` replace/empty, never sorts; `useTimelineStore` selector hook.
- `src/lib/stores/rooms.ts` -- add ephemeral UI state `selectedRoomId: string | null`, `selectRoom(roomId | null)` (allowed ephemeral UI state; mirror invariant unchanged).
- `src/lib/format-time.ts` -- add `formatMessageTime(ms): string` (HH:MM, invalid/≤0 → `""`).
- `src/components/chat/message-bubble.tsx` -- NEW. Text bubble: props `{ item: Message-variant vm, grouped: boolean }`; muted (incoming) / primary (outgoing) by `isOwn`, `rounded-[14px]`, avatar+name shown only when `!grouped`, subtle `formatMessageTime` timestamp; `cn()` classes; accessible.
- `src/components/layout/conversation-pane.tsx` -- replace placeholder: read `selectedRoomId` + `currentAccount`, subscribe on selection, render a bottom-anchored `ScrollArea` with a 720 px-max centered column of grouped `MessageBubble`s (compute grouping over consecutive `Message` items, skip `Other`), loading `Skeleton` / empty "No messages yet." / inline `TimelineUnavailable` error states; unsubscribe + `clear()` on room change / unmount (newest-mount-wins, `cancelled`-gated `onBatch`).
- `src/components/chat/chat-row.tsx` -- add a selected state (highlight + `aria-current`) driven by a `selected` prop.
- `src/components/layout/chat-list-pane.tsx` -- wire `onSelect={selectRoom}` and pass `selected={room.roomId === selectedRoomId}` per row.
- Tests: `keeper-core` unit (`vm.rs` serde round-trip for the three new types; `timeline.rs` `text_body` across text/notice/emote/image/none and `timeline_diff_to_op` covering every op variant; `error.rs`/`ipc.rs` `TimelineError` → `TimelineUnavailable`+`retriable:true`); frontend (`vector-diff.test.ts` each op incl. reset-replaces + range guards; `timeline.test.ts`; `format-time.test.ts` `formatMessageTime`; `message-bubble.test.tsx` incoming/outgoing/grouped; `conversation-pane.test.tsx` subscribe/render/switch/unsubscribe + states; updated `chat-row.test.tsx`/`chat-list-pane.test.tsx` for selection).

## Tasks & Acceptance

**Execution:**
- [x] `keeper-core/src/vm.rs` -- add `TimelineItemVm`, `TimelineOp`, `TimelineBatch`; `IpcErrorCode::TimelineUnavailable`; serde round-trip tests.
- [x] `keeper-core/src/error.rs` -- add `TimelineError` + `CoreError::Timeline`; secret-free messages.
- [x] `keeper-core/src/timeline.rs` -- NEW: `text_body`, `item_to_vm`, pure `timeline_diff_to_op`, `run_timeline_producer` (get_room/build/subscribe, Reset-then-diffs, hardened sink/teardown, keeps `Timeline` alive); unit-test `text_body` + `timeline_diff_to_op`; `tracing` outcome by room id, no secrets.
- [x] `keeper-core/src/account.rs` -- `subscribe_timeline`/`unsubscribe_timeline` reusing the live handle + shared subscription map + shared `abort_subscription`; `TimelineSink` type.
- [x] `keeper-core/src/lib.rs` -- `pub mod timeline`.
- [x] `keeper/src/ipc.rs` -- `timeline_subscribe`/`timeline_unsubscribe` commands forwarding batches to the `Channel`; `to_ipc_error` for `CoreError::Timeline` → `TimelineUnavailable`(`retriable:true`) + mapping test.
- [x] `keeper/src/lib.rs` -- register both commands.
- [x] regenerate ts-rs bindings and commit `src/lib/ipc/gen/{TimelineItemVm,TimelineOp,TimelineBatch}.ts` + updated `IpcErrorCode.ts`.
- [x] `src/lib/stores/vector-diff.ts` (+ `vector-diff.test.ts`) -- generic range-guarded reducer; refactor `rooms.ts` to delegate.
- [x] `src/lib/stores/timeline.ts` (+ `timeline.test.ts`) -- ordered mirror store; reset replaces without duplication.
- [x] `src/lib/stores/rooms.ts` -- add `selectedRoomId`/`selectRoom`.
- [x] `src/lib/ipc/client.ts` -- `subscribeTimeline`/`unsubscribeTimeline` wrappers + re-exports.
- [x] `src/lib/format-time.ts` (+ test) -- `formatMessageTime`.
- [x] `src/components/chat/message-bubble.tsx` (+ test) -- grouped text bubble, incoming/outgoing styling.
- [x] `src/components/layout/conversation-pane.tsx` (+ test) -- subscribe/render/switch/unsubscribe lifecycle, grouping, loading/empty/error states, 720 px centered column.
- [x] `src/components/chat/chat-row.tsx` + `src/components/layout/chat-list-pane.tsx` (+ updated tests) -- selection wiring & highlight.

**Acceptance Criteria:**
- Given a selected Chat, when the conversation pane opens, then a per-room timeline channel streams `TimelineItemVm` items as a `Reset` snapshot followed by diff batches from the SDK `Timeline`, and text messages render as bubbles — incoming muted, outgoing primary, 14 px radius, consecutive same-sender grouped under a single avatar — with the text column capped at 720 px and centered in wider panes (FR-8/FR-9, AD-4/AD-8, UX-DR5).
- Given a Chat previously synced, when it is reopened in the same session, then the cached timeline renders without a network round-trip (targeting the < 150 ms switch bar, NFR-4), and closing/switching a Chat tears down its subscription and `Timeline` without leaking the account's other streams (AD-19).
- Given live activity, when a new remote message arrives in the open Chat, then it appears via a diff batch and only the new bubble mounts (keyed by the item's `unique_id`) without re-rendering or re-ordering the list; ordering is authoritative from the Rust SDK `Timeline` and the TS store never sorts (AD-8/AD-9/AD-20).
- Given timeline activation fails (room not found / build error), then `timeline_subscribe` returns `TimelineUnavailable` and the pane shows an honest inline error rather than a silent spinner, and no partial subscription is retained (AD-21).
- Given code review, then no token, session, event raw JSON, or message plaintext beyond the rendered text body appears on any VM/IPC response or in `tracing` logs, and `keeper-core` carries no `tauri` dependency (NFR-9, AD-6).
- Given the quality gates, when `bun run check`, `bun run check:rust`, `bun run test:rust`, and `cargo deny check` (from `src-tauri/`) run, then all pass.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 1, low 0)
- defer: 1
- reject: 9
- addressed_findings:
  - `[medium]` `[patch]` The conversation pane's message `<ol>` was rendered inside a Radix `ScrollArea` with only `flex flex-col` and no bottom anchoring, so a room's history aligned to the TOP of the pane — short rooms sat with empty space below and long rooms opened scrolled to the oldest message — directly contradicting the spec's thrice-stated "bottom-anchored scroll region" requirement (and inconsistent with the loading skeleton, which already used `justify-end`). Replaced the `ScrollArea` with a plain `overflow-y-auto` flex-column region (the spec explicitly sanctions "a plain bottom-anchored scroll region is sufficient") whose content uses `mt-auto` to rest at the bottom, plus a minimal always-scroll-to-bottom effect keyed on the streamed `items` so the newest message stays in view (no auto-follow tuning / jump-to-bottom — Epic 3 polish). `conversation-pane.tsx`.
  - Deferred (1): the shared `subscribe()` IPC helper (`client.ts`, pre-existing from Story 1.4) never clears `channel.onmessage` when `invoke` rejects, leaking a handler per failed subscribe — appended to `deferred-work.md`.
  - Rejected (9, unreachable / by-design / spec-sanctioned / cosmetic): `unsubscribe_timeline`/`unsubscribe_room_list` sharing one id-keyed subscription map (ids are globally unique via a single atomic — no real collision); `one()` using `??` could drop a falsy single payload (cannot fire — VMs are always objects); `toRenderedMessages` unmemoized (perf micro-opt; virtualization is out of scope and keys prevent remounts); an interleaved `Other` (e.g. a reaction) breaking a same-sender group (spec-sanctioned in the I/O matrix); non-text/UTD/redacted collapsing to `Other{key}` with no reason discriminant (explicitly out of scope — "non-text items render as nothing"); `truncate_body`'s doc claiming "grapheme" where it caps by `char` (cosmetic wording, only at the exact 4096-char boundary); the first-activation concurrent-sibling teardown race in `subscribe_timeline` (narrow, retriable, unreachable in the single-room-at-a-time UI where the room list activates first); and `forward_timeline` sinking an empty `TimelineBatch` for an empty diff `Vec` (harmless no-op churn, matches the room-list producer's existing pattern).
- addressed_findings:
  - `[medium]` `[patch]` Timeline build errors (`RoomNotFound`/`Build`) were buried inside the spawned producer task, so `subscribe_timeline` always returned `Ok` → the pane hung on a silent loading skeleton (violating AC-4) and a just-activated account's `SyncService` leaked with zero subscribers (violating the AD-21 "no partial live account retained" AC). Split the producer into a synchronous `open_timeline` (builds the `Timeline` + subscribes, surfacing `TimelineUnavailable`) and a background `forward_timeline` loop; `subscribe_timeline` now mirrors the room-list `did_activate` teardown on build failure. This also removes the reaper-vs-registration race (a fast-failing producer leaking a dead map entry). `account.rs`, `timeline.rs`.
  - `[medium]` `[patch]` A finite `origin_server_ts` beyond the JS `Date` range (`8.64e15 < ms ≤` ruma `UInt` max ~`9.007e15`) passed `formatMessageTime`'s guard, then `Intl.DateTimeFormat.format`/`toISOString` threw `RangeError`, crashing the whole conversation-pane render from untrusted remote input. Clamped both `formatMessageTime` and `formatRoomTimestamp` to `MAX_DATE_MS`; added a boundary test. `format-time.ts`.
  - `[medium]` `[patch]` The conversation-pane effect early-returned on `accountId`/`selectedRoomId` null without clearing the timeline store, so a previous room's/account's rendered messages could linger (stale plaintext across account loss). It now clears the store and resets load/error state in that branch; corrected the misleading `rooms.ts` comment that claimed selection was cleared elsewhere. `conversation-pane.tsx`, `rooms.ts`.
  - `[low]` `[patch]` The shared `applyDiffOp` refactor had dropped the room-list reducer's compile-time exhaustiveness guard (a future op variant would silently no-op, risking AD-8/20 desync). Reworked it to a `T`-only generic over the canonical `DiffOp<T>` union with a `never` default check (also removing the `as unknown` casts). `vector-diff.ts`, `rooms.ts`, `timeline.ts`.
  - `[low]` `[patch]` `applyDiffOp`'s `truncate` lacked the range guard its siblings had; a negative length would `slice` from the end and corrupt the mirror. Guarded it; added negative/zero/missing-payload tests. `vector-diff.ts`, `vector-diff.test.ts`.
  - `[low]` `[patch]` A degenerate empty/whitespace-only text message became a `Message` VM (empty bubble, suppressed "No messages yet."). `text_body` now returns `None` for it → rendered as `Other` (skipped); added a test. `timeline.rs`.
  - `[low]` `[patch]` `truncate_body` ran an O(n) `chars().count()` on every message on the snapshot hot path; added a byte-length fast path. `timeline.rs`.
  - Incidental to patch 1: added `#![recursion_limit = "256"]` to the keeper shell crate (the synchronous timeline build deepened the `timeline_subscribe` async-fn layout past rustc's default).
  - Rejected (8, spec-sanctioned / by-design / impossible): the "only the new bubble mounts" AC being slightly overstated for mid-list `Remove`/`Insert` (wording nuance, not a bug); the display-name fallback living in TS (`senderDisplayName ?? sender`) rather than Rust (behaviorally identical); no `item_to_vm` unit test (SDK `EventTimelineItem` has no public test constructor — the spec's live-only residual, same posture as 1.4's decode path; the pure seams `text_body`/`timeline_diff_to_op` are tested); StrictMode/rapid-reselect briefly running two producers for one room (the resolve-then-unsubscribe branch tears the extra down — transient by design); `applyDiffOp` silently dropping out-of-range `set`/`insert`/`remove` (the deliberate 1.4 crash-avoidance guard; a correct SDK stream never emits them); no loading-timeout distinguishing "loading" from "stuck" (no such mechanism specced; out of scope); grouping keying on `sender` not `isOwn` (impossible to diverge — `isOwn` is a pure function of sender identity); and `usize→u32` index truncation at >4.29B items (near-impossible, matches 1.4's reject).

## Design Notes

**Grounded matrix-sdk-ui 0.18.0 API (verified against the vendored source at `~/.cargo/registry/src/*/matrix-sdk-ui-0.18.0/`):**
```rust
use matrix_sdk_ui::timeline::{RoomExt, TimelineItem, TimelineItemContent, MsgLikeKind};
use matrix_sdk::ruma::events::room::message::MessageType;

let room = client.get_room(&room_id).ok_or(TimelineError::RoomNotFound)?;
let timeline = room.timeline().await.map_err(|e| TimelineError::Build(e.to_string()))?;
let (initial, stream) = timeline.subscribe().await;   // Vector<Arc<TimelineItem>>, Stream<Vec<VectorDiff<Arc<TimelineItem>>>>
sink(TimelineBatch { ops: vec![TimelineOp::Reset { items: initial.iter().map(|i| item_to_vm(i)).collect() }] });
tokio::pin!(stream);
while let Some(diffs) = stream.next().await {
    let ops = diffs.into_iter()
        .map(|d| timeline_diff_to_op(d.map(|i| item_to_vm(&i))))  // VectorDiff::map is sync — item_to_vm has no await
        .collect();
    if !sink(TimelineBatch { ops }) { break; }
}
// `timeline` must stay in scope for the whole loop: its drop handle cancels the SDK's background tasks.
```
`item_to_vm` (all sync): match `item.kind()` → `Event(ev)`; `ev.content()` → `TimelineItemContent::MsgLike(m)`; `m.kind` → `MsgLikeKind::Message(msg)`; `text_body(msg.msgtype())` → `Some(body)` gives `Message { key: item.unique_id().0.clone(), sender: ev.sender().to_string(), sender_display_name: match ev.sender_profile() { Ready(p) => p.display_name.clone(), _ => None }, body, timestamp: ev.timestamp().0.into(), is_own: ev.is_own() }`. Every other content kind, non-text msgtype, virtual item, redacted, or UTD → `Other { key: item.unique_id().0.clone() }`. `text_body`: `MessageType::Text|Notice|Emote` → that body string; else `None`.

**Why 1:1 (no Rust-side filtering).** Filtering `Other` items out of a `VectorDiff` stream would require re-mapping every index — the exact fragile path Story 1.4 avoided. Forwarding one VM per SDK item keeps `timeline_diff_to_op` a pure, index-preserving 1:1 seam (the unit-tested boundary), and the frontend simply renders only `Message` items. Grouping is a pure render-time pass over the rendered subsequence: a `Message` is `grouped` when the immediately-preceding **rendered** message has the same `sender`.

**Supervision & teardown.** `subscribe_timeline` reuses the Story-1.4 `AccountHandle` (its `Client` + `SyncService`) and the shared `subscriptions: Arc<Mutex<HashMap<u64, JoinHandle>>>` + `NEXT_SUBSCRIPTION_ID`. Extract the room-list abort/remove logic into one `abort_subscription(account_id, id)` used by both `unsubscribe_room_list` and `unsubscribe_timeline`. Apply the room-list producer's hardened patterns verbatim (bool `BatchSink`, break on closed channel, self-remove on natural completion, abort-if-handle-vanished on registration). Switching rooms in the UI aborts the old subscription and drops its `Timeline` (SDK drop handle cancels timeline tasks) — the room-list stream and any sibling subscription are untouched, satisfying "no leaked streams."

**Frontend (AD-9).** `timelineStore` holds only the streamed `TimelineItemVm[]` (single open room). `conversation-pane` effect: on `selectedRoomId` change, `clear()` at effect start (newest-mount-wins), `subscribeTimeline`, gate `onBatch` with a `cancelled` flag, and return a cleanup that `unsubscribeTimeline` + `clear()`s — StrictMode-safe. `Reset` replaces contents, so re-subscribe never duplicates. Bubbles key on `item.key` (`unique_id`) so a `PushBack` mounts one node.

**Residual (documented, not a gap):** the live path (`room.timeline()`, `subscribe()`, real `VectorDiff` sequence, `item_to_vm` over real events, "< 150 ms cached", "appears within ~2 s") is exercised only against a real Synapse ≥ 1.114 — the epic exit gate. Unit tests cover the pure seams (`text_body`, `timeline_diff_to_op`, the TS reducer, serde round-trips, error mapping). Send/composer, media, replies/edits/reactions, receipts, and history pagination are out of scope (Stories 1.6 / Epic 3).

## Verification

**Commands:**
- `bun run check` -- expected: biome + tsc strict + vitest (new `vector-diff`/`timeline`/`message-bubble`/`conversation-pane`/`format-time` tests, updated `chat-row`/`chat-list-pane`) green.
- `bun run check:rust` -- expected: rustfmt + clippy `-D warnings` clean (new `timeline` module, no `.unwrap()`); core stays tauri-free (workspace guard).
- `bun run test:rust` -- expected: cargo-nextest green; ts-rs bindings regenerate to match committed `src/lib/ipc/gen/` (only the intended new `TimelineItemVm`/`TimelineOp`/`TimelineBatch` + changed `IpcErrorCode`).
- `cd src-tauri && cargo deny check` -- expected: license firewall passes (no new crates — matrix-sdk-ui already a dependency).

**Manual checks (require a real Synapse ≥ 1.114 — automated tests can't exercise live sync):**
- `op run --env-file=.env.1p -- bun run tauri dev`: sign in → click a chat → its history renders as bubbles (incoming muted / outgoing primary, grouped same-sender), text column centered ≤ 720 px. Send a message from another client → it appears in the open chat within ~2 s as a new bubble with no full-list flicker.
- Switch to another chat and back → cached history renders instantly with no duplicate bubbles; no leaked sync/timeline tasks. Open a room with no messages → "No messages yet."

## Auto Run Result

Status: **done**

### Summary
Implemented the per-open-room live timeline across both layers. `keeper-core` gained a `timeline` module: pure, unit-tested seams (`text_body` for text/notice/emote — empty bodies skipped; `timeline_diff_to_op` 1:1 over all eyeball-im `VectorDiff` variants; `item_to_vm` mapping each SDK `TimelineItem` to exactly one `TimelineItemVm` so diff indices stay aligned), a synchronous `open_timeline` (builds the matrix-sdk-ui `Timeline` via `RoomExt` + `subscribe()`), and a background `forward_timeline` loop that emits the cached snapshot as a `Reset` then forwards each diff batch verbatim over a `tauri::ipc::Channel` — nothing sorted in TypeScript (AD-8/9/20). `AccountManager::subscribe_timeline` reuses (or idempotently activates) the Story-1.4 single-account handle, builds the timeline synchronously so `RoomNotFound`/`Build` surface as `IpcErrorCode::TimelineUnavailable` (an honest inline error, not a silent spinner), tears down a just-activated partial account on failure (AD-21), and registers the forwarding task in the shared subscription map (`unsubscribe_timeline` shares `abort_subscription` with the room list). The frontend mirrors ops into an ordered vanilla-zustand store via a shared, exhaustiveness-guarded `applyDiffOp` reducer (never sorts; `Reset` replaces), renders text `MessageBubble`s (incoming muted / outgoing primary, 14 px radius, consecutive same-sender grouped under one avatar) in a 720 px-max centered column, keyed by the item's `unique_id`, and manages subscribe/switch/unsubscribe over the effect lifecycle (StrictMode-safe, clears on account/room change). No token/session/event-raw-JSON/message-plaintext-beyond-text crosses IPC or reaches a log.

### Files changed
- `crates/keeper-core/src/vm.rs` — `TimelineItemVm` (tagged `message`/`other`), `TimelineOp` (11 variants), `TimelineBatch`, `IpcErrorCode::TimelineUnavailable`; serde round-trip tests.
- `crates/keeper-core/src/error.rs` — `TimelineError { RoomNotFound, Build }` + `CoreError::Timeline`.
- `crates/keeper-core/src/timeline.rs` (NEW) — `text_body`, `truncate_body`, `item_to_vm`, pure `timeline_diff_to_op`, synchronous `open_timeline`, background `forward_timeline`; unit tests for the pure seams.
- `crates/keeper-core/src/account.rs` — `TimelineSink`; `subscribe_timeline` (sync build + `did_activate` teardown + registration-gap guard) / `unsubscribe_timeline`; shared `abort_subscription`.
- `crates/keeper-core/src/lib.rs` — `pub mod timeline`.
- `crates/keeper/src/ipc.rs` — `timeline_subscribe`/`timeline_unsubscribe` commands; `to_ipc_error` `CoreError::Timeline` → `TimelineUnavailable`(retriable) + mapping tests.
- `crates/keeper/src/lib.rs` — command registration; `#![recursion_limit = "256"]`.
- `src/lib/ipc/gen/{TimelineItemVm,TimelineOp,TimelineBatch}.ts` (NEW) + `IpcErrorCode.ts` — regenerated bindings.
- `src/lib/ipc/client.ts` — `subscribeTimeline`/`unsubscribeTimeline` wrappers + re-exports.
- `src/lib/stores/vector-diff.ts` (NEW) + test — shared `T`-generic, range- and exhaustiveness-guarded reducer.
- `src/lib/stores/timeline.ts` (NEW) + test — ordered timeline mirror store.
- `src/lib/stores/rooms.ts` (+ test) — `selectedRoomId`/`selectRoom`; delegates to the shared reducer.
- `src/lib/format-time.ts` (+ test) — `formatMessageTime`; both formatters clamped to the JS `Date` range.
- `src/components/chat/message-bubble.tsx` (NEW) + test — grouped text bubble.
- `src/components/layout/conversation-pane.tsx` (+ test) — subscribe/switch/unsubscribe lifecycle, grouping, loading/empty/error states, 720 px column, clears on account/room change.
- `src/components/chat/chat-row.tsx` + `src/components/layout/chat-list-pane.tsx` (+ tests) — selection wiring & highlight.

### Review findings
- Two reviewers (adversarial-general Blind Hunter + edge-case-hunter), fresh context. Triage: **0 intent_gap, 0 bad_spec, 7 patch (3 medium, 4 low), 0 defer, 8 reject**. See Review Triage Log.
- **Patches (all applied):** made timeline build errors surface synchronously as `TimelineUnavailable` (was a silent spinner + leaked `SyncService`); fixed a conversation-pane render crash on an out-of-range remote timestamp; cleared stale rendered messages on account loss; restored the shared reducer's compile-time exhaustiveness guard; guarded negative `truncate`; skipped empty-body messages; fast-pathed `truncate_body`.

### Verification
- `bun run check` ✅ — biome clean, tsc strict clean, vitest **114 passed (14 files)**, core-tauri-free guard passes.
- `bun run check:rust` ✅ — rustfmt `--check` + clippy `--all-targets -D warnings` clean.
- `bun run test:rust` ✅ — cargo-nextest **80 passed, 0 skipped**; ts-rs bindings regenerate idempotently (only `TimelineItemVm`/`TimelineOp`/`TimelineBatch` new + `IpcErrorCode` changed).
- `cd src-tauri && cargo deny check licenses bans sources` ✅ (`bans ok, licenses ok, sources ok`). No new crate (Cargo.lock/Cargo.toml unchanged); the pre-existing gtk-rs `advisories` failure and OpenSSL unmatched-allowance warning (stories 1.1–1.4) are unchanged and out of scope.
- Not run: live sync against a real Synapse ≥ 1.114 (blocking) — the whole build/subscribe/diff/`item_to_vm` path is reasoned-about and unit-tested only at its pure seams (`text_body`, `timeline_diff_to_op`, the TS reducer, VM serde, error mapping). This is the epic exit gate. See Manual checks.

### Residual risks
- The live path (`room.timeline()`, `subscribe()`, real `VectorDiff` sequence, `item_to_vm` over real events incl. `is_own`, "< 150 ms cached", "appears within ~2 s") is exercised only against a real homeserver.
- `item_to_vm` has no unit test (the SDK `EventTimelineItem` exposes no public test constructor), so the highest-visibility mapping — including `is_own`-driven incoming/outgoing styling — is live-only; consistent with Story 1.4's decode-path residual.
- Sign-out still does not stop the account's `SyncService` or reset `selectedRoomId` (Story 1.8; already tracked in `deferred-work.md`). Single-account with the panes always mounted means no trigger in Epic 1.
- `followup_review_recommended: true` — the patch pass restructured the Rust timeline subscribe/teardown error path (behavioral), fixed a render crash and a cross-account state bleed, and reworked a shared reducer used by two stores; an independent follow-up review is worthwhile.

### Follow-up review pass (2026-07-04)

An independent follow-up review (fresh-context Blind Hunter + Edge Case Hunter over the full baseline→HEAD diff) surfaced one real, this-story defect and one pre-existing issue; the rest were unreachable, by-design, spec-sanctioned, or cosmetic.

- **Patch applied (1, medium):** the conversation pane's message `<ol>` was not bottom-anchored (`flex flex-col` inside a Radix `ScrollArea`, no `justify-end`/`mt-auto`), so history aligned to the top of the pane — short rooms left empty space below and long rooms opened at the oldest message, contradicting the spec's thrice-stated bottom-anchor requirement (and the loading skeleton, which already used `justify-end`). Replaced the `ScrollArea` with a plain `overflow-y-auto` flex-column region (spec: "a plain bottom-anchored scroll region is sufficient") whose content uses `mt-auto`, plus a minimal always-scroll-to-bottom effect keyed on the streamed `items`. `conversation-pane.tsx` only — no Rust or IPC change.
- **Deferred (1):** the shared `subscribe()` IPC helper (`client.ts`, pre-existing from Story 1.4) does not clear `channel.onmessage` when `invoke` rejects, leaking a handler per failed subscribe. Recorded in `deferred-work.md`.
- **Rejected (9):** shared id-keyed subscription map (ids globally unique), `one()` `??` falsy-drop (VMs always objects), unmemoized `toRenderedMessages` (perf micro-opt, out of scope), interleaved-`Other` ungrouping (spec-sanctioned), no UTD/redaction discriminant (out of scope), `truncate_body` "grapheme" doc wording (cosmetic boundary case), first-activation concurrent-sibling teardown race (narrow/retriable/unreachable in single-room UI), empty-diff `TimelineBatch` (harmless, matches room-list pattern).

**Verification (this pass):** `bun run check` ✅ — biome clean, tsc strict clean, vitest **114 passed (14 files)**, core-tauri-free guard passes. No Rust files changed this pass, so `check:rust` / `test:rust` / `cargo deny` results from the prior pass stand. Live sync against a real Synapse remains the epic exit gate (unchanged residual). `followup_review_recommended: false` — the sole change is one localized, non-behavioral layout fix confined to `conversation-pane.tsx`.
