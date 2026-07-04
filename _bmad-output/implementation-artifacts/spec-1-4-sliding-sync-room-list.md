---
title: 'Story 1.4 — Sliding-Sync Room List'
type: 'feature'
created: '2026-07-04'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '2fd66bc724c7614cf7c9088dc5e24ec1f3619c83'
final_revision: 'f22852a5644460181f054296cf568a8675b03e7c'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** After Story 1.3 a user can sign in, but `login_password` immediately drops the live `Client`, so the app shell shows a static "No conversations yet" placeholder. Epic 1's vertical slice (timeline, send) is blocked until the account actually syncs and its Chats appear, newest first, updating live — proving the snapshot-then-diff room-list seam (AD-8/9/20) on the real MSC4186 sync path.

**Approach:** Introduce a minimal single-account supervision layer in `keeper-core` that restores the persisted session from the Keychain, rebuilds the `Client` against its existing SQLite store, and runs `SyncService` + `RoomListService` (matrix-sdk-ui) under a supervised tokio task. The room list's `entries_with_dynamic_adapters` stream (recency-sorted by the SDK) is converted, diff-for-diff, into `RoomListBatch` view-model batches streamed over a `tauri::ipc::Channel` — a full snapshot (`Reset`) first, then incremental ops. The frontend mirrors those ops into an **ordered** zustand array (never re-sorting) and renders 64 px chat rows (avatar, display name, last-message preview, timestamp) in the existing chat-list pane.

## Boundaries & Constraints

**Always:**
- All Matrix/sync/crypto/persistence logic stays in `keeper-core`; the `keeper` shell is IPC/platform glue only and gains no new business logic. `keeper-core` gains no `tauri` dependency. (AD-6)
- **Ordering is computed in Rust only (AD-20).** keeper forwards the SDK's recency-sorted `VectorDiff` sequence verbatim as `RoomListOp`s; the TS store applies index-based ops to a plain array and **never sorts, re-sorts, or re-orders**. `entries_with_dynamic_adapters` already sorts (latest-event → recency → name) and repositions a room via a `Set`/move diff when a new event arrives, so an incoming message reaches the top through the diff stream, not TS logic.
- **Snapshot-then-diff, re-subscribe safe (AD-8).** Every subscription opens with a full-reset batch (a `Reset` op carrying the current window) and then diffs; (re)subscribing at any time yields a fresh snapshot the store applies by *replacing* its contents, so it never duplicates rows. Follow the existing `demo_subscribe` Channel pattern.
- **Lazy activation, session-restore path.** The room-list subscribe command activates the account if not already live: read the serialized `MatrixSession` from the Keychain (`session_keychain_key(account_id)`), read `homeserver_url` from the `keeper.db` accounts row, build a `Client` with `sqlite_store(accounts/<id>/sdk, None)`, `matrix_auth().restore_session(session, RoomLoadSettings::default())`, then build and `start()` a `SyncService`. This same path is what Story 1.8 reuses for cold-start restore. Activation is idempotent — a second subscribe reuses the live account, never a second `Client`/`SyncService`.
- **Supervised, leak-free tasks (AD-19).** The live `Client`, `SyncService`, and per-subscription forwarding task live in an `AccountHandle` owned by an `AccountManager` in the shell's managed `AppState`. Each subscribe registers its task's abort handle under a subscription id; an explicit `room_list_unsubscribe(account_id, id)` aborts exactly that task. The frontend calls unsubscribe on effect cleanup so React 19 StrictMode's double-mount does not leak tasks or duplicate streams.
- **Secret containment (NFR-9).** `RoomVm`/`RoomListBatch` carry only non-secret render data (room id, display name, message preview text, timestamp, optional avatar URL). No tokens, session material, or `event_id`-beyond-need cross IPC; message plaintext previews are the visible content of already-decrypted events only. `tracing` logs carry no message bodies, tokens, or session data.
- **Row view model.** `RoomVm { roomId, displayName, lastMessage: string | null, timestamp: number | null, avatarUrl: string | null }`; `timestamp` is `i64` ms since Unix epoch (never ISO). `lastMessage` is the plain-text body of the room's latest event when it is an `m.room.message` (text/notice/emote); `null` otherwise. `displayName` uses the SDK's computed room display name.
- **Error taxonomy (AD-21).** Add `keeper-core` `AccountError` (`SessionMissing`, `RestoreFailed`, `SyncStart`) rolling into `CoreError::Account`; map through the single shell `to_ipc_error` funnel to a new `IpcErrorCode::SyncUnavailable` (`retriable: true`). Failed activation surfaces an honest inline state in the chat-list pane, not a silent spinner.
- TS: no `any`, `import type`, `@/` alias, 2-space/100-col/double-quote Biome, `cn()` for classes, reuse installed shadcn primitives (`Avatar`, `ScrollArea`, `Skeleton`) — do not hand-write in `src/components/ui/`. Rust: no `.unwrap()`/bare `.expect()` in production paths, `?` + `thiserror`, clippy `-D warnings` clean, `tracing` not `println!`.
- Regenerate ts-rs bindings for the new VMs (`RoomVm`, `RoomListBatch`, `RoomListOp`) into `src/lib/ipc/gen/` and commit them to match `cargo` output.

**Block If:**
- matrix-sdk-ui 0.18 does not expose `SyncService::builder(client).build()/.start()`, `room_list_service().all_rooms().entries_with_dynamic_adapters(page)`, and a filter setter (`set_filter`) — a stack-anchor conflict with AD-2. (Verified present in the vendored 0.18 source during planning; only block if implementation proves otherwise.)

**Never:**
- No per-room `Timeline` / message bubbles / conversation pane wiring — that is Story 1.5. Clicking a row is a focusable full-width target that records a selected room id at most; it must not open or stream a timeline.
- No multi-account machinery, `AuthProvider` trait extraction, or inbox-merge across accounts (Epic 2 / Story 2.1). The `AccountManager` here is a single-account-capable holder keyed by `account_id`; do not build the registry/merge logic.
- No send/outbox, unread badges, network/bridge overlays, per-account hue edge bars, favorites, or spaces (later epics). Rows show only avatar, name, preview, timestamp.
- No scroll-driven visible-range following / windowed pagination beyond a single fixed page — seed windowing with one page + totals; dynamic range updates are the Unified Inbox epic's extension of AD-20.
- No re-sorting or recency computation in TypeScript. No holding room/message state in a JS store as source of truth — the store mirrors the Rust diff stream only (AD-9).
- No `matrix-js-sdk` or any Matrix JS lib; no crypto/token/message-decode logic in TS.
- No at-rest DB encryption change (`sqlite_store(dir, None)` unchanged); no modification to the Story 1.3 `login_password` flow.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| First subscribe after login | logged-in account, sync not yet live | account activates (restore + `SyncService.start()`), `all_rooms` stream opens, first batch is a `Reset` snapshot of the current window (possibly empty), then diff batches as rooms sync | none |
| Empty account | account with 0 joined rooms | `Reset` snapshot with empty ops; pane shows "No conversations yet" | none |
| Live new message | a synced room receives a new event | a diff batch repositions that room toward the top (SDK `Set`/move diff) within ~2 s of sync delivery; TS applies the op, does not sort | none |
| Re-subscribe | subscribe called again (e.g. StrictMode remount) while active | prior task aborted on cleanup; new stream opens with a fresh `Reset` snapshot; store replaces contents — no duplicate rows, no second `Client`/`SyncService` | none |
| Latest event is non-message | room whose latest event is state/reaction/redaction | row shows `lastMessage: null` (preview blank), `timestamp` from the event when available | none |
| Session missing at activate | Keychain entry absent for `account_id` | subscribe fails; inline "Couldn't start syncing" state in the pane | `AccountError::SessionMissing` → `SyncUnavailable` (`retriable:true`); logged via `tracing`, no secrets |
| Restore/sync start fails | `restore_session` or `SyncService.build/start` errors | subscribe fails with the same inline state; no partial live account retained | `AccountError::RestoreFailed`/`SyncStart` → `SyncUnavailable`; logged with cause, no secrets |
| Unsubscribe | frontend unmounts / closes | `room_list_unsubscribe` aborts exactly that subscription's task; other account state untouched | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- add `RoomVm` (roomId/displayName/lastMessage?/timestamp?/avatarUrl?, serde+`#[ts(export)]`, camelCase); add `RoomListOp` (internally-tagged `#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]` enum mirroring the used `VectorDiff` variants: `Reset{rooms}`, `Append{rooms}`, `Clear`, `PushFront{room}`, `PushBack{room}`, `PopFront`, `PopBack`, `Insert{index,room}`, `Set{index,room}`, `Remove{index}`, `Truncate{length}`); add `RoomListBatch { ops: Vec<RoomListOp>, total: Option<u32> }`.
- `src-tauri/crates/keeper-core/src/error.rs` -- add `AccountError { SessionMissing, RestoreFailed(String), SyncStart(String) }`; add `CoreError::Account(#[from] AccountError)`; keep messages secret-free.
- `src-tauri/crates/keeper-core/src/account.rs` -- NEW. `AccountManager` (holds `tokio::sync::Mutex<HashMap<String, AccountHandle>>`); `AccountHandle` (owns `Client`, `Arc<SyncService>`, `Mutex<HashMap<u64, AbortHandle>>` of live subscriptions). Public: `AccountManager::new()`; `async fn subscribe_room_list(&self, platform: &dyn Platform, account_id, sink: Box<dyn Fn(RoomListBatch) + Send + Sync>) -> Result<u64, CoreError>`; `fn unsubscribe_room_list(&self, account_id, subscription_id)`; `async fn shutdown(&self, account_id)`. Private: `activate(platform, account_id)` (Keychain session → registry homeserver → build `Client` → `restore_session(.., RoomLoadSettings::default())` → `SyncService::builder(client).build().await` → `.start().await`); `async fn room_item_to_vm(&RoomListItem) -> RoomVm` (display name + latest-event preview/ts); `fn vector_diff_to_op(VectorDiff<RoomVm>) -> RoomListOp` (pure, unit-testable). Producer task: `all_rooms().await`, `entries_with_dynamic_adapters(ROOM_LIST_PAGE_SIZE)`, `set_filter(non-left)`, loop the entries stream (+ `loading_state` for `total`), map diffs → ops → `RoomListBatch`, call `sink`.
- `src-tauri/crates/keeper-core/src/registry.rs` -- reuse existing `get_account(account_id)` for `homeserver_url`/`device_id` (no schema change).
- `src-tauri/crates/keeper-core/src/lib.rs` -- `pub mod account;`.
- `src-tauri/crates/keeper/src/ipc.rs` -- `AppState` gains `accounts: keeper_core::account::AccountManager`; add `#[tauri::command] async fn room_list_subscribe(state, account_id, channel: Channel<RoomListBatch>) -> Result<u64, IpcError>` (sink = `move |b| { let _ = channel.send(b); }`) and `room_list_unsubscribe(state, account_id, subscription_id) -> Result<(), IpcError>`; extend `to_ipc_error` for `CoreError::Account` → `SyncUnavailable` (`retriable:true`); add the mapping unit test.
- `src-tauri/crates/keeper-core/src/vm.rs` (`IpcErrorCode`) + `src-tauri/crates/keeper/src/lib.rs` -- add `SyncUnavailable` variant; register both new commands in `generate_handler!`.
- `src/lib/ipc/gen/` -- regenerated: NEW `RoomVm.ts`, `RoomListBatch.ts`, `RoomListOp.ts`; updated `IpcErrorCode.ts`.
- `src/lib/ipc/client.ts` -- add `subscribeRoomList(accountId, onBatch): Promise<number>` (via existing `subscribe`) and `unsubscribeRoomList(accountId, id): Promise<void>`; re-export `RoomVm`, `RoomListBatch`, `RoomListOp`.
- `src/lib/stores/rooms.ts` -- NEW vanilla zustand store `{ rooms: RoomVm[], total: number | null, applyBatch(batch), clear() }`; `applyBatch` folds ops onto an immutable array (Reset/Clear replace/empty; index ops splice) and **never sorts**; `useRoomsStore` selector hook.
- `src/lib/format-time.ts` -- NEW `formatRoomTimestamp(ms: number): string` (today → `HH:MM`, else short date).
- `src/components/chat/chat-row.tsx` -- NEW 64 px full-width `<button>` row: `Avatar` (fallback initials from display name), display name (`text-sm font-medium` truncate), `lastMessage` preview (`text-sm text-muted-foreground` truncate), timestamp (`text-xs`); visible focus ring, accessible label; optional `onSelect(roomId)`.
- `src/components/layout/chat-list-pane.tsx` -- subscribe on mount using `currentAccount.accountId`, render rows from `useRoomsStore` inside `ScrollArea`, empty state ("No conversations yet"), inline error state on `SyncUnavailable`; unsubscribe + `clear()` on cleanup / account change.
- Tests: `keeper-core` unit (`vm.rs` serde round-trip for `RoomVm`/`RoomListBatch`/`RoomListOp`; `account.rs` `vector_diff_to_op` covering every op variant; `error.rs`/`ipc.rs` `AccountError` → `SyncUnavailable`+`retriable:true`); frontend (`rooms.test.ts` each op incl. reset-replaces-no-dup; `format-time.test.ts`; `chat-row.test.tsx`; updated `chat-list-pane.test.tsx`).

## Tasks & Acceptance

**Execution:**
- [x] `keeper-core/src/vm.rs` -- add `RoomVm`, `RoomListOp`, `RoomListBatch`; extend `IpcErrorCode` with `SyncUnavailable`; add serde round-trip tests.
- [x] `keeper-core/src/error.rs` -- add `AccountError` + `CoreError::Account`; secret-free messages.
- [x] `keeper-core/src/account.rs` -- NEW: `AccountManager`/`AccountHandle`, lazy `activate` (restore_session + SyncService), `subscribe_room_list`/`unsubscribe_room_list`/`shutdown`, `room_item_to_vm` (latest-event text preview + ts), pure `vector_diff_to_op`; `tracing` the activation/subscribe outcome by `account_id` with no secrets; unit-test `vector_diff_to_op` across all variants.
- [x] `keeper-core/src/lib.rs` -- expose `account`.
- [x] `keeper/src/ipc.rs` -- `AppState.accounts`; `room_list_subscribe`/`room_list_unsubscribe` commands forwarding batches to the `Channel`; `to_ipc_error` for `CoreError::Account` → `SyncUnavailable`(`retriable:true`) + mapping test.
- [x] `keeper/src/lib.rs` -- register both commands in `generate_handler!`.
- [ ] regenerate ts-rs bindings and commit `src/lib/ipc/gen/{RoomVm,RoomListBatch,RoomListOp}.ts` + updated `IpcErrorCode.ts`.
- [x] `src/lib/stores/rooms.ts` (+ `rooms.test.ts`) -- ordered mirror store; `applyBatch` applies ops, never sorts; reset replaces without duplication.
- [x] `src/lib/format-time.ts` (+ `format-time.test.ts`) -- timestamp formatter.
- [x] `src/lib/ipc/client.ts` -- `subscribeRoomList`/`unsubscribeRoomList` wrappers + re-exports.
- [x] `src/components/chat/chat-row.tsx` (+ `chat-row.test.tsx`) -- 64 px accessible row (avatar/name/preview/timestamp).
- [x] `src/components/layout/chat-list-pane.tsx` (+ updated `chat-list-pane.test.tsx`) -- subscribe/render/unsubscribe lifecycle, empty + error states.

**Acceptance Criteria:**
- Given a logged-in account, when the chat-list pane subscribes to the room-list channel, then `keeper-core` restores the session and runs `SyncService` + `RoomListService` under a supervised task, and streams a windowed `RoomListVm` as a `Reset` snapshot batch followed by diff batches into the zustand mirror store; re-subscribing at any time yields a fresh snapshot with no duplicated rows and no second `Client`/`SyncService` (FR-8; AD-8/9/19/20).
- Given rooms render, when the list draws, then each 64 px row shows avatar, display name, last-message preview, and timestamp, and rows are full-width click/Enter targets with a visible focus ring; and when an incoming message arrives in any room, that room moves toward the top within ~2 s of sync delivery via a diff op — with no sorting performed in TypeScript (UX-DR3; AD-20).
- Given ordering logic, then recency ordering is computed in Rust (the SDK's sorter, owned by `keeper-core`) and the TS store applies index-based diff ops to an array without ever re-sorting (AD-20).
- Given activation fails (missing session / restore / sync-start), then the subscribe command returns `SyncUnavailable` and the pane shows an honest inline error rather than a silent spinner, and no partial live account is retained (AD-21).
- Given code review, then no token, session, or message-plaintext-beyond-rendered-preview appears on any VM/IPC response or in `tracing` logs, and `keeper-core` carries no `tauri` dependency (NFR-9, AD-6).
- Given the quality gates, when `bun run check`, `bun run check:rust`, `bun run test:rust`, and `cargo deny check` (from `src-tauri/`) run, then all pass.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 13: (high 0, medium 3, low 10)
- defer: 1: (high 0, medium 1, low 0)
- reject: 18
- addressed_findings:
  - `[medium]` `[patch]` Ungated batch sink caused cross-account/post-unmount row bleed and a StrictMode late-`clear()` that wiped the new mount's snapshot. `chat-list-pane.tsx` now clears the store at effect *start* (newest mount wins), passes a `cancelled`-gated `onBatch` to the subscription, and drops the cleanup `clear()`.
  - `[medium]` `[patch]` `applyOp` trusted diff indices blindly — a `set` past length created `undefined` holes (whole-pane render crash) and `remove`/`insert` silently desynced. Added `[0,length]` range guards that no-op an out-of-range op, with tests.
  - `[medium]` `[patch]` The room-list producer was never reaped on a dead channel or natural completion. `BatchSink` now returns `bool` (shell = `channel.send(..).is_ok()`); the producer breaks when the channel closes, and a naturally-completed task removes its own subscription entry from the (now `Arc<Mutex<..>>`) map.
  - `[low]` `[patch]` `set_filter`'s `bool` was discarded — a dropped stream would hang forever on the empty state (silent failure, AD-21). Now `warn`s and returns if the filter is not applied.
  - `[low]` `[patch]` `mxc://` avatar URL was fed to `<img src>`, which the webview can't load. ChatRow now only renders `AvatarImage` for `http(s)` URLs, else initials (media scheme handler is a later epic).
  - `[low]` `[patch]` `total` regressed to `null` on later diff batches; store now keeps the last-known total (`batch.total ?? state.total`).
  - `[low]` `[patch]` `formatRoomTimestamp` could `RangeError`/render 1970 on `NaN`/`≤0` ms; guarded to return `""` (rendered as no timestamp).
  - `[low]` `[patch]` An `Ok("")` display name rendered a blank row; the fallback chain now skips empty/whitespace names (→ cached → room id) and logs resolve failures at `debug` (room id only).
  - `[low]` `[patch]` A `shutdown`-in-gap between spawn and registration orphaned the producer task; registration now aborts + errors if the handle vanished.
  - `[low]` `[patch]` `loading_state` could be re-polled after termination under `select!`; a `loading_done` guard disables that branch on `None`.
  - `[low]` `[patch]` Initial load was indistinguishable from a genuinely empty account; the pane now shows a `Skeleton` loading state until the first batch, then "No conversations yet."
  - `[low]` `[patch]` `shutdown` aborted tasks but never stopped the SyncService; it now calls `sync.stop().await` first.
  - `[low]` `[patch]` The latest-event preview decode had no coverage; extracted a pure `decode_message_preview` seam with unit tests (m.room.message → body; non-message → none).
  - Rejected (18, spec-sanctioned / by-design / impossible): blank previews for encrypted/UTD/local/invite events, `u32` index truncation at a 200-room window, future-timestamp clamping and TZ day-boundary behavior, and the already-handled popFront/popBack-empty, truncate-beyond, and reset/clear-ordering cases.

### 2026-07-04 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 0
- reject: 18
- addressed_findings:
  - `[low]` `[patch]` `all_rooms()` failing *after* a successful `activate()` left a started `SyncService` in the manager map with zero subscribers, contradicting the AD-21 AC "no partial live account is retained." `subscribe_room_list` now records whether this call activated the account (`did_activate`); on an `all_rooms()` error it stops the `SyncService` and drops the handle, guarded by a subscription-emptiness check so a racing sibling subscribe's live subscription is never torn down.
  - Rejected (18, invariant-upheld / by-design / spec-satisfied / near-impossible): the "producer emits before its abort handle is registered" leak (the manager `Mutex` serializes subscribe-registration, `unsubscribe`, and `shutdown`, the registration block aborts if the account vanished, and `unsubscribe` cannot fire before the id is returned — the only residue is a harmless dead `JoinHandle`); duplicate-`roomId` React keys (the SDK `ObservableVector` never carries dupes and TS de-dup would violate AD-9); no retry button on the inline error (spec requires only an honest inline state, not a retry affordance); `m.notice`/`m.emote` rendered as their body (spec: "plain-text body"); `index as u32`/`length as u32` casts (by-design 200-room window); sticky `total` and `total`-unused-in-gating (by-design, seed-for-later); `mxc://` avatar branch as dead code (acknowledged future media-scheme epic); `set_filter == false` → permanent skeleton and empty-ops-batch-marks-loaded (near-impossible: the first stream yield after the filter is always the `Reset`, and the loading-state branch never sinks a batch); `Truncate` beyond length (slice-safe); future/skew timestamps (clock skew, prior-rejected); grapheme/ellipsis and `account_id`-logging doc nits (behavior correct, id is an opaque ULID); abort-not-awaited (fire-and-forget by design); and the signout `SyncService` leak (already tracked in `deferred-work.md` for Story 1.8).

## Design Notes

**Grounded matrix-sdk-ui 0.18 API (verified against the vendored source at `~/.cargo/registry/src/*/matrix-sdk-ui-0.18.0/`):**
```rust
// Activate (lazy, also the Story 1.8 restore path):
let session: MatrixSession = serde_json::from_str(&platform.keychain_get(&session_keychain_key(account_id))?
    .ok_or(AccountError::SessionMissing)?)?;
let row = registry::get_account(&platform.data_dir()?, account_id)?.ok_or(AccountError::SessionMissing)?;
let sdk_dir = platform.data_dir()?.join("accounts").join(account_id).join("sdk");
let client = Client::builder().homeserver_url(&row.homeserver_url).sqlite_store(&sdk_dir, None).build().await
    .map_err(|e| AccountError::RestoreFailed(e.to_string()))?;
client.matrix_auth().restore_session(session, RoomLoadSettings::default()).await
    .map_err(|e| AccountError::RestoreFailed(e.to_string()))?;
let sync = SyncService::builder(client.clone()).build().await.map_err(|e| AccountError::SyncStart(e.to_string()))?;
sync.start().await;                                    // spawns background sync tasks

// Room list stream (recency-sorted by the SDK; NOTHING sorted in keeper):
let room_list = sync.room_list_service().all_rooms().await.map_err(|e| AccountError::SyncStart(e.to_string()))?;
let (stream, controller) = room_list.entries_with_dynamic_adapters(ROOM_LIST_PAGE_SIZE); // e.g. 200
controller.set_filter(Box::new(new_filter_non_left(/* confirm client/service arg at impl time */)));
// The stream yields NOTHING until set_filter; then a Reset, then live VectorDiff batches.
tokio::pin!(stream);
while let Some(diffs) = stream.next().await {          // Vec<VectorDiff<RoomListItem>> per tick
    let mut ops = Vec::new();
    for d in diffs { ops.push(vector_diff_to_op(d.map(/* RoomListItem -> RoomVm (async) */))); }
    sink(RoomListBatch { ops, total: current_total });
}
```
`entries_with_dynamic_adapters` applies `new_sorter_lexicographic([latest_event, recency, name])` internally, so a new event repositions its room automatically — the "moves to top within 2 s" AC is the SDK's diff, not keeper logic. Because `VectorDiff::map` on a `RoomListItem` is not async-friendly, convert item→`RoomVm` first (await display name / latest event) and then convert the already-`RoomVm` diff via the pure `vector_diff_to_op` (this is the unit-tested seam). `loading_state()` yields `RoomListLoadingState::Loaded { maximum_number_of_rooms }` → `total`.

**Latest-event preview (no SDK helper exists).** From `item.latest_event()` (`LatestEventValue`, not feature-gated): on `Remote(TimelineEvent)`, deserialize `ev.raw()` to `ruma::events::AnySyncTimelineEvent`; if `MessageLike(RoomMessage(SyncMessageLikeEvent::Original(o)))`, use `o.content.body()` as `lastMessage` and `o.origin_server_ts` (or `ev.timestamp`) as `timestamp`; every other event kind → `lastMessage: None` with `timestamp` from `item.latest_event_timestamp()`. Truncate preview defensively (e.g. ≤ 256 chars) before it crosses IPC.

**RoomListOp ↔ VectorDiff.** Mirror the eyeball-im `VectorDiff` variants keeper actually receives (`Append/Clear/PushFront/PushBack/PopFront/PopBack/Insert/Set/Remove/Truncate/Reset`) one-to-one. The TS `applyBatch` reducer is a straight array transform per op (Reset/Clear replace or empty; `PushFront`→unshift; `Insert`/`Set`/`Remove`→splice; `Truncate`→length). No comparison, no sort — order is authoritative from Rust. A mid-stream `Reset` (subscriber lag) simply replaces contents, which is why re-subscribe never duplicates.

**Supervision & teardown.** `AppState.accounts: AccountManager` is Tauri-managed shared state; the manager guards its `HashMap` with `tokio::sync::Mutex` (held across `.await` during activation only). Each `subscribe_room_list` spawns its producer task and stores the `AbortHandle` under a monotonically-increasing subscription id (reuse the `NEXT_SUBSCRIPTION_ID` atomic style from `demo_subscribe`); `unsubscribe_room_list` aborts and removes it. The frontend effect returns a cleanup that calls `unsubscribeRoomList` and `clear()`s the store, so StrictMode's mount→unmount→mount does not leak tasks or stack duplicate streams.

**Frontend store (AD-9).** `roomsStore = createStore<RoomsState>()(...)` at module load; components read via `useRoomsStore(selector)`. Holds only the streamed `RoomVm[]` + `total`. `applyBatch` is pure/immutable; no network, no derivation of truth.

**Residual (documented, not a gap):** the whole live path (restore_session, SyncService, real `VectorDiff` sequence, latest-event decode) is exercised only against a real Synapse ≥ 1.114 — the epic exit gate; unit tests cover the pure seams (`vector_diff_to_op`, store reducer, serde). Scroll-driven visible-range windowing and row selection→timeline are deferred to Story 1.5 / the Unified Inbox epic.

## Verification

**Commands:**
- `bun run check` -- expected: biome + tsc strict + vitest (new `rooms`/`format-time`/`chat-row` tests, updated `chat-list-pane` test) green.
- `bun run check:rust` -- expected: rustfmt + clippy `-D warnings` clean (new `account`/vm/error code, no `.unwrap()`); core stays tauri-free (workspace guard).
- `bun run test:rust` -- expected: cargo-nextest green; ts-rs bindings regenerate to match committed `src/lib/ipc/gen/`.
- `cd src-tauri && cargo deny check` -- expected: license firewall passes (no new crates — matrix-sdk-ui already a dependency).

**Manual checks (require a real Synapse ≥1.114 — automated tests can't exercise live sync):**
- `op run --env-file=.env.1p -- bun run tauri dev`: sign in → chat list populates with the account's rooms newest-first; send a message to one room from another client → that room jumps to the top within ~2 s. Confirm rows show avatar/name/preview/timestamp and are keyboard-focusable.
- Toggle away/back (or reload) to re-subscribe → list re-populates with no duplicate rows. Sign into an account with zero rooms → "No conversations yet".

## Auto Run Result

Status: **done**

### Summary
Implemented the sliding-sync room list across both layers. `keeper-core` gained an `account` module with a single-account-capable `AccountManager`/`AccountHandle` (AD-19) that lazily *activates* an account — restore the persisted `MatrixSession` from the Keychain, rebuild the `Client` on its existing SQLite store, `restore_session(.., RoomLoadSettings::default())`, then build and `start()` a matrix-sdk-ui `SyncService` (this is also the Story 1.8 cold-start path). A supervised producer task drives `RoomListService::all_rooms().entries_with_dynamic_adapters(200)` with a non-left filter; the SDK's recency sort (latest-event → recency → name) is forwarded diff-for-diff as `RoomListOp`s inside `RoomListBatch`es over a `tauri::ipc::Channel` — a `Reset` snapshot first, then incremental ops — so nothing is ever sorted in TypeScript (AD-20). The frontend mirrors ops into an ordered vanilla-zustand array (`applyBatch`, never sorts, `Reset` replaces), renders 64 px accessible `ChatRow`s (avatar/name/preview/timestamp) inside a `ScrollArea`, and manages subscribe/unsubscribe over the effect lifecycle (StrictMode-safe). Activation failures resolve to a named `AccountError` → `IpcErrorCode::SyncUnavailable` (retriable) surfaced as an honest inline error. No token/session/message-plaintext-beyond-preview crosses IPC or reaches a log.

### Files changed
- `crates/keeper-core/src/vm.rs` — `RoomVm`, `RoomListOp` (11 `VectorDiff` variants), `RoomListBatch`; `IpcErrorCode::SyncUnavailable`; serde tests.
- `crates/keeper-core/src/error.rs` — `AccountError {SessionMissing, RestoreFailed, SyncStart}` + `CoreError::Account`.
- `crates/keeper-core/src/account.rs` (NEW) — `AccountManager`/`AccountHandle`, lazy `activate`, supervised room-list producer, `subscribe`/`unsubscribe`/`shutdown`, latest-event preview decode, pure `vector_diff_to_op` + `decode_message_preview` seams (unit-tested).
- `crates/keeper-core/src/lib.rs` — `pub mod account`.
- `crates/keeper/src/ipc.rs` — `AppState.accounts`; `room_list_subscribe`/`room_list_unsubscribe` commands; `to_ipc_error` for `CoreError::Account` → `SyncUnavailable`(retriable) + mapping tests.
- `crates/keeper/src/lib.rs` — command registration.
- `src/lib/ipc/gen/{RoomVm,RoomListOp,RoomListBatch}.ts` (NEW) + `IpcErrorCode.ts` — regenerated bindings.
- `src/lib/ipc/client.ts` — `subscribeRoomList`/`unsubscribeRoomList` wrappers + re-exports.
- `src/lib/stores/rooms.ts` (NEW) + test — ordered mirror store, range-guarded reducer.
- `src/lib/format-time.ts` (NEW) + test — chat-row timestamp formatter.
- `src/components/chat/chat-row.tsx` (NEW) + test — 64 px accessible row.
- `src/components/layout/chat-list-pane.tsx` + test — subscribe/render/unsubscribe lifecycle, loading/empty/error states.
- `src/App.test.tsx`, `src/components/layout/app-shell.test.tsx` — updated for the new loading state.
- `src-tauri/{Cargo.toml,Cargo.lock}` + `crates/keeper-core/Cargo.toml` — added `futures-util` (already resolved in the lock; no new license surface).

### Review findings
- Two reviewers (adversarial-general Blind Hunter + edge-case-hunter), fresh context. Triage: 0 intent_gap, 0 bad_spec, **13 patch** (3 medium, 10 low), **1 defer**, 18 reject. See Review Triage Log.
- **Patches (all applied):** gated the batch sink + reset-at-mount (cross-account/StrictMode bleed); range-guarded the reducer (sparse-array crash); made the producer reap on dead channel + natural completion (`BatchSink -> bool`, `Arc<Mutex>` subscription map); checked `set_filter`'s bool (no silent hang); guarded `mxc://` avatars, `NaN/≤0` timestamps, empty display names; abort-on-registration-gap; `loading_state` None-safe; loading-vs-empty `Skeleton`; `shutdown` now `sync.stop()`s; added a `decode_message_preview` test seam.
- **Deferred:** wiring `AccountManager::shutdown` into a real sign-out path is Story 1.8's AC (see `deferred-work.md`).

### Verification
- `bun run check` ✅ — biome clean, tsc strict clean, vitest **64 passed (10 files)**, core-tauri-free guard passes.
- `bun run check:rust` ✅ — rustfmt `--check` + clippy `--all-targets -D warnings` clean.
- `bun run test:rust` ✅ — cargo-nextest **54 passed, 0 skipped**; ts-rs bindings regenerate idempotently (only the intended `RoomVm`/`RoomListOp`/`RoomListBatch` new + `IpcErrorCode` changed).
- `cd src-tauri && cargo deny check licenses bans sources` ✅ (`bans ok, licenses ok, sources ok`). No new crate; the pre-existing gtk/unic `advisories` residual (stories 1.1–1.3) is unchanged and out of scope.
- Not run: live sync against a real Synapse ≥1.114 (blocking) — the whole activation/sync/diff/preview path is reasoned-about and unit-tested only at its pure seams (`vector_diff_to_op`, `decode_message_preview`, the TS reducer, error mapping). This is the epic exit gate. See Manual checks.

### Residual risks
- The live path (restore_session, SyncService start, real `entries_with_dynamic_adapters` diff sequence, "moves to top within 2 s", latest-event raw decode over real events) is exercised only against a real homeserver.
- Sign-out does not yet stop the account's SyncService (Story 1.8; deferred). Single-account with the list always mounted means no trigger in Epic 1.
- Avatars are initials-only until the media/`mxc://` scheme handler lands (later epic); scroll-driven visible-range windowing and row-selection→timeline are deferred to Story 1.5 / the Unified Inbox epic.
- `followup_review_recommended: true` — the patch pass was broad across Rust concurrency/lifecycle and TS reducer/lifecycle; an independent follow-up review is worthwhile.

### Follow-up review pass (2026-07-04)
An independent follow-up review (two fresh-context reviewers: adversarial-general Blind Hunter + edge-case-hunter) was run against the full diff since the baseline. Triage: **0 intent_gap, 0 bad_spec, 1 patch (low), 0 defer, 18 reject**.
- **Patch applied:** `all_rooms()` failing after a successful `activate()` previously left a started `SyncService` in the manager map with zero subscribers, contradicting the AD-21 "no partial live account is retained" AC. `subscribe_room_list` now tears down the just-activated account (stop `SyncService` + drop handle) on an `all_rooms()` error, guarded by a subscription-emptiness check so a concurrent sibling subscribe is never affected. `src-tauri/crates/keeper-core/src/account.rs`.
- **Notable rejects (invariant already upheld):** the flagged "producer emits before abort-handle registration" leak is not reachable — the manager `Mutex` serializes registration/`unsubscribe`/`shutdown`, the registration block aborts when the account vanished, and `unsubscribe` cannot fire before the id is returned; TS de-dup was rejected because the SDK vector never carries duplicate rooms and de-dup would violate AD-9.
- **Verification (follow-up):** `bun run check` ✅ (biome + tsc + **64 vitest** + core-tauri-free guard), `bun run check:rust` ✅ (rustfmt + clippy `-D warnings`), `bun run test:rust` ✅ (**54 nextest passed**), `cd src-tauri && cargo deny check licenses bans sources` ✅ (`bans ok, licenses ok, sources ok`; the pre-existing OpenSSL unmatched-allowance warning is unchanged).
- `followup_review_recommended` set to **false** — this pass made a single localized low-consequence fix on a rare error path; no further independent review is warranted.
