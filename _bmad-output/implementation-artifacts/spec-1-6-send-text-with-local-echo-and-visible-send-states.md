---
title: 'Story 1.6 — Send Text with Local Echo and Visible Send States'
type: 'feature'
created: '2026-07-04'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '7d4f344445b6d565a2a73ecb53dba7389021d85c'
final_revision: '465758ed285588e92194e7291f45a6dbade0c316'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** After Story 1.5 the conversation pane streams and renders a room's live timeline, but it is read-only — there is no composer and no way to send. Epic 1's vertical slice is incomplete until a user can type a message that appears instantly as local echo and honestly reports whether it actually went out. This story also seeds the FR-41/AD-13 invariant that a single `send::submit` gate is the *only* path feeding the SDK `SendQueue`, which the later undo-send / approval epics build on.

**Approach:** Add a composer to the conversation pane (autogrowing `Textarea`; Enter sends, ⇧Enter newlines) that calls a new `send_text` IPC command. In `keeper-core`, a new `send` module exposes `submit(&Timeline, text, trigger)` — the sole function that feeds the SDK send queue (`timeline.send(RoomMessageEventContent::text(..))`). The message's local echo, and every subsequent send-state transition, arrive through the **existing** Story-1.5 per-room `Timeline::subscribe()` diff stream (no TypeScript-synthesized echo), so the frontend stays a pure renderer. `item_to_vm` gains a `sendState` read from `EventTimelineItem::send_state()`, surfaced as an optional field on the `Message` VM, and the frontend renders microcopy captions ("Sending…" → "Sent"; persistent destructive "Failed — Retry") under the message group. Retry re-drives the *existing* wedged local echo via its `SendHandle::unwedge()` (same `unique_id`, no duplicate), routed through a `send_retry` command.

## Boundaries & Constraints

**Always:**
- All send/queue/crypto/persistence logic stays in `keeper-core`; the `keeper` shell stays IPC/platform glue only. `keeper-core` gains no `tauri` dependency. (AD-6)
- **Single dispatch gate (AD-13/FR-41).** `keeper-core::send::submit(timeline, text, trigger: SendTrigger)` is the *only* function that feeds new content to the SDK send queue (the only call site of `Timeline::send(..)` / `send_queue().send(..)`). `SendTrigger` seeds `{ ComposerSend, ApprovalPaneApprove }`; only `ComposerSend` is used this story. A Rust test asserts `submit` is the sole content-dispatch entry point in the `send` module (e.g. an `include_str!` source scan asserting the SDK send call appears exactly once and inside `submit`). Retry via `unwedge()` re-drives an already-dispatched request and is **not** a new dispatch — it does not violate this invariant.
- **Local echo & state are authoritative from Rust (AD-8/AD-9/AD-20).** The composer only calls `send_text`; the sent message appears solely because the SDK `Timeline` emits a `PushBack` (then `Set`) over the Story-1.5 stream keeper already forwards. The TS store never invents, mutates, re-orders, or removes a timeline item — it applies the same diff ops as for received messages. No send/outbox store holds message state as source of truth.
- **Send-state VM.** Extend `TimelineItemVm::Message` with `sendState: Option<SendState>` where `SendState ∈ { Sending, Sent, Failed }` (serde/ts camelCase → `"sending"|"sent"|"failed"`). Map `EventSendState`: `NotSentYet → Sending`; `Sent → Sent`; `SendingFailed { is_recoverable: false } → Failed`; `SendingFailed { is_recoverable: true } → Sending` (transient, the queue is still retrying). A non-local-echo (remote/received) item has `send_state() == None → sendState: None`. No new secret crosses IPC (no txn id, event raw JSON, error object, or token) — only the enum tag.
- **Reuse the same live `Timeline` for subscribe + send + retry.** `unique_id`s are only stable within one `Timeline` instance, so send/retry MUST operate on the exact `Timeline` that produced the subscribed items. Store the room's `Arc<Timeline>` on the account's supervision state when `subscribe_timeline` opens it, look it up (by room id) for `send_text`/`send_retry`, and drop it wherever that subscription is torn down (unsubscribe + natural completion) so no `Timeline` (and its SDK tasks) leak. (AD-19)
- **Composer UX (UX-DR5).** `Textarea` autogrows to 8 lines then scrolls; **Enter sends, ⇧Enter inserts a newline**; a whitespace-only body never dispatches. The composer is present only when a room is open and its timeline loaded; it is absent/disabled otherwise. Input latency stays under one frame (no IPC round-trip on keystroke — local `useState` draft, cleared on successful submit and on room change).
- **Send-state captions (UX-DR10/UX-DR11).** Render as a `caption` under the message group, sentence case, no error codes, no emoji: `Sending…`, `Sent`, and a persistent destructive `Failed — Retry`. A `failed` caption always renders and its Retry never auto-clears; `sending`/`sent` render under the last bubble of a same-sender group. Retry re-enters the controlled send path (`send_retry`), never a raw SDK call.
- **Error taxonomy (AD-21).** Add `keeper-core` `SendError { RoomNotFound, NoOpenTimeline, EchoNotFound, Dispatch(String) }` rolling into `CoreError::Send`; map through the single `to_ipc_error` funnel to a new `IpcErrorCode::SendFailed` (`retriable: true`). Enqueue-time failures surface as this inline error; asynchronous delivery failure surfaces as the `Failed` send-state caption (not an IpcError). No message body, token, or txn id in `tracing` logs — room id (opaque) only.
- TS: no `any`, `import type`, `@/` alias, 2-space/100-col/double-quote Biome, `cn()` for classes, reuse installed shadcn `Textarea`/`Button` — never hand-write in `src/components/ui/`. Rust: no `.unwrap()`/bare `.expect()` in production paths, `?` + `thiserror`, clippy `-D warnings` clean, `tracing` not `println!`.
- Regenerate ts-rs bindings for the new/changed VMs (`SendState` new; `TimelineItemVm` Message gains `sendState`; `IpcErrorCode` gains `SendFailed`) into `src/lib/ipc/gen/` and commit them to match `cargo` output.

**Block If:**
- matrix-sdk-ui 0.18 does not expose `Timeline::send(AnyMessageLikeEventContent) -> Result<SendHandle, _>`, `EventTimelineItem::send_state()/local_echo_send_handle()`, or `SendHandle::unwedge()`, or `EventSendState` lacks the `NotSentYet/SendingFailed{is_recoverable}/Sent` shape — a stack-anchor conflict with AD-13. (Verified present in the vendored 0.18.0 source during planning; only block if implementation proves otherwise.)

**Never:**
- No offline queue pill, reconnect convergence, force-quit persistence of queued sends, or the amber "Queued — sends when you're back online" state — that is Story 1.7. (This story's failure state is the permanent "Failed — Retry"; do not implement offline/queued detection.)
- No edit, reply, redaction/undo-send, reactions, drafts persistence (Epic 7), read receipts, or typing indicators. No media/file send (Story 3.7). No approval pane (`ApprovalPaneApprove` is only seeded in the enum, unused).
- No changes to Story 1.5's receive path beyond adding the `sendState` field and its mapping, and no changes to the room-list/login flows. No second `Client`/`SyncService`/`Timeline` per room. No holding message state in a JS store as source of truth; no `matrix-js-sdk`; no crypto/token/txn logic in TS.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Send a message | open room, non-empty body, Enter | `send_text` dispatches via `submit`→`timeline.send`; local echo appears through the existing timeline stream as a new outgoing bubble with `Sending…`; composer clears; caption resolves to `Sent` | none on happy path |
| ⇧Enter | caret in composer | inserts a newline, does not send; `Textarea` autogrows to 8 lines then scrolls | none |
| Empty/whitespace send | body is empty or only whitespace, Enter | no dispatch, no echo, composer unchanged | none (silently ignored) |
| Permanent send failure | SDK reports `SendingFailed{is_recoverable:false}` | the echo's caption becomes persistent destructive `Failed — Retry`; it never auto-clears; bubble stays | delivered via `sendState: Failed` on a `Set` diff |
| Retry a failed send | user activates Retry on a failed echo | `send_retry(account,room,key)` → `local_echo_send_handle().unwedge()`; the SAME echo re-drives (no duplicate bubble); caption returns to `Sending…` then `Sent`/`Failed` | `SendError::EchoNotFound` → `SendFailed` if the echo is gone |
| Recoverable in-flight failure | `SendingFailed{is_recoverable:true}` | caption stays `Sending…` (queue auto-retries); no Retry button | none |
| Send with no open timeline | `send_text` for a room with no active timeline subscription | command returns `SendFailed`; no echo | `SendError::NoOpenTimeline` → `SendFailed`(retriable) |
| Room not found | `client.get_room` returns `None` at send time | command returns `SendFailed` | `SendError::RoomNotFound` → `SendFailed` |
| Switch room while sending | user changes room before `Sent` | old timeline unsubscribed + `Arc<Timeline>` dropped (no leak); the in-flight send continues in the SDK send queue; re-opening the room later shows the reconciled remote message | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- add `SendState` enum (`#[serde(rename_all="camelCase")]` unit enum `Sending|Sent|Failed`, `#[ts(export)]`); add `send_state: Option<SendState>` to `TimelineItemVm::Message`; add `IpcErrorCode::SendFailed`; serde round-trip tests for `SendState` and the extended `Message`.
- `src-tauri/crates/keeper-core/src/error.rs` -- add `SendError { RoomNotFound, NoOpenTimeline, EchoNotFound, Dispatch(String) }`; add `CoreError::Send(#[from] SendError)`; secret-free messages.
- `src-tauri/crates/keeper-core/src/send.rs` -- NEW. `pub enum SendTrigger { ComposerSend, ApprovalPaneApprove }`; `pub async fn submit(timeline: &Timeline, text: &str, trigger: SendTrigger) -> Result<(), SendError>` — the SOLE content-dispatch gate: build `RoomMessageEventContent::text(text)` → `AnyMessageLikeEventContent`, `timeline.send(content).await.map_err(|e| SendError::Dispatch(e.to_string()))?`, `tracing` outcome by room id only; `pub async fn retry(timeline: &Timeline, item_key: &str) -> Result<(), SendError>` — locate the item whose `unique_id().0 == item_key` in `timeline.items().await`, `local_echo_send_handle().ok_or(SendError::EchoNotFound)?`, `.unwedge().await.map_err(|e| SendError::Dispatch(e.to_string()))`; a `#[cfg(test)]` `include_str!("send.rs")` guard asserting the SDK send call site is unique and within `submit` (FR-41).
- `src-tauri/crates/keeper-core/src/timeline.rs` -- in `item_to_vm`'s `Message` branch add `send_state: ev.send_state().map(map_send_state)`; add pure `fn map_send_state(state: &EventSendState) -> SendState`; unit-test `map_send_state` for the constructible variants (`NotSentYet`, `Sent`, `SendingFailed` recoverable/unrecoverable via `is_recoverable`).
- `src-tauri/crates/keeper-core/src/account.rs` -- when `subscribe_timeline` opens the room, wrap its `Timeline` in `Arc` and register it (keyed to the subscription/room) so it is reachable by send/retry and dropped on the same teardown paths (unsubscribe + natural completion — extend the shared abort/remove helper). Add `send_text(&self, account_id, room_id, body) -> Result<(), CoreError>` and `retry_send(&self, account_id, room_id, item_key) -> Result<(), CoreError>`: get the live `AccountHandle`, resolve the room's stored `Arc<Timeline>` (missing → `SendError::NoOpenTimeline`; unparsable/unknown room → `RoomNotFound`), delegate to `send::submit`/`send::retry`.
- `src-tauri/crates/keeper-core/src/lib.rs` -- `pub mod send;`.
- `src-tauri/crates/keeper/src/ipc.rs` -- add `#[tauri::command] async fn send_text(state, account_id, room_id, body) -> Result<(), IpcError>` and `send_retry(state, account_id, room_id, item_key) -> Result<(), IpcError>`; extend `to_ipc_error` for `CoreError::Send` → `SendFailed`(`retriable:true`) + a mapping unit test.
- `src-tauri/crates/keeper/src/lib.rs` -- register both commands in `generate_handler!`.
- `src/lib/ipc/gen/` -- regenerated: NEW `SendState.ts`; updated `TimelineItemVm.ts` (Message `sendState`), `IpcErrorCode.ts`.
- `src/lib/ipc/client.ts` -- add `sendText(accountId, roomId, body): Promise<void>` and `retrySend(accountId, roomId, itemKey): Promise<void>` (`invoke` wrappers); re-export `SendState`.
- `src/components/chat/composer.tsx` -- NEW. Controlled `Textarea` (autogrow ≤ 8 lines then scroll, `field-sizing-content` + max-height) + send `Button`; props `{ onSend(body): Promise<void>, disabled }`; Enter→send (trim-guard), ⇧Enter→newline; local `useState` draft cleared on successful send; accessible label. No IPC knowledge itself (parent wires `onSend`).
- `src/components/chat/message-bubble.tsx` -- add optional send-state caption under the bubble for outgoing items: `Sending…`/`Sent` (muted) and persistent destructive `Failed — Retry` button (calls an `onRetry` prop); alignment matches the outgoing bubble; captions only for `Message` items carrying a `sendState`.
- `src/components/layout/conversation-pane.tsx` -- render the `Composer` in a bottom footer (720 px-centered column, `border-t`), disabled unless a room is loaded; wire `onSend={(body) => sendText(accountId, roomId, body)}` and pass an `onRetry={(key) => retrySend(accountId, roomId, key)}` down to bubbles; compute caption placement (failed always; sending/sent on group tail) in the existing grouping pass.
- Tests: `keeper-core` unit (`vm.rs` serde round-trip for `SendState` + extended `Message`; `timeline.rs` `map_send_state`; `send.rs` single-gate `include_str!` guard; `error.rs`/`ipc.rs` `SendError` → `SendFailed`+`retriable:true`); frontend (`composer.test.tsx` Enter-sends / ⇧Enter-newline / empty-guard / clears-on-send / disabled; `message-bubble.test.tsx` sending/sent/failed captions + Retry calls `onRetry`; `conversation-pane.test.tsx` composer present/disabled, `sendText` wired, retry wired; update existing timeline/message-bubble/conversation-pane fixtures to include `sendState: null`).

## Tasks & Acceptance

**Execution:**
- [x] `keeper-core/src/vm.rs` -- `SendState` enum; `Message.sendState: Option<SendState>`; `IpcErrorCode::SendFailed`; serde round-trip tests.
- [x] `keeper-core/src/error.rs` -- `SendError` + `CoreError::Send`; secret-free messages.
- [x] `keeper-core/src/send.rs` -- NEW: `SendTrigger`, `submit` (sole gate), `retry` (via `unwedge`); `include_str!` single-gate guard test; `tracing` by room id, no secrets.
- [x] `keeper-core/src/timeline.rs` -- `map_send_state` + wire `send_state` into `item_to_vm`'s Message; unit-test `map_send_state`.
- [x] `keeper-core/src/account.rs` -- store/drop the room's `Arc<Timeline>` on the shared subscription lifecycle; `send_text`/`retry_send` resolving the live handle + open timeline.
- [x] `keeper-core/src/lib.rs` -- `pub mod send`.
- [x] `keeper/src/ipc.rs` -- `send_text`/`send_retry` commands; `to_ipc_error` `CoreError::Send` → `SendFailed`(retriable) + mapping test.
- [x] `keeper/src/lib.rs` -- register both commands.
- [ ] regenerate ts-rs bindings; commit NEW `SendState.ts` + updated `TimelineItemVm.ts`/`IpcErrorCode.ts`.
- [x] `src/lib/ipc/client.ts` -- `sendText`/`retrySend` wrappers + `SendState` re-export.
- [x] `src/components/chat/composer.tsx` (+ test) -- autogrow Textarea, Enter/⇧Enter, trim-guard, clear-on-send, disabled.
- [x] `src/components/chat/message-bubble.tsx` (+ updated test) -- send-state captions + `Failed — Retry` → `onRetry`.
- [x] `src/components/layout/conversation-pane.tsx` (+ updated test) -- footer composer, `sendText`/`retrySend` wiring, caption placement in grouping pass.
- [ ] update existing timeline/message-bubble/conversation-pane test fixtures for the new `sendState` field.

**Acceptance Criteria:**
- Given the composer in an open Chat, when the user presses Enter (⇧Enter newlines; whitespace-only ignored), then the message dispatches through `send::submit(text, ComposerSend)` — the only function that feeds the SDK `SendQueue` — appears immediately as an outgoing local-echo bubble with a `Sending…` caption that resolves to `Sent`, and the `Textarea` autogrows to 8 lines then scrolls (FR-9, AD-13, UX-DR5).
- Given a send that permanently fails, when the SendQueue reports an unrecoverable failure, then the message shows a persistent destructive `Failed — Retry` caption that never disappears on its own, and Retry re-drives the same wedged echo through the controlled send path (no duplicate message), with all captions following the microcopy voice (sentence case, no error codes) (NFR-5, UX-DR10/UX-DR11).
- Given the FR-41 audit, then a Rust test asserts `send::submit` is the sole content-dispatch entry point in `keeper-core::send` (AD-13).
- Given local echo and state, then the outgoing bubble and every send-state transition arrive solely via the existing per-room `Timeline` diff stream (Rust-authoritative order/state); the TS store never invents, mutates, re-orders, or removes items, and no token, txn id, event raw JSON, or message plaintext beyond the rendered body crosses IPC or reaches `tracing` (AD-8/AD-9/AD-20, NFR-9).
- Given the quality gates, when `bun run check`, `bun run check:rust`, `bun run test:rust`, and `cargo deny check` (from `src-tauri/`) run, then all pass.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 2, low 2)
- defer: 1
- reject: 9
- addressed_findings:
  - `[medium]` `[patch]` The `Composer` was mounted without a `key`, so its local `useState` draft survived a `selectedRoomId` A→B switch — a draft typed in room A stayed in the composer and, once B's timeline loaded, was sendable to room B (cross-room send; violates the spec's "draft cleared … on room change"). Added `key={selectedRoomId}` so the composer remounts fresh per room. `conversation-pane.tsx`.
  - `[medium]` `[patch]` An enqueue-time send failure (`sendText` rejects → `SendFailed`) produces no timeline echo to fall back on, but the composer swallowed the rejection in an empty `catch {}`, so the user got no signal the message never entered the queue (the spec promised enqueue-time failures "surface as this inline error", AD-21). The composer now surfaces an honest inline `role="alert"` caption ("Couldn't send. Check your connection and try again.") on rejection, keeps the draft, and clears the error on the next edit; added two tests. `composer.tsx`, `composer.test.tsx`.
  - `[low]` `[patch]` `open_timeline_for` resolved the room's `Arc<Timeline>` via `HashMap::values().find()` (arbitrary iteration order), so if a room were transiently registered under two subscription ids (StrictMode remount / re-subscribe before the reaper runs) send/retry could target a stale `Timeline` instance whose `unique_id`s no longer match the rendered items (spurious `EchoNotFound` on retry). Now deterministically selects the newest (highest subscription id) entry for the room; also switched the not-live-account branch from the misleading `RoomNotFound` to `NoOpenTimeline`. `account.rs`.
  - `[low]` `[patch]` The conversation pane's `onRetry` fired `void retrySend(...)`, leaving a rejected retry (e.g. the echo reconciled away → `EchoNotFound`) as an unhandled promise rejection. Added `.catch(() => {})` — the persistent `Failed — Retry` caption already remains to invite another attempt. `conversation-pane.tsx`.
  - Deferred (1): the FR-41 single-dispatch-gate guard (`send.rs`) is module-scoped and matches the literal `.send(content)`, so a `Timeline::send` added in another file or a rename/inline of the call would defeat it — appended to `deferred-work.md`.
  - Rejected (9, spec-sanctioned / by-design / unreachable / consistent-with-1.5): transient `Sending…`/`Sent` captions collapsing to the group tail during a burst (spec mandates sending/sent only on the tail; `failed` always renders); `submit` returning `Ok(())` for an empty body (spec-described defensive no-op, composer already guards it); `retry` doing an O(n) `timeline.items()` scan (perf, retries are rare); `SendError::Dispatch(e.to_string())` crossing IPC (matches the accepted Story-1.5 `TimelineError::Build(String)` pattern; matrix-sdk send-error `Display` carries no plaintext/token); empty-room composer "permanently disabled" (unreachable — `onBatch` sets `loaded=true` on Story-1.5's always-emitted initial `Reset`, even empty); retry on a reconciled/non-wedged echo (handled gracefully as `EchoNotFound`); the Failed-echo-still-exposes-a-send-handle assumption (sound per SDK — a wedged local echo stays `Local`; live-only residual); the two-lock ordering in the send path (consistent with `abort_subscription`/`shutdown`, no deadlock); and `let _ = trigger` in `submit` (intentional AD-13 enum seed for the later approval epic).

### 2026-07-04 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 0
- reject: 18
- addressed_findings:
  - none
- Follow-up review (triggered by the prior pass's `followup_review_recommended: true`, which flagged the cross-room-send remount fix and the new inline-error path for independent re-examination). Two fresh-context reviewers (adversarial-general Blind Hunter + edge-case-hunter) ran against the full baseline→HEAD diff. The prior pass's four patches all verified as holding (composer `key`, inline enqueue error, deterministic newest-subscription timeline resolution, `.catch()` on retry). No new actionable this-story defect survived verification — 18 deduplicated findings all rejected.
  - Rejected (18): **(a) already-adjudicated by the prior pass** — `SendError::Dispatch(e.to_string())` crossing IPC (already rejected: matches accepted Story-1.5 `TimelineError::Build`; SDK send-error `Display` carries no plaintext/token, and the composer discards `IpcError.message` entirely); the brittle module-scoped `.send(content)` guard (**already an open `deferred-work.md` entry** — not re-added, per NEW-entries-only); O(n) `timeline.items()` retry scan (perf); `let _ = trigger` dead `SendTrigger` (intentional AD-13 seed); empty-room composer enable (by-design — initial `Reset` sets `loaded`). **(b) spec-sanctioned by-design** — `SendingFailed{is_recoverable:true} → Sending` "shows Sending indefinitely / no Retry" (the intent contract's I/O matrix *mandates* exactly this: `is_recoverable:true` is the SDK's own signal the queue auto-retries on reconnect without user action, and offline/reconnect convergence is explicitly Story 1.7's scope — the proposed "surface Failed + Retry" fix would contradict the frozen contract); send/retry racing an aborting `Timeline` on room-switch (the "Switch room while sending" I/O-matrix row accepts this — the send continues in the SDK queue, the echo reconciles on re-open); `onRetry`/`retrySend` swallowing rejections (the prior pass's deliberate `.catch(() => {})` — the persistent `Failed — Retry` caption is the feedback, and `NoOpenTimeline` on retry is unreachable since retry only fires from a Failed caption in the *open* room). **(c) unreachable** — zero-width-char (U+200B/FEFF) bodies bypassing the whitespace guard (the spec scopes the guard to *whitespace*, and such input is user-crafted with near-zero real incidence → a tiny/blank bubble at worst); null `accountId`/`selectedRoomId` mid-send "losing the draft" (the composer's `disabled` prop and `onSend` derive from the same render — they cannot diverge; `disabled` gates `send()`); `setState`-after-unmount / stale-error on room switch (the `key={selectedRoomId}` remount discards the old instance; React 18 dropped that warning); double-send on held/rapid Enter (the `sending` state guard + `disabled={!canSend}` + `preventDefault` close the window — React commits `setSending(true)` between discrete keydown events); `unwedge` on a non-wedged echo (the Retry affordance renders only for `Failed`, so the UI cannot trigger it on a Sending/Sent item; a reconciled item yields `EchoNotFound`); Retry on a non-own bubble (remote items map to `sendState: null` → no caption). **(d) cosmetic / code-quality nits** — `groupTail` in-place mutation being refactor-fragile (correct as written); autogrow `max-h` line-height approximation (sub-line visual only); no watchdog timeout on a hung `onSend` (`invoke` hang is out-of-scope, not a spec requirement); doc-comment precision on "auto-retrying" (accurate to the SDK's `is_recoverable` semantics).

## Design Notes

**Grounded matrix-sdk-ui / matrix-sdk 0.18.0 API (verified against the vendored source):**
```rust
use matrix_sdk_ui::timeline::{Timeline, EventSendState};
use matrix_sdk::ruma::events::{AnyMessageLikeEventContent, room::message::RoomMessageEventContent};

// submit (the ONLY content-dispatch call site):
let content = AnyMessageLikeEventContent::RoomMessage(RoomMessageEventContent::text_plain(text));
timeline.send(content).await.map_err(|e| SendError::Dispatch(e.to_string()))?; // -> SendHandle (ignored)

// send-state read inside item_to_vm's Event(ev) / Message branch:
send_state: ev.send_state().map(map_send_state)  // Option<&EventSendState> -> Option<SendState>
// EventSendState: NotSentYet{..} | SendingFailed{ error, is_recoverable } | Sent{ event_id }

// retry (re-drive, NOT a new dispatch):
let item = timeline.items().await.into_iter()
    .find(|i| i.as_event().and_then(|e| /* unique_id match */).is_some()); // match by unique_id().0 == item_key
let handle = ev.local_echo_send_handle().ok_or(SendError::EchoNotFound)?; // Option<SendHandle>
handle.unwedge().await.map_err(|e| SendError::Dispatch(e.to_string()))?;
```
`RoomMessageEventContent::text_plain` is the plain-text constructor (ruma-events 0.34). `Timeline::send` delegates to `room.send_queue().send()`; the room's subscribed `Timeline` observes the queue and emits the local echo as a `PushBack`, then `Set` diffs as the state advances — all through the Story-1.5 forwarder unchanged, so no new stream and no TS-side echo.

**Why reuse the same `Timeline` (not `room.timeline()` afresh).** `TimelineUniqueId` is assigned by a `Timeline` instance as it builds items; two independently built `Timeline`s can give the same event different ids. Retry matches the frontend's `key` (`unique_id().0`) against timeline items, so it MUST use the exact `Timeline` that produced them. Hence the account handle stores the open room's `Arc<Timeline>` (shared with the producer) and drops it on teardown, keeping the SDK drop-handle cancellation semantics from Story 1.5 intact.

**Why retry is `unwedge`, not re-`submit`.** Re-submitting fresh content would create a second local echo (duplicate message) and, with the send queue disabled after an unrecoverable error, may not dispatch. `SendHandle::unwedge()` (`local_echo_send_handle()`) re-drives the *existing* wedged request in place — same `unique_id`, caption flips `Failed → Sending… → Sent`. It re-drives an already-fed request, so `submit` remains the sole *content* feed (FR-41 preserved).

**Caption placement.** Reuse Story-1.5's consecutive-same-sender grouping. A `failed` caption always renders under its bubble (persistent, actionable). `sending`/`sent` render only under the last bubble of a same-sender group to avoid noise. Received messages and remote (reconciled) own-messages carry `sendState: null` → no caption.

**Residual (documented, not a gap):** the live send path (`timeline.send`, real `EventSendState` transitions, `unwedge` re-drive, "appears immediately", "resolves to Sent") is exercised only against a real Synapse ≥ 1.114 — the epic exit gate — consistent with Story 1.5's live-only residual. Unit tests cover the pure seams (`map_send_state`, VM serde, the single-gate guard, error mapping) and the frontend composer/caption behavior with a mocked IPC client.

## Verification

**Commands:**
- `bun run check` -- expected: biome + tsc strict + vitest green (new `composer`, updated `message-bubble`/`conversation-pane`/timeline fixtures).
- `bun run check:rust` -- expected: rustfmt + clippy `-D warnings` clean (new `send` module, no `.unwrap()`); core stays tauri-free (workspace guard).
- `bun run test:rust` -- expected: cargo-nextest green; ts-rs bindings regenerate to match committed `src/lib/ipc/gen/` (new `SendState`, changed `TimelineItemVm`/`IpcErrorCode` only).
- `cd src-tauri && cargo deny check` -- expected: license firewall passes (no new crates — ruma/matrix-sdk-ui already dependencies).

**Manual checks (require a real Synapse ≥ 1.114 — automated tests can't exercise live send):**
- `op run --env-file=.env.1p -- bun run tauri dev`: sign in → open a chat → type + Enter → the bubble appears instantly (outgoing/primary) with `Sending…` resolving to `Sent`; it also appears on another client. ⇧Enter inserts a newline; the composer grows to 8 lines then scrolls.
- Force a failure (e.g. revoke access / offline mid-send) → the bubble shows persistent `Failed — Retry`; activating Retry re-drives the same bubble (no duplicate) and it resolves to `Sent` once connectivity returns.

## Auto Run Result

Status: **done**

### Summary
Added text send with honest, Rust-authoritative visible send states, completing Epic 1's send half. `keeper-core` gained a `send` module establishing the FR-41/AD-13 single-dispatch gate: `send::submit(&Timeline, text, trigger)` is the sole call site of `Timeline::send` (a `#[cfg(test)]` source-scan guard asserts it), and `send::retry` re-drives an existing wedged local echo via `SendHandle::unwedge()` (same `unique_id`, no duplicate) rather than re-submitting. Local echo and every send-state transition ride the **existing** Story-1.5 per-room `Timeline::subscribe()` diff stream — `item_to_vm` now reads `EventTimelineItem::send_state()` into a new `sendState: Option<SendState>` on the `Message` VM (`NotSentYet`/recoverable→`Sending`, `Sent`→`Sent`, unrecoverable→`Failed`, remote→`null`), so the frontend synthesizes nothing (AD-8/9/20). `AccountManager` stores each open room's `Arc<Timeline>` (reused for subscribe/send/retry, dropped on teardown) and exposes `send_text`/`retry_send`; the shell adds `send_text`/`send_retry` commands funnelling `CoreError::Send` → `IpcErrorCode::SendFailed`. The frontend gained a `Composer` (autogrow ≤8 lines, Enter sends / ⇧Enter newline, whitespace-guarded, draft cleared per room via `key`, inline error on enqueue failure) in a 720 px-centered `border-t` footer, and `MessageBubble` renders microcopy send-state captions (`Sending…`/`Sent` on the group tail, persistent destructive `Failed — Retry` → `onRetry`).

### Files changed
- `crates/keeper-core/src/send.rs` (NEW) — `SendTrigger`, `submit` (sole gate), `retry` (via `unwedge`), FR-41 single-gate guard test.
- `crates/keeper-core/src/vm.rs` — `SendState` enum; `Message.sendState`; `IpcErrorCode::SendFailed`; serde round-trip tests.
- `crates/keeper-core/src/error.rs` — `SendError { RoomNotFound, NoOpenTimeline, EchoNotFound, Dispatch }` + `CoreError::Send`.
- `crates/keeper-core/src/timeline.rs` — pure `map_send_state` + wired into `item_to_vm`; `OpenTimeline` shares an `Arc<Timeline>`.
- `crates/keeper-core/src/account.rs` — per-room `Arc<Timeline>` registry (dropped on unsubscribe/completion/shutdown); `send_text`/`retry_send`; deterministic newest-subscription timeline resolution.
- `crates/keeper-core/src/lib.rs` — `pub mod send`.
- `crates/keeper/src/ipc.rs` — `send_text`/`send_retry` commands; `to_ipc_error` `Send → SendFailed`(retriable) + tests.
- `crates/keeper/src/lib.rs` — command registration.
- `src/lib/ipc/gen/{SendState.ts (NEW),TimelineItemVm.ts,IpcErrorCode.ts}` — regenerated bindings.
- `src/lib/ipc/client.ts` — `sendText`/`retrySend` wrappers + `SendState` re-export.
- `src/components/chat/composer.tsx` (NEW) + test — autogrow composer, Enter/⇧Enter, whitespace-guard, clear-on-send, inline enqueue-error.
- `src/components/chat/message-bubble.tsx` (+ test) — send-state captions + `Failed — Retry`.
- `src/components/layout/conversation-pane.tsx` (+ test) — composer footer, `sendText`/`retrySend` wiring, `key`-per-room, caption placement.
- timeline/message-bubble/conversation-pane test fixtures updated for `sendState`.

### Review findings
- Two reviewers (adversarial-general Blind Hunter + edge-case-hunter), fresh context. Triage: **0 intent_gap, 0 bad_spec, 4 patch (2 medium, 2 low), 1 defer, 9 reject**. See Review Triage Log.
- **Patches (all applied):** `key={selectedRoomId}` so a draft never leaks across rooms (was a cross-room-send bug); an inline composer error on enqueue-time failure (was silently swallowed); deterministic newest-subscription timeline resolution + accurate `NoOpenTimeline` for a dead account (was arbitrary `.values().find()` + misleading `RoomNotFound`); `.catch()` on the fire-and-forget retry (was an unhandled rejection).
- **Deferred (1):** the FR-41 single-gate guard is module-scoped + literal-string-based (a `Timeline::send` in another file, or a rename/inline, defeats it) → `deferred-work.md`.

### Verification
- `bun run check` ✅ — biome clean, tsc strict clean, vitest **131 passed (15 files)**, core-tauri-free guard passes.
- `bun run check:rust` ✅ — rustfmt `--check` + clippy `--all-targets -D warnings` clean.
- `bun run test:rust` ✅ — cargo-nextest **94 passed, 0 skipped**; ts-rs bindings regenerate idempotently (only new `SendState` + changed `TimelineItemVm`/`IpcErrorCode`).
- `cd src-tauri && cargo deny check licenses bans sources` ✅ (`bans ok, licenses ok, sources ok`). No new crate; the pre-existing OpenSSL unmatched-allowance warning (stories 1.1–1.5) is unchanged and out of scope.
- Not run: live send against a real Synapse ≥ 1.114 (the epic exit gate) — the `timeline.send`/`EventSendState`/`unwedge` path is reasoned-about and unit-tested only at its pure seams. See Manual checks.

### Residual risks
- The live send path (`timeline.send`, real `EventSendState` transitions incl. `is_recoverable`, `unwedge` re-drive, "appears immediately"/"resolves to Sent") runs only against a real homeserver.
- `map_send_state`'s `SendingFailed` arm and `item_to_vm`'s `send_state` read have no unit test (the SDK exposes no public constructor for the failed `EventSendState` / a local-echo `EventTimelineItem`), consistent with Story 1.5's live-only residual.
- The FR-41 guard correctly enforces the present code but is not refactor-resilient or crate-wide (deferred).
- `followup_review_recommended: true` — the pass fixed a cross-room-send correctness bug (via composer remount, which has React lifecycle/IME implications) and added a new inline-error UI path on the user-facing send flow; an independent follow-up is worthwhile.

### Follow-up review pass (2026-07-04)
The recommended independent follow-up ran: two fresh-context reviewers (adversarial-general + edge-case-hunter) re-examined the full baseline→HEAD diff. All four prior-pass patches verified as holding, and no new actionable defect survived verification — 18 deduplicated findings all rejected (already-adjudicated / spec-sanctioned by-design / unreachable / cosmetic; see the Review Triage Log follow-up entry). No code changed this pass. `followup_review_recommended` lowered to `false` (the review is now converged) and `status` set to `done`.
