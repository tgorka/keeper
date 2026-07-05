---
title: 'Receipts, Typing, and History Pagination'
type: 'feature'
created: '2026-07-04'
status: 'done'
baseline_revision: '3a666559caffc60d622cc50331742661edb7817d'
final_revision: 'c4f12a45b2d22957a0b30aa67d2a011b74b151fb'
review_loop_iteration: 0
followup_review_recommended: false
context: ['{project-root}/docs/project-context.md']
warnings: ['multiple-goals', 'oversized']
---

<intent-contract>

## Intent

**Problem:** keeper never emits its own read receipts or typing notices, never renders others' read state or typing, and cannot load history older than the initial sync window — the timeline shows no "seen" state, no "X is typing…", and back-scrolling stops dead at the first cached event with no boundary and no way to fetch more. The SDK's read-receipt tracking is off (`room.timeline()` default), so per-item receipts are empty, and there is no `signals` seam confining the receipt/typing/presence SDK surface (AD-14).

**Approach:** Create the `keeper-core::signals` module as the SOLE emitter of SDK receipt/typing APIs (AD-14): mark-room-read (public `m.read`) and typing-notice set/subscribe. Enable read-receipt tracking on the timeline and carry each message's other-member readers (opaque user ids) in the Message VM so the frontend renders micro-avatar read markers and an own-message read tick. Wire back-pagination (`Timeline::paginate_backwards`) triggered by scrolling near the top, with a live pagination-status stream driving an honest history-boundary row (spinner while loading, "start of conversation" when the homeserver has no more, an explicit offline stop instead of an infinite spinner), and preserve scroll position when older events prepend so a ≥10k-event back-scroll never yanks the view.

## Boundaries & Constraints

**Always:**
- Every SDK receipt/typing/presence *emit* or *subscribe* call (`Timeline::mark_as_read`, `Timeline::send_single_receipt`, `Room::typing_notice`, `Room::subscribe_to_typing_notifications`) lives ONLY in `keeper-core::signals` — enforced by a crate-wide source-scan guard test that asserts those method names appear in no other `keeper-core/src/*.rs` file (AD-14; stronger than the send-gate's single-file guard by design). Reading already-populated per-item receipts for rendering (`EventTimelineItem::read_receipts`) is render data, not emission, and stays in `timeline.rs`.
- Read receipts and typing are emitted as PUBLIC (`m.read` / normal typing) — this is normal, non-Incognito operation. Incognito/private-receipt policy is Epic 8; do not add scope/precedence logic here.
- Only opaque Matrix user ids cross IPC for readers and typists (already true of `sender`); typists additionally carry a resolved display name for the "… is typing" copy. No presence, no per-user avatars/mxc, no receipt event ids, no crypto material (NFR-9, AD-1).
- Back-pagination prepends older events through the EXISTING timeline diff stream (`PushFront`/`Insert` ops the store already applies) — no second timeline channel. Pagination reads history and is NOT a signal: `paginate_backwards` stays in `timeline.rs`/`account.rs`, not `signals`.
- The history-boundary row is honest per state: paginating → spinner + "Older history loads from your homeserver"; homeserver start reached → "This is the start of the conversation"; offline → an explicit offline message and NO spinner (stops, never spins forever) (epic UX).
- Prepending older history must preserve the user's visual scroll position (compensate `scrollTop` by the height delta); auto-scroll-to-bottom fires only for bottom growth when the user was already near the bottom.
- Copy follows UX-DR10 voice: sentence case, no exclamation marks, honest; Glossary nouns capitalized where they apply.

**Block If:**
- `matrix-sdk-ui` 0.18 does not expose `Timeline::paginate_backwards`, `Timeline::live_back_pagination_status`, `Timeline::mark_as_read`/`send_single_receipt`, `TimelineBuilder::track_read_marker_and_receipts`, or `Room::typing_notice`/`subscribe_to_typing_notifications` (all confirmed present for 0.18 — see Design Notes).

**Never:**
- No presence (online/last-seen) — AD-14 names presence as the seam's scope, but this story emits/renders only receipts + typing; presence is future.
- No Incognito/private-receipt policy, no per-scope precedence, no typing suppression — Epic 8 (Stories 8.1, 8.2). Only the `signals` module seam is created now.
- No per-reader avatar images / profile-avatar (mxc) resolution — read markers render initials-based micro-avatars derived from the opaque user id. Real avatar images are future (profile work / Story 4.6).
- No read-receipt-driven unread counts or inbox badges — that is Epic 4 (Story 4.1). This story renders in-timeline read state only.
- No threaded read receipts, no fully-read marker UI, no jump-to-unread.
- No virtualization rewrite of the timeline; keep the existing plain scroll container.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Mark room read | Room open, latest event present | `signals::mark_read` → `Timeline::mark_as_read(Read)` dispatches a public `m.read`; other clients see it | best-effort; dispatch failure logged, non-fatal (no UI error) |
| Mark read, empty timeline | Room open, no events yet | No-op (`mark_as_read` returns `false`); no error | none |
| Others' read receipt arrives | A member's read receipt lands on a message | That message's VM gains the member's id in `readers`; a micro-avatar renders at that position (via SDK `Set` diff) | none |
| Own message read | ≥1 other member's receipt sits on the user's own message | The own message shows a read indicator (tick) in addition to the reader cluster | none |
| Own receipt in list | Own user id present in an item's `read_receipts` | Filtered out of `readers` (never render self as a reader) | none |
| Start typing | Composer gains non-empty text | `set_typing(true)` emitted, throttled to ≤1/3s while typing | best-effort; failure ignored |
| Stop typing | Send, clear, blur, or ~5s idle | `set_typing(false)` emitted once | best-effort; failure ignored |
| Others typing | Typing stream yields non-empty user list | Typing row renders "<name> is typing…" / two names / "Several people are typing…" within ~2s | none |
| Typing empties / offline | Empty typing list, or account offline | Typing row hides (renders nothing) | none |
| Scroll near top | User scrolls within threshold of the top, not already paginating, not at start, online | `paginate_backwards(N)` called; older events prepend via diff stream; scroll position preserved | error → boundary shows a retriable inline error, spinner stops |
| Reached homeserver start | `paginate_backwards` returns `true` / status `Idle { hit_start }` | Boundary row shows "This is the start of the conversation"; no further pagination attempts | none |
| Paginate while offline | Top reached, account offline | Boundary shows an explicit offline message and does NOT spin; no pagination attempt | none |
| Pagination in flight | `live_back_pagination_status` yields `Paginating` | Boundary shows a spinner + "Older history loads from your homeserver" | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/signals.rs` (new) -- The SOLE receipt/typing SDK emit/subscribe gate (AD-14). `pub async fn mark_read(timeline: &Timeline) -> Result<bool, SignalError>` → `timeline.mark_as_read(ReceiptType::Read)` (public `m.read`). `pub async fn set_typing(room: &Room, typing: bool) -> Result<(), SignalError>` → `room.typing_notice(typing)`. `pub fn subscribe_typing(room: &Room) -> (EventHandlerDropGuard, broadcast::Receiver<Vec<OwnedUserId>>)` → wraps `room.subscribe_to_typing_notifications()`. Guard test scans all `keeper-core/src/*.rs` and asserts `.mark_as_read(`, `.send_single_receipt(`, `.typing_notice(`, `.subscribe_to_typing_notifications(` appear only in `signals.rs`.
- `src-tauri/crates/keeper-core/src/lib.rs` -- Add `pub mod signals;`.
- `src-tauri/crates/keeper-core/src/error.rs` -- Add `SignalError { Dispatch(String) }` (best-effort receipt/typing failures) into `CoreError`; reuse `TimelineError` for pagination (`RoomNotFound`/`NoOpenTimeline`/`Build`).
- `src-tauri/crates/keeper-core/src/timeline.rs` -- (1) In `open_timeline`, build via `Timeline::builder(&room).track_read_marker_and_receipts(TimelineReadReceiptTracking::MessageLikeEvents).build()` instead of `room.timeline()` so per-item receipts populate. (2) In `item_to_vm`, populate a new `readers` field on `Message` from `ev.read_receipts()` keys, excluding `own_user_id`, as opaque id strings (pure, sync — no member lookup). (3) `pub async fn paginate_backwards(timeline: &Timeline, num_events: u16) -> Result<bool, TimelineError>` wrapping `timeline.paginate_backwards`. (4) `pub async fn run_pagination_status_producer(timeline: Arc<Timeline>, sink: PaginationSink)` over `timeline.live_back_pagination_status()` mapping `PaginationStatus` → `PaginationStatusBatch`.
- `src-tauri/crates/keeper-core/src/vm.rs` -- (1) Add `readers: Vec<String>` to `TimelineItemVm::Message` (other members whose latest read receipt is on this item; own excluded; opaque ids only). (2) `TypistVm { user_id, display_name: Option<String> }` + `TypingBatch { typists: Vec<TypistVm> }`. (3) `PaginationState { Paginating, Idle }` + `PaginationStatusBatch { state: PaginationState, hit_start: bool }`. All derive serde camelCase + ts-rs `#[ts(export)]`.
- `src-tauri/crates/keeper-core/src/account.rs` -- Wrappers + producer lifecycle mirroring `subscribe_connection_status`: `mark_room_read(account_id, room_id)` (resolve open timeline → `signals::mark_read`, swallow best-effort failure); `set_typing(account_id, room_id, typing)` (resolve Room → `signals::set_typing`); `paginate_backwards(account_id, room_id, num_events) -> Result<bool>` (resolve open timeline → `timeline::paginate_backwards`); `subscribe_typing(platform, account_id, room_id, sink) -> u64` (resolve Room → spawn producer that reads the typing broadcast, resolves each id's display name via `room.get_member_no_sync`, emits `TypingBatch`) + `unsubscribe_typing`; `subscribe_pagination_status(platform, account_id, room_id, sink) -> u64` (resolve the registered `Arc<Timeline>` → spawn `run_pagination_status_producer`) + `unsubscribe_pagination_status`. Typing/pagination tasks register in the account's `subscriptions` map and reap on drop like the other producers (AD-19).
- `src-tauri/crates/keeper/src/ipc.rs` -- One-shot commands `mark_room_read`, `set_typing`, `paginate_backwards`; streaming `typing_subscribe`/`typing_unsubscribe`, `pagination_status_subscribe`/`pagination_status_unsubscribe`; error arms in `to_ipc_error` for `SignalError` (non-retriable, best-effort) and pagination (`TimelineUnavailable`, retriable).
- `src-tauri/crates/keeper/src/lib.rs` -- Register the six new commands in `invoke_handler`.
- `src/lib/ipc/client.ts` -- Wrappers: `markRoomRead(accountId, roomId)`, `setTyping(accountId, roomId, typing)`, `paginateBackwards(accountId, roomId, numEvents): Promise<boolean>`, `subscribeTyping(accountId, roomId, onBatch)` + `unsubscribeTyping`, `subscribePaginationStatus(...)` + `unsubscribePaginationStatus` (mirror the existing `subscribe` helper).
- `src/components/chat/read-receipts.tsx` (new) -- Micro-avatar cluster from `readers` (initials from the user-id localpart, deterministic color from the id), max ~3 chips + "+K" overflow; labeled for a11y. Renders nothing when `readers` is empty.
- `src/components/chat/message-bubble.tsx` -- Add `readers: string[]` to the message VM prop; render `ReadReceipts` under the bubble; in `SendStateCaption`, when `isOwn && sendState === null && readers.length > 0`, show a "Read" tick alongside the "Sent" caption (ticks on own messages).
- `src/components/chat/typing-indicator.tsx` (new) -- Row rendering "<name> is typing…", "<a> and <b> are typing…", or "Several people are typing…"; aria-live polite; renders nothing when no typists.
- `src/components/chat/history-boundary.tsx` (new) -- Top-of-timeline boundary row: `paginating` → spinner (`aria-busy`) + "Older history loads from your homeserver"; `offline` → "You're offline — older messages will load when you reconnect" (no spinner); `atStart` → "This is the start of the conversation"; `error` → honest retriable message. Not an aria-live flooder for static states.
- `src/components/chat/composer.tsx` -- Emit typing: on non-empty input change `setTyping(true)` throttled ≤1/3s; on send / clear / blur / ~5s idle `setTyping(false)`. Best-effort (ignore rejections).
- `src/components/layout/conversation-pane.tsx` -- On room open, subscribe typing + pagination status (local state, cleanup on room change/unmount) and render `TypingIndicator` (between timeline and composer) + `HistoryBoundary` (top of the list). Replace the unconditional scroll-to-bottom effect with: preserve `scrollTop` across prepends (compensate by height delta), and only auto-scroll to bottom on bottom growth when the user was near the bottom. Add a top-scroll/sentinel trigger calling `paginateBackwards` when near the top, online, not already paginating, not at start. Call `markRoomRead` when the room is viewed and on new incoming content while focused (throttled).
- `src/lib/ipc/gen/*` -- Regenerated ts-rs bindings (`TimelineItemVm` gains `readers`; new `TypistVm`, `TypingBatch`, `PaginationState`, `PaginationStatusBatch`).
- Tests: Rust `#[cfg(test)]` + TS `*.test.tsx` -- see test tasks.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/signals.rs` + `lib.rs` -- New `signals` module: `mark_read`, `set_typing`, `subscribe_typing`; crate-wide sole-gate guard test. -- AD-14 seam, single receipt/typing surface.
- [x] `src-tauri/crates/keeper-core/src/error.rs` -- `SignalError` into `CoreError`. -- Typed error surface.
- [x] `src-tauri/crates/keeper-core/src/timeline.rs` -- Enable `track_read_marker_and_receipts` in `open_timeline`; add `readers` to the Message VM in `item_to_vm` (own-filtered); `paginate_backwards`; `run_pagination_status_producer`. -- Receipts populate; readers render; history loads with status.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- `readers` on `Message`; `TypistVm`/`TypingBatch`; `PaginationState`/`PaginationStatusBatch`. -- Typed VM boundary (AD-7).
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- `mark_room_read`, `set_typing`, `paginate_backwards`; `subscribe_typing`/`unsubscribe_typing`, `subscribe_pagination_status`/`unsubscribe_pagination_status` with producer lifecycle. -- Per-account wiring (AD-19).
- [x] `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- Six commands + registration + error mapping. -- IPC surface (AD-4).
- [x] `src/lib/ipc/client.ts` -- Typed wrappers for the six commands. -- Typed IPC access.
- [x] `src/components/chat/read-receipts.tsx` + `message-bubble.tsx` -- Reader micro-avatars + own-message read tick. -- Read-state rendering (AC1).
- [x] `src/components/chat/typing-indicator.tsx` -- Typing row copy variants. -- Typing rendering (AC2).
- [x] `src/components/chat/history-boundary.tsx` -- Boundary states (paginating/offline/atStart/error). -- Honest history boundary (AC3/AC4).
- [x] `src/components/chat/composer.tsx` -- Throttled typing emit + stop conditions. -- Typing emission (AC2).
- [x] `src/components/layout/conversation-pane.tsx` -- Subscribe typing + pagination status; scroll-preserving prepend + near-bottom auto-scroll; top-scroll paginate trigger; mark-read on view. -- Wiring + smooth pagination (AC3/AC4).
- [x] `src-tauri/**` tests -- signals sole-gate guard (crate-wide); `item_to_vm` excludes own user from `readers` and includes others; VM serde shapes (`readers`, `TypingBatch`, `PaginationStatusBatch`); `paginate_backwards`/status producer resolve + gate as feasible under the SDK-constructor limit. -- Lock the contract.
- [x] `src/**` tests -- read-receipts: initials/overflow, empty renders nothing, own-read tick; typing-indicator: one/two/many/empty copy; history-boundary: paginating spinner, offline stops (no spinner), atStart copy; composer: typing throttle + stop on send/clear/blur; conversation-pane: prepend preserves scroll, top-scroll triggers paginate (online only, not at start), offline stops. -- Cover the I/O matrix + ACs.

**Acceptance Criteria:**
- Given the user opens and views a Room with messages, when the read is marked, then keeper emits a public `m.read` receipt other Matrix clients observe; and when other members' read receipts arrive, keeper renders their read position as micro-avatar markers and shows a read tick on the user's own read messages (FR — receipts).
- Given the user types in the composer, then keeper emits a typing notice (throttled) and stops it on send/clear/blur/idle; and given another member is typing, keeper renders "<name> is typing…" (or a multi-user variant) within ~2 s (FR — typing).
- Given the user scrolls to the top of a long Room, when older history is available on the homeserver, then keeper back-paginates without freezing or yanking the view (≥10k events), prepends the older events in place, and shows a boundary row with a spinner while loading; when the homeserver has no more history, the boundary states the conversation start (FR — pagination).
- Given the account is offline and the user scrolls to the top, then the boundary row states it is offline and stops rather than spinning forever, and resumes when back online (epic UX honesty rule).
- Given `bun run check:all`, then Biome, tsc, vitest, rustfmt, clippy (`-D warnings`), cargo-nextest, and `cargo deny check` all pass; ts-rs bindings regenerate with the new `readers`/typing/pagination types and no drift.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 0, low 2)
- defer: 0
- reject: 22: (high 0, medium 0, low 22)
- addressed_findings:
  - `[low]` `[patch]` The history-boundary `error` state rendered a plain `<div>` with no `role`/`aria-live`, so a screen-reader user who scrolled to trigger a back-pagination that then failed got no announcement that loading failed or that a Retry existed — the least-recoverable boundary state was the least accessible (the `paginating` row already carries `role="status"`/`aria-busy`). Added `role="alert"` to the error row so the failure and its Retry are announced. Existing history-boundary tests still pass.
  - `[low]` `[patch]` `runPaginate` set `paginatingRef = true` without first checking it, so the single-flight guard was enforced only on the near-top scroll path (`onScroll`) and not inside `runPaginate` itself; the boundary Retry button (`onRetryPagination`) called `runPaginate` directly, so a rapid Retry could admit a second concurrent `paginateBackwards`. Moved the `paginatingRef` check into `runPaginate`'s entry guard so every entry point is single-flight, matching the spec's "`paginatingRef` is the sole in-flight guard" design. (Low reachability in practice — the Retry button unmounts on the first click's re-render — but this restores the stated invariant.)
  - Notable rejections (22, all low/no-consequence). Two were verified false positives against the vendored SDK/JS semantics: (a) a claimed "first `paginate_backwards` call is a lazy no-op that stalls history loading" — the `None`/lazy arm still prepends already-cached older events by lowering `subscriber_skip_count` (emitting `PushFront` diffs), and the network arm loops until it yields non-empty events or reaches the start, so `Ok(false)` never means "zero events prepended"; and the paired "transient empty page churn" rests on the same misread; (b) a claimed `hueOf` negative-hue at `INT_MIN` — in JS `Math.abs(-2147483648)` is the double `2147483648` and `% 360 = 128` (positive), so the C/Rust `i32`-overflow intuition does not apply. Others: read `✓` tick rendering regardless of `groupTail` (documented-intentional; receipt-position semantics put each member's receipt on a single item, and the reader micro-avatars disambiguate — cosmetic, already accepted); group-room "Read" tick semantics (accepted prior pass); `classifyBatch` insert-at-index>0 (older history prepends as `pushFront`/insert-at-0, never mid-list); async-media prepend height jump and multi-batch `prevScrollHeight` baseline (accepted prior pass); composer unmount emitting `setTyping(false)` to the "wrong" room (the unmounting instance's `onTypingRef` still points at the *old* room's callback — emits to the correct old room; accepted prior pass); `markRoomRead` twice on open (by-design: prompt mark-on-view + debounced re-mark for content that loads after open); re-mark on a non-message tail key (marking the room read on any new tail is correct); serial `get_member_no_sync` in the typing producer (local cache lookup, tiny typist sets); AD-14 guard not scanning the shell crate (AD-14 scopes the seam to `keeper-core`); `num_events` clamp raising 0→1 and the clamp comment referencing a frontend constant (unreachable — the sole caller passes `PAGINATE_BATCH`); focused-timeline `None` branch swallowing the sink result (unreachable — keeper only opens live timelines); best-effort `.catch(() => {})` swallowing all rejections (matches the established codebase convention for best-effort IPC); `initialsOf`/`hueOf` collisions (decorative); and a test asserting scroll geometry via `data-msg-key` (test-quality nit).

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 6: (high 0, medium 4, low 2)
- defer: 0
- reject: 19: (high 0, medium 0, low 19)
- addressed_findings:
  - `[medium]` `[patch]` The user's public `m.read` receipt was marked only once on room-open and never re-advanced when new messages arrived while the room stayed open — deviating from the Code Map's "mark read on view **and on new incoming content while focused (throttled)**", so other clients saw the reader stuck behind. Added a debounced effect that re-marks the newest item read (~1 s after content settles) while the pane is open; the mark-on-view stays for prompt open-time receipt. New behavior is covered transitively by the existing subscribe/mark test.
  - `[medium]` `[patch]` The pagination state machine self-cleared its error and raced its single-flight guard: the status-stream `onBatch` called `setPaginationError(false)` on **every** batch (so a genuine failed fetch's retriable boundary was silently replaced by the next idle/paginating status) and flipped `paginatingRef` on any idle (a coalesced idle mid-fetch could clear the guard and admit a concurrent paginate). Reworked into a single `runPaginate` helper: the fetch promise solely owns the in-flight guard (cleared unconditionally in `finally`) and a sticky retriable error (persists until Retry); the resolved `hitStart` boolean is now authoritative so pagination stops at the homeserver start even if the status stream is silent. Added a failed-fetch→sticky-error→Retry test (a status batch can no longer clear it).
  - `[medium]` `[patch]` Scroll preservation used a single whole-container `scrollHeight` delta, so a bottom-append (a peer message) while the user was scrolled up reading history was mis-compensated as a prepend and jolted the view **down** — violating the "never yank the view" AC. Now the batch is classified from its ops (`reset` / `prepend` = pushFront/insert-at-0 / other) and only a genuine prepend compensates scrollTop; a bottom-append while scrolled up leaves scrollTop untouched; a reset anchors to the bottom. Added a bottom-append-no-jump test.
  - `[medium]` `[patch]` The AD-14 sole-gate guard test enumerated sibling files with a hand-maintained `include_str!` list ("add new modules here"), so it **failed open** — a future module calling a receipt/typing SDK method would not be scanned. Rewrote it to walk `CARGO_MANIFEST_DIR/src` at test time (recursing submodules), so a new offending module is caught automatically. Restores the spec's stated "crate-wide" invariant. Also corrected the boundary-state precedence so `offline` overrides a transient in-flight spinner and a stale error (offline stops rather than spins, per the epic honesty rule); added an offline-mid-pagination precedence test.
  - `[low]` `[patch]` The reader micro-avatar hue hash reduced modulo 360 on **every** step, collapsing entropy so distinct readers clustered on the same color. Now accumulates in full 32-bit width and reduces once at the end (`Math.abs(hash) % 360`).
  - `[low]` `[patch]` `paginate_backwards` accepted any `u16` from IPC (the webview trust boundary) and passed it straight to the SDK. Clamped to `[1, 200]` (`MAX_PAGINATE_EVENTS`) in the core wrapper as defense-in-depth against an outsized back-fill.
  - Notable rejections (19, all low/no-consequence): producer keep-alive parity (holding `Room`/`Arc<Timeline>` already pins the client; teardown is via the drop guard / channel close); typing own-id filter (the SDK filters own id at the source, verified); `room_for` membership (UI only calls it for the joined open room; dispatch is best-effort); own "Read ✓" tick semantics in group rooms (the reader micro-avatars disambiguate; cosmetic); typing throttle per-session vs wall-clock (emitting on a fresh session start is desirable; SDK throttles on-wire); unmount typing to the prior room on a fast switch (typing auto-expires server-side); `title={userId}` tooltip (opaque id already crosses IPC as `sender`, by-design no reader name); focused-timeline `None` pagination branch (keeper only opens live timelines — unreachable); multi-batch `prevScrollHeight` baseline (synchronous `onBatch` reads all see the same pre-paint DOM); substring-guard defeatable via aliasing/re-export (inherent to a source-scan lint, matches the accepted send-gate precedent); readers dedup (the SDK receipt map keys are unique by construction); and other cosmetic/unreachable items.

## Design Notes

**Verified SDK surface (matrix-sdk / matrix-sdk-ui 0.18, from vendored source).**
- `Timeline::paginate_backwards(num_events: u16) -> Result<bool, Error>` — returns whether the start of the timeline was hit. Older events arrive through the already-subscribed diff stream as `PushFront`/`Insert`, which the frontend store's `applyDiffOp` already handles — so pagination needs a trigger + status, not new render plumbing.
- `Timeline::live_back_pagination_status() -> Option<(PaginationStatus, impl Stream<Item = PaginationStatus>)>`; `PaginationStatus` (from `matrix_sdk::event_cache`) is `Idle { hit_timeline_start }` | `Paginating`. Map to `PaginationStatusBatch { state, hit_start }`.
- `Timeline::mark_as_read(ReceiptType::Read) -> Result<bool>` (delegates to `send_single_receipt` on the latest event); `ReceiptType` is `ruma …::create_receipt::v3::ReceiptType`.
- `TimelineBuilder::track_read_marker_and_receipts(TimelineReadReceiptTracking)` — variants `AllEvents` | `MessageLikeEvents` | `Disabled`; **default is off**, so the current `room.timeline()` yields empty `read_receipts()`. Switch to the builder with `MessageLikeEvents`.
- `EventTimelineItem::read_receipts() -> &IndexMap<OwnedUserId, Receipt>` — per-item readers; the SDK places each user's receipt on their latest-read item, so per-item `readers` is exactly that user's read position (no extra "furthest read" computation).
- `Room::typing_notice(bool) -> Result<()>`; `Room::subscribe_to_typing_notifications() -> (EventHandlerDropGuard, broadcast::Receiver<Vec<OwnedUserId>>)` — own user already filtered out by the SDK.

**Why `signals` is crate-wide-guarded, but pagination and receipt-reading are not.** AD-14 scopes the seam to the SDK APIs that *emit/subscribe* receipts/typing/presence. `deferred-work.md` records that the send-gate guard is single-file and cannot enforce a crate-wide invariant; the `signals` guard is written crate-wide from the start (scan every `keeper-core/src/*.rs`, assert the emit/subscribe method names appear only in `signals.rs`) because AD-14 states exactly that invariant. Reading `EventTimelineItem::read_receipts()` is rendering data (no network emission) and belongs in the timeline mapper; `paginate_backwards`/`live_back_pagination_status` read history and are not signals — both stay out of `signals.rs`.

**Micro-avatars are initials, not images.** Resolving per-reader avatars needs async profile/mxc work and `item_to_vm` is a pure sync `VectorDiff::map` callback with no room handle. So the Message VM carries only opaque reader ids (already the shape of `sender`), and the frontend renders deterministic initials chips. Typing runs in an async producer that *does* hold the `Room`, so typists carry a resolved display name for nicer copy. Real avatar images and unread badges are deferred (profile work / Epic 4).

**Scroll preservation is the anti-freeze mechanism.** The current pane unconditionally sets `scrollTop = scrollHeight` on every batch, which would jam the view to the bottom whenever older history prepends. Back-pagination requires: capture `scrollHeight` before applying a prepend batch and restore `scrollTop += (newHeight - oldHeight)` after, and only auto-scroll to bottom when new content is appended at the bottom and the user was already near it. This keeps a ≥10k-event back-scroll smooth (the epic's test bar).

## Verification

**Commands:**
- `bun run check` -- Biome + tsc + vitest green (new read-receipts/typing/boundary/composer/pane tests).
- `bun run check:rust` -- rustfmt clean + clippy `-D warnings` clean.
- `bun run test:rust` -- cargo-nextest green; ts-rs bindings regenerate with `readers`/typing/pagination types and no git drift.
- `cargo deny check` (from `src-tauri/`) -- licenses/bans/sources clean; no new dependency (advisories-only pre-existing gtk-rs baseline excepted, as in prior stories).

**Manual checks (real second session, test credentials in 1Password):**
- Open an encrypted Room in keeper and in Element; confirm Element shows keeper's read receipt advancing and keeper renders Element's read marker as a micro-avatar and a read tick on own read messages.
- Type in Element; confirm keeper shows "<name> is typing…" within ~2 s and clears when they stop; type in keeper and confirm Element shows keeper typing.
- In a Room with long history, scroll to the top; confirm older messages load with the boundary spinner, the view does not jump, and the boundary states the conversation start when the homeserver runs out.
- Go offline and scroll to the top; confirm the boundary says offline and does not spin; reconnect and confirm pagination resumes.

## Auto Run Result

Status: done

**Summary:** Implemented Story 3.9 end to end across three features plus a new architectural seam. (1) **Read receipts** — `open_timeline` now builds with `track_read_marker_and_receipts(MessageLikeEvents)` so per-item receipts populate; `item_to_vm` carries a `readers: Vec<String>` (other members' opaque ids, own excluded, via the pure `readers_of` helper); the frontend renders an initials-based micro-avatar cluster at each read position and a read tick on own read messages; the room is marked read (public `m.read`) on view and, after the review patch, re-marked debounced on new incoming content while focused. (2) **Typing** — the new `keeper-core::signals` module gates `Room::typing_notice`/`subscribe_to_typing_notifications`; the composer emits throttled typing (≤1/3 s) and stops on send/clear/blur/idle/unmount; a typing-indicator row renders one/two/"several" copy. (3) **History pagination** — `Timeline::paginate_backwards` (clamped page size) is triggered by a near-top scroll, older events prepend over the existing diff stream, a `live_back_pagination_status` producer drives an honest history-boundary row (paginating spinner / offline stop / start-of-conversation / retriable error), and the scroll layout effect preserves the visual position on prepend so a ≥10k-event back-scroll never yanks the view. (4) **Signals seam (AD-14)** — `signals.rs` is the sole caller of the four receipt/typing SDK methods, enforced by a crate-wide, fail-closed, directory-walking guard test.

**Files changed:**
- `src-tauri/crates/keeper-core/src/signals.rs` (new) — sole receipt/typing gate (`mark_read`, `set_typing`, `subscribe_typing`) + crate-wide directory-walk sole-gate guard test.
- `src-tauri/crates/keeper-core/src/timeline.rs` — receipt tracking in `open_timeline`; `readers_of` + `readers` on the Message VM; `paginate_backwards`; `run_pagination_status_producer` + `map_pagination_status`.
- `src-tauri/crates/keeper-core/src/account.rs` — `mark_room_read`, `set_typing`, `paginate_backwards` (clamped to `MAX_PAGINATE_EVENTS`), `subscribe_typing`/`subscribe_pagination_status` (+ unsubscribe) with the supervised-producer lifecycle; `run_typing_producer`.
- `src-tauri/crates/keeper-core/src/{vm,error,lib}.rs` — `readers` field, `TypistVm`/`TypingBatch`, `PaginationState`/`PaginationStatusBatch`; `SignalError`; `pub mod signals`.
- `src-tauri/crates/keeper/src/{ipc,lib}.rs` — six commands (`mark_room_read`, `set_typing`, `paginate_backwards`, typing + pagination-status subscribe/unsubscribe) + registration + error mapping (`signalDispatchFailed`).
- `src/lib/ipc/client.ts` + `src/lib/ipc/gen/*` — typed wrappers; regenerated bindings (`TimelineItemVm.readers`, `IpcErrorCode.signalDispatchFailed`, + 4 new type files).
- `src/components/chat/{read-receipts,typing-indicator,history-boundary}.tsx` (new, + tests) — micro-avatar cluster (full-width hue hash), typing row, honest boundary row.
- `src/components/chat/{composer,message-bubble}.tsx` (+ tests) — throttled typing emit; reader cluster + own-message read tick.
- `src/components/layout/conversation-pane.tsx` (+ tests) — typing + pagination-status subscriptions; debounced re-mark-read; batch-classified scroll preservation; single-flight `runPaginate` with authoritative `hitStart` + sticky error; offline-first boundary precedence; near-top paginate trigger.

**Review findings breakdown:** intent_gap 0, bad_spec 0, patch 6 (medium 4, low 2 — all applied + tested), defer 0, reject 19 (all low).
- **Patches applied:** (1) [med] re-mark read on new incoming while focused (debounced) — closed a deviation from the explicit spec clause; (2) [med] pagination state machine — sticky retriable error a status batch can't clear, single-flight guard owned by the fetch promise, authoritative `hitStart`; (3) [med] scroll preservation — classify batches so a bottom-append while scrolled up no longer yanks the view; (4) [med] AD-14 guard rewritten to a fail-closed directory walk + offline-first boundary precedence; (5) [low] full-width reader hue hash; (6) [low] clamp IPC `num_events`.
- **Rejected (19, all low):** producer keep-alive parity, typing own-id re-filter (SDK filters at source), room-membership check, group "Read" tick semantics, typing throttle per-session, unmount-typing-to-prior-room, `title` tooltip, focused-timeline unreachable branch, multi-batch scroll baseline, substring-guard aliasing, readers dedup (map keys unique), and other cosmetic/unreachable items.

**Verification performed (independently re-run after patches):**
- `bun run check` — Biome clean, tsc clean, **431** vitest tests pass (46 files; +3 new pagination/scroll/precedence tests).
- `bun run check:rust` — `cargo fmt --check` clean, clippy `-D warnings` clean.
- `bun run test:rust` — cargo-nextest **289** pass (incl. the rewritten crate-wide fail-closed signals guard).
- `cargo deny check` (from `src-tauri/`) — bans/licenses/sources clean; `advisories FAILED` is the pre-existing gtk-rs/`proc-macro-error2`/unicode unmaintained baseline transitive from Tauri's Linux backend — no new dependency (`Cargo.toml`/`Cargo.lock` unchanged).
- ts-rs bindings regenerated with drift limited to the intended 6 files.

**Follow-up review:** recommended (`followup_review_recommended: true`). This pass made behavior-touching corrections to a core interaction (the pagination single-flight/error state machine, scroll-preservation classification, and boundary-state precedence) plus a security-relevant guard hardening (fail-open → fail-closed) and a functional receipts fix — enough breadth and consequence to benefit from an independent second look, especially given none of the live-SDK paths (real receipts/typing across Element, ≥10k back-scroll, network-kill mid-paginate) are exercisable in automated gates.

**Follow-up review pass (2026-07-04):** An independent second review (Blind Hunter + Edge Case Hunter, deduped and re-classified) produced 0 intent_gap, 0 bad_spec, 2 low patches, 0 defer, 22 low rejects. Patches applied and verified: (1) `role="alert"` on the history-boundary `error` state so a failed back-pagination and its Retry are announced to assistive tech; (2) moved the `paginatingRef` single-flight check into `runPaginate`'s entry guard so the boundary Retry path (not just the scroll trigger) is single-flight. Both are localized `.tsx` changes; `bun run check` re-run green (Biome clean, tsc clean, **431** vitest tests pass). Two of the rejected findings were confirmed false positives against the vendored SDK/JS semantics (the "lazy first-paginate stall" — the lazy arm still prepends cached events via `subscriber_skip_count`; and a `hueOf` `INT_MIN` negative-hue — `Math.abs` returns a positive double in JS). No behavior-changing or spec-level findings survived, so no further follow-up review is warranted.

**Residual risks:** Live behavior against a real second Matrix session — Element observing keeper's advancing read receipt, keeper rendering Element's read marker + typing within ~2 s, a genuine ≥10k-event back-scroll staying smooth, and offline/reconnect mid-pagination — was not exercised here (see Manual checks). Per-item `read_receipts`, the typing broadcast, and `live_back_pagination_status` producers are verified via pure helpers, VM serde tests, the crate-wide guard, and full frontend behavior tests rather than a constructed live `Timeline`/`EventTimelineItem` (the same `pub(super)`/no-lightweight-constructor limit prior stories hit). Reader micro-avatars are initials-only (no profile avatars — future); read state is in-timeline only (no unread badges — Epic 4); receipts/typing are public (Incognito policy — Epic 8).
