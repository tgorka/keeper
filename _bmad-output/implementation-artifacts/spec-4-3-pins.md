---
title: 'Pins'
type: 'feature'
created: '2026-07-04'
status: 'done'
baseline_revision: '0d7110335b3d93df1d49360302997df5201659d1'
final_revision: '189ade472a6018d5f8e22384b9e08c69f9f723c2'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** The Unified Inbox has no Pins. A user cannot hoist their most important chats out of the recency-ordered flow into a stable, self-ordered strip at the top, so high-value conversations sink as unrelated chats get newer activity. FR-22 / UX-DR4 (Pins strip) is unbuilt.

**Approach:** Add a third inbox window — **Pins** — to the Rust `InboxMerger` alongside Inbox and Archive (Story 4.2's two-window partition). Pin membership + user-controlled order are **local** keeper state (SQLite `keeper.db` via `registry.rs`), because Matrix has no standard *notable* tag for pins — a custom account-data tag would not update `RoomNotableTags` and so would never re-emit the room-list stream live (verified in matrix-sdk-base 0.18 `handle_notable_tags`). Pin mutations persist to the registry and poke an out-of-band `emit()` so the strip updates within one frame. The frontend mirrors the pins window into a slim store and renders a horizontal strip of 44 px circular avatars with native drag-to-reorder and a per-row Pin/Unpin context-menu item.

## Boundaries & Constraints

**Always:** Pins membership, ordering, sectioning, and the three-way partition are computed in Rust and streamed as an authoritative windowed `InboxBatch` (AD-20) — TS never derives, sorts, or filters pin state. Pinned rooms are removed from the Inbox and Archive windows (they live only in the Pins window) and stay pinned regardless of newer activity. Pin state persists across restart. Mutations (`pin_room`, `unpin_room`, `reorder_pins`) are `domain_verb` snake_case Tauri commands that reflect back through the pins stream (AD-8); the frontend uses the single `inbox`-family mirror-store pattern (AD-9) applying diff batches via `applyDiffOp`. Follow Story 4.2's two-channel `inbox_subscribe` shape, extended to a third `Channel<InboxBatch>`.

**Block If:** (none expected — this is an additive third window over an established pattern with local persistence; no external decision is required.)

**Never:** No `m.favourite`/`m.lowpriority` or any Matrix tag for pins (favourite belongs to Story 4.4; low-priority to 4.2). No cross-client sync of pins (pins have no server representation; "local where not representable" per epic context). No optimistic TS overlay that hides/moves/re-sorts a row (ordering is Rust-authoritative). No unread badge, label, or sidebar entry for the strip (it lives inside the Inbox pane). No sidebar/primary-view changes. No single-key `p` verb (deferred to Epic 9 — context-menu only).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Pin a chat | inbox row, `pin_room(acct,room)` | row leaves Inbox window, appears in Pins window as a 44 px circular avatar (network badge) at the strip end; persisted with next order | best-effort; registry write error → `Err` mapped by `to_ipc_error`, no partial state |
| Unpin a chat | pins avatar, `unpin_room(acct,room)` | row leaves Pins window, returns to its correct chronological Inbox (or Archive) position | best-effort; on error the pin remains |
| Reorder pins | `reorder_pins([{acct,room}…])` in new order | Pins window re-emits in the given order; order persisted (contiguous) and survives restart | invalid/missing ref rows are skipped; order stays consistent |
| Overflow | 9+ pins | strip scrolls horizontally (no wrap, no growth); ~8 fit before scroll | n/a |
| Newer activity elsewhere | unpinned chat gets a message | pinned chats keep position/order; only Inbox window re-orders | n/a |
| Pinned + archived/unread | pinned room becomes archived or unread | stays in Pins window (pins win over archive/unread); not duplicated into Inbox/Archive | n/a |
| Relaunch | pins persisted in `keeper.db` | Pins window restores with same membership + order on next `inbox_subscribe` | missing/empty table → empty strip (hidden) |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/registry.rs` -- schema init (`accounts`/`settings` ~L44-63) + `get_setting`/`set_setting` (~L74-102): add a `pins(account_id, room_id, sort_order INTEGER, PRIMARY KEY(account_id,room_id))` table to the schema, plus `set_pin(data_dir, account_id, room_id, order: i64)` (upsert), `remove_pin(data_dir, account_id, room_id)`, `get_pins(data_dir) -> Vec<(String,String,i64)>` (ordered by `sort_order` asc). Also drop a room's pin in `delete_account` cleanup path if one exists.
- `src-tauri/crates/keeper-core/src/inbox.rs` -- `InboxMerger`/`MergeState` (L49-70): add `pins_sink: InboxSink` and `pins: HashMap<(String,String), i64>`; `new(inbox_sink, archive_sink, pins_sink, pins)`. `emit` (L124-154): after `merge`, look up each item's `(account_id, room_id)` in `pins` → build the **Pins** window (pinned only, stable-sorted by order asc), and partition the rest: Inbox `!pinned && (!is_archived || is_unread)`, Archive `!pinned && is_archived && !is_unread`; Reset each of the three sinks (per-window `total`). Add `update_pins(&self, HashMap<(String,String),i64>)` that replaces the map and calls `emit` (out-of-band poke, mirrors `remove_account`'s direct-`emit` at L96-101). `to_inbox_room` (L188): carry `is_pinned`. Extend test helpers (`capturing_merger` L414 → three captures).
- `src-tauri/crates/keeper-core/src/vm.rs` -- `InboxRoomVm` (L895-938): add `is_pinned: bool` (`#[ts(export)]` regenerates `InboxRoomVm.ts`); update sample builders. `RoomVm` unchanged (pin state is merger-side, not SDK-sourced).
- `src-tauri/crates/keeper-core/src/account.rs` -- `subscribe_inbox` (L310): accept `pins_sink`; load initial pins via `registry::get_pins` into the merger. `AccountManager` (L170) + `InboxHandle` (L159): add `pin_room`/`unpin_room`/`reorder_pins(data_dir, …)` that mutate the registry then reload `get_pins` and call `handle.merger.update_pins(map)` under the `inbox` lock (no-op re-emit if no active subscription). `pin_room` order = `max(existing)+1`; `reorder_pins` reassigns contiguous `0..n`.
- `src-tauri/crates/keeper/src/ipc.rs` -- `inbox_subscribe` (~L1292): add a third `Channel<InboxBatch>` (pins) wrapped into a sink. Add `#[tauri::command] pin_room` / `unpin_room` / `reorder_pins` resolving `data_dir` via `state.platform.data_dir()` (mirror `archive_room` ~L1125, `to_ipc_error`). `reorder_pins` arg: `order: Vec<PinRef>` where `#[derive(Deserialize)] #[serde(rename_all="camelCase")] struct PinRef { account_id, room_id }`.
- `src-tauri/crates/keeper/src/lib.rs` -- `generate_handler!` (~L47): register `pin_room`, `unpin_room`, `reorder_pins`.
- `src/lib/ipc/client.ts` -- `subscribeInbox` (L269): take `(onInbox, onArchive, onPins)`, create a third channel. Add `pinRoom` / `unpinRoom` / `reorderPins(order)` wrappers (best-effort, swallow rejection) mirroring `archiveRoom` (L747).
- `src/lib/stores/pins-rooms.ts` -- NEW: slim mirror (`pinsRoomsStore` + `usePinsRoomsStore`) — `rooms`, `total`, `applyBatch` via `applyDiffOp`, `clear` (copy of `archive-rooms.ts`).
- `src/components/layout/chat-list-pane.tsx` -- subscribe the third channel into `pinsRoomsStore`; per-window `loadedPins`; render `<PinsStrip>` above the `<ScrollArea>` only in the inbox view; account filter applies to the pins window too.
- `src/components/layout/pins-strip.tsx` -- NEW: horizontal `overflow-x-auto`, no-wrap flex of `<RoomAvatar size="xl">` (44 px); native HTML5 drag reorder (`draggable`, `onDragStart/Over/Drop`, ephemeral local drag index cleared on drop) dispatching `reorderPins`; click selects the room; per-avatar context menu with Unpin. Hidden entirely when empty.
- `src/components/ui/avatar.tsx` -- add an `"xl"` size (`data-[size=xl]:size-11` = 44 px) and its badge scale (`AvatarBadge` L54-68).
- `src/components/chat/RoomAvatar.tsx` -- NEW: extract room→avatar (image/initials fallback + network `AvatarBadge`) so the strip and `chat-row` share one source; used at `size="lg"` in the row and `size="xl"` in the strip.
- `src/components/chat/chat-row.tsx` -- use `RoomAvatar`; add a Pin/Unpin context-menu item gated on `room.isPinned`, calling `pinRoom`/`unpinRoom` best-effort.
- Tests: `registry.rs` (pins CRUD/order), `inbox.rs` (three-window partition + `update_pins` re-emit), `pins-rooms.test.ts`, `pins-strip.test.tsx`, `chat-row.test.tsx`, `chat-list-pane.test.tsx`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- add `pins` table to schema init + `set_pin`/`remove_pin`/`get_pins`; unit-test CRUD + ordering + upsert. -- durable local pin membership/order.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `is_pinned: bool` to `InboxRoomVm` with a doc comment; update every sample/test builder; regenerate `InboxRoomVm.ts`. -- carry pin state on the streamed VM for context-menu gating.
- [x] `src-tauri/crates/keeper-core/src/inbox.rs` -- give `InboxMerger` a `pins_sink` + `pins` map; in `emit`, build the Pins window (pinned, order asc) and partition the remainder into Inbox/Archive excluding pinned, Reset all three sinks; add `update_pins` (poke `emit`); carry `is_pinned` through `to_inbox_room`; extend fixtures to three sinks + the golden three-window test. -- compute all three windows from one merge.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- `subscribe_inbox` accepts `pins_sink` and seeds the merger from `registry::get_pins`; add `pin_room`/`unpin_room`/`reorder_pins` (registry write → reload → `merger.update_pins`), append-at-end / contiguous-reorder semantics. -- wire the third stream + mutations with live re-emit.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- extend `inbox_subscribe` with a third `Channel<InboxBatch>`; add `pin_room`/`unpin_room`/`reorder_pins` commands (`PinRef` arg, `state.platform.data_dir()`, `to_ipc_error`). -- expose stream + mutations.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register `pin_room`, `unpin_room`, `reorder_pins` in `generate_handler!`. -- wire IPC.
- [x] `src/lib/ipc/client.ts` -- `subscribeInbox(onInbox, onArchive, onPins)`; add `pinRoom`/`unpinRoom`/`reorderPins`. -- typed access to the third stream + commands.
- [x] `src/lib/stores/pins-rooms.ts` -- NEW slim mirror via `applyDiffOp`. -- mirror the pins window.
- [x] `src/components/ui/avatar.tsx` -- add `"xl"` (44 px) size + badge scale. -- strip avatar size.
- [x] `src/components/chat/RoomAvatar.tsx` -- NEW shared room→avatar; adopt in `chat-row.tsx`. -- one avatar source for row + strip.
- [x] `src/components/layout/pins-strip.tsx` -- NEW horizontal strip: 44 px circular avatars, native drag reorder → `reorderPins`, click-to-select, Unpin context menu, hidden when empty, overflow scrolls horizontally. -- render + reorder pins.
- [x] `src/components/layout/chat-list-pane.tsx` -- subscribe the third channel → `pinsRoomsStore`, per-window `loadedPins`, render `<PinsStrip>` atop the inbox view. -- surface the strip.
- [x] `src/components/chat/chat-row.tsx` -- Pin/Unpin context-menu item gated on `room.isPinned` (best-effort). -- per-row pin control.
- [x] Tests -- `inbox.rs`: pinned rooms populate the Pins window in order, are excluded from Inbox/Archive, and stay on newer activity; `update_pins` re-emits all three. `pins-rooms.test.ts`: applyBatch/clear. `pins-strip.test.tsx`: renders in stream order, drag dispatches `reorderPins`, Unpin invokes `unpinRoom`. `chat-row.test.tsx`: Pin vs Unpin by `isPinned` + invoke. `chat-list-pane.test.tsx`: third channel feeds the store; strip renders in inbox view and hides when empty. -- cover behavior.

**Acceptance Criteria:**
- Given a chat in the Unified Inbox, when the user pins it via the row context menu, then it leaves the chronological flow and renders as a 44 px circular avatar (network badge overlaid) in the Pins strip at the top of the chat list; unpinning returns it to its correct chronological position — the three-way split computed in Rust and streamed, never derived in TypeScript (FR-22, UX-DR4, AD-20).
- Given multiple pinned chats, when the user drags a pin to a new position, then the order updates and persists across a relaunch, and 9+ pins overflow into a horizontal scroll (no wrap/growth).
- Given new activity in unpinned chats, then pinned chats keep their position and order irrespective of that activity, and a pinned chat that becomes unread or archived remains in the Pins window (not duplicated into Inbox or Archive).
- Given a code audit, then pin state uses local `registry` persistence (no Matrix tag), `.mark_as_read(`/`.send_single_receipt(` remain only in `signals.rs` (AD-14 guard green), and no inbox/pins ordering or filtering is done in TypeScript.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 2, low 2)
- defer: 3: (high 0, medium 1, low 2)
- reject: 10: (high 0, medium 0, low 10)
- addressed_findings:
  - `[medium]` `[patch]` Reorder while an account-switcher filter was active submitted only the *filtered* subset of pins; Rust rewrote those to contiguous `0..n`, colliding with the hidden pins' orders and corrupting the persisted global order. Disabled drag-reorder while filtered (`reorderable={filterAccountId === null}` from the pane; `draggable`/`onDragStart`/`onDrop` gated in `pins-strip.tsx`) so reorder only runs over the complete set; added a regression test.
  - `[medium]` `[patch]` The shared `RoomAvatar` unconditionally rendered an always-on `AvatarBadge` (solid accent dot) on *every* chat row — a visual regression on the existing inbox/archive and premature Story 4.6 scope (no Network identity exists yet). Removed the placeholder badge from `RoomAvatar`; the real Network badge lands with FR-24 attribution in Story 4.6. (Also resolves the test-only `data-testid` leaking into production DOM.)
  - `[low]` `[patch]` `pins-strip.tsx` `onDrop` spliced with the drag-start index against the current `pins` prop; a stream `Reset` shrinking/replacing the window mid-drag could splice an `undefined` element and throw on `accountId`. Added bounds + `moved !== undefined` guards; added a stale-index regression test.
  - `[low]` `[patch]` The pins `emit` sort keyed only on `sort_order`, so a transient order collision let recency (via stable sort) flip the strip order across re-emits. Added a deterministic `(sort_order, account_id, room_id)` tie-break in `inbox.rs`; added a unit test asserting order stability across recency changes.
  - Deferred (3): a pinned room absent from the live SlidingSync window is silently excluded from the Pins strip (architectural — shared with the Inbox/Archive windowed merge, not unique to pins); `reorder_pins` performs N separate non-transactional registry writes (partial-failure could leave a half-rewritten order — low probability); no keyboard-accessible reorder alternative to native HTML5 drag (a11y enhancement, context-menu Pin/Unpin remains keyboard-operable).
  - Rejected (10, noise or by-design): pins mirror not reset in `use-sign-out` (consistent with `archiveRoomsStore`, which also isn't; the pane clears all inbox mirrors on (re)subscribe and the login screen unmounts the pane); `open()`-per-`set_pin` re-running `CREATE TABLE IF NOT EXISTS` (matches the crate-wide per-call connection pattern); `reload_pins` holding the `inbox` lock while awaiting the merger mutex (no cycle — the producer never re-takes `inbox`, so no current deadlock); `pin_order` cloning the key on the merge hot path (micro-allocation, matches existing merge clones); phantom pins from non-pinned refs in a reorder payload (subsumed by the filter fix — the frontend only ever sends current pins; a race self-corrects on the next re-emit); the shared `closed` latch across three channels (by-design and pre-existing from 4.2's two channels; channels share one subscription lifecycle); `${accountId}:${roomId}` React key format (consistent composite-key convention, uniqueness holds); order-integer gaps after a reorder (benign for an ascending sort); duplicate rows on Append/Insert (pins emit only `Reset` by design); empty-slice reorder no-op (benign).

### 2026-07-04 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 0
- reject: 25: (high 0, medium 0, low 25)
- addressed_findings:
  - `[low]` `[patch]` The avatar-only Pins strip gave its avatar buttons an `aria-label` (screen-reader OK) but no `title`, so sighted users hovering an initials-only avatar (the common case until the media/mxc scheme-handler epic lands) got no tooltip to distinguish pins. Added `title={room.displayName}` to the strip button — a hover tooltip that directly serves the strip's recognize-a-pin affordance. TS-only; `bun run check` green (biome + tsc + 480 vitest + tauri-free guard).
- note: This was an independent follow-up pass (two fresh adversarial reviewers). ~90% of surfaced findings were re-raises already triaged in the first pass and confirmed correctly handled: the three HIGH classes were all already addressed — partial-order-under-filter corruption was *patched* (drag disabled while filtered), while non-atomic N-write reorder and "pinned room absent from the live sync window" were already *deferred* (present in the ledger; NOT re-deferred here to avoid duplicate entries). Rejected (25, already-handled / by-design / noise): concurrent `pin_room` `max()+1` collision (single-user UI-serialized; already mitigated by the deterministic `(sort_order, account_id, room_id)` tie-break added in pass 1); `reorder_pins` upserting non-pinned refs / orphan pins (subsumed by the filter fix — frontend only ever sends the complete current set); ghost/un-unpinnable pin for a left/forgotten room (same architectural class as the already-deferred absent-from-window limitation); non-transactional reorder (already deferred); `emit` key clone on the hot path (already rejected); same-length mid-drag stale-index reorder (drag gesture is sub-second, pins emit only `Reset`, self-corrects on next re-emit); no max-pin cap / `i64` overflow (unreachable; overflow scroll is by-design); fire-and-forget reorder with no failure toast (best-effort no-optimistic-overlay is spec-mandated); `RoomAvatar.tsx` PascalCase filename (spec-directed Code Map name; no lint gate enforces it); no dedicated core-level `reorder_pins` test (the `0..n` rewrite is repeated `registry::set_pin`, tested, plus `update_pins` re-emit, tested in `inbox.rs`); `roomInitials` surrogate-pair `slice(0,2)` (pre-existing logic extracted verbatim, cosmetic); shared three-channel `closed` latch, `visiblePins` per-render re-filter, `applyOp` wrapper, tie-break `i64::MAX` default, seeded-pins-on-subscribe test gap, empty/duplicate-ref reorder, `reorderable` mid-drag flip, drop-without-dragstart, filtered-account-signout stale store, `update_pins`-after-closed, `delete_account` order gaps, re-pin-already-pinned (UI never offers Pin on a pinned row) — all guarded, matching existing patterns, or of no user consequence.

## Design Notes

**Why local, not a Matrix tag:** matrix-sdk-base 0.18 `BaseRoomInfo::handle_notable_tags` only maps `m.favourite`→`FAVOURITE` and `m.lowpriority`→`LOW_PRIORITY` into `RoomNotableTags` (a 2-bit flag set). Only a *notable* tag change updates `RoomInfo` and drives a room-list `VectorDiff::Set`. A custom `u.keeper.pin` tag is not notable, so pin changes would never re-emit the merged stream live (they'd surface only after an unrelated event or restart). Since Matrix has no standard pin tag, per the epic's "persists locally where a state has no server representation," pins are keeper-local: registry-backed, merger-owned, and re-emitted by an explicit `update_pins` poke — the same direct-`emit` path `remove_account` already uses.

**Three-window partition (extends 4.2's two):**
```
merged (recency): [A pin(ord 1), B pin(ord 0), C archived read, D unread, E read]
pins    = [B, A]        // pinned, sorted by sort_order asc   (removed from below)
inbox   = [D, E]        // !pinned && (!archived || unread)
archive = [C]           // !pinned && archived && !read
```
Pins win over archive/unread, so a pinned row is never in two windows and never re-sorts on activity. Each window is a normal `InboxBatch`, reusing `applyDiffOp` and the existing `InboxOp`/`InboxBatch` types — the only new generated field is `InboxRoomVm.is_pinned`.

**Order model:** `sort_order: i64`, ascending. `pin_room` appends (`max+1`); `reorder_pins` rewrites the full ordered ref list to contiguous `0..n`. Order is global across accounts (the strip merges pinned rooms from all accounts), so both membership and order are reloaded from the registry and pushed into the merger as one map on every mutation — one code path keeps the in-memory map and disk exactly in sync.

**No optimistic overlay (like 4.2):** which window a row belongs to and its order are Rust-authoritative (AD-20). Drag shows only an ephemeral in-component preview during the gesture; on drop it dispatches `reorderPins` and the authoritative order arrives via the stream (sub-frame, local write). A failed mutation is a best-effort no-op logged at `debug!`, matching the crate convention.

## Verification

**Commands:**
- `bun run test:rust` -- cargo-nextest green; regenerated `InboxRoomVm.ts` includes `isPinned`; new `registry` pins + `inbox` three-window tests pass.
- `bun run bindings:check` -- no uncommitted drift under `src/lib/ipc/gen`.
- `bun run check:rust` -- rustfmt + clippy `-D warnings`; AD-14 guard `signals_is_the_sole_receipt_typing_gate` green (pins touch no receipt/typing API).
- `bun run check` -- biome + tsc + vitest pass, including new pins-strip, pins-store, chat-row, and chat-list-pane tests.

## Auto Run Result

Status: done

**Summary:** Implemented Story 4.3 Pins. Pins are keeper-**local** state (SQLite `keeper.db` via a new `pins(account_id, room_id, sort_order)` table in `registry.rs`) rather than a Matrix tag, because matrix-sdk-base 0.18 only treats `m.favourite`/`m.lowpriority` as *notable* — a custom pin tag would never re-emit the room-list stream live. The `InboxMerger` now partitions its single recency-ordered merge into **three** windows over one subscription (three Tauri channels): **Pins** (pinned, `sort_order` asc, tie-broken by account/room id), **Inbox** (`!pinned && (!archived || unread)`), and **Archive** (`!pinned && archived && !read`). Pins win over archive/unread, so a pinned room appears only in the strip and never re-sorts on new activity. `pin_room`/`unpin_room`/`reorder_pins` write the registry, reload the whole pin map, and poke an out-of-band `InboxMerger::update_pins` re-emit so the strip updates within one frame. The frontend adds a `pinsRoomsStore` mirror, a horizontal `PinsStrip` of 44 px circular avatars (native HTML5 drag reorder, click-to-select, per-avatar Unpin), a shared `RoomAvatar`, an `xl` avatar size, and a per-row Pin/Unpin context-menu item. All ordering/partitioning stays Rust-authoritative (AD-20); pins are account-data-free, so the AD-14 receipt/typing seam is untouched.

**Files changed (code):**
- `src-tauri/crates/keeper-core/src/registry.rs` — `pins` table + `set_pin`/`remove_pin`/`get_pins`; `delete_account` pin cleanup; CRUD/order tests.
- `src-tauri/crates/keeper-core/src/inbox.rs` — `pins_sink` + `pins` map on `InboxMerger`; three-window `emit` (pins win) with deterministic `(sort_order, account_id, room_id)` tie-break; `update_pins` poke; `is_pinned` stamping; three-sink fixtures + golden/tie-break/re-emit tests.
- `src-tauri/crates/keeper-core/src/vm.rs` — `is_pinned` on `InboxRoomVm`.
- `src-tauri/crates/keeper-core/src/account.rs` — `subscribe_inbox` seeds pins + takes `pins_sink`; `pin_room`/`unpin_room`/`reorder_pins` + `reload_pins`.
- `src-tauri/crates/keeper/src/ipc.rs`, `lib.rs` — third `inbox_subscribe` channel; `pin_room`/`unpin_room`/`reorder_pins` commands (`PinRef`) + registration.
- `src/lib/ipc/client.ts` — `subscribeInbox(onInbox, onArchive, onPins)`; `pinRoom`/`unpinRoom`/`reorderPins`.
- `src/lib/stores/pins-rooms.ts` (new) — slim pins mirror.
- `src/components/chat/RoomAvatar.tsx` (new) — shared room avatar (no placeholder badge — real Network badge lands in 4.6).
- `src/components/layout/pins-strip.tsx` (new) — the strip: drag reorder (disabled while filtered), Unpin menu, hidden when empty.
- `src/components/ui/avatar.tsx` — `xl` (44 px) size.
- `src/components/layout/chat-list-pane.tsx` — third channel → `pinsRoomsStore`; strip atop the inbox view; `reorderable` gated on the account filter.
- `src/components/chat/chat-row.tsx` — `RoomAvatar` + Pin/Unpin context-menu item.
- `src/lib/ipc/gen/InboxRoomVm.ts` — regenerated (`isPinned`).
- Tests: `registry.rs`, `inbox.rs` (three-window + pins-win + tie-break + re-emit), `pins-rooms.test.ts`, `pins-strip.test.tsx` (order/select/reorder/unpin + filtered-no-reorder + stale-drag), `chat-row.test.tsx`, `chat-list-pane.test.tsx`; fixtures in `rooms.test.ts`, `archive-rooms.test.ts`, `use-sign-out.test.ts`.

**Review findings:** 4 patches applied (2 medium: reorder-under-account-filter corrupted the persisted global pin order → drag disabled while filtered; the shared `RoomAvatar` painted an always-on placeholder badge on every chat row → removed, deferred to 4.6. 2 low: `onDrop` stale/out-of-bounds splice guard; deterministic pins-sort tie-break) — all with new test coverage. 3 deferred (pinned room absent from the live sync window is silently excluded — architectural; non-transactional N-write reorder; no keyboard reorder alternative). 10 rejected as noise or by-design (sign-out mirror-reset parity with archive; per-call registry connections; benign lock ordering; hot-path key clones; phantom-ref pins subsumed by the filter fix; shared three-channel close latch; composite React key; order-integer gaps; Reset-only pins de-dup; empty-reorder no-op).

**Verification:** `bun run check:rust` (rustfmt + clippy `-D warnings`, AD-14 guard green) — PASS; `bun run test:rust` (302 cargo-nextest; bindings regenerated) — PASS; `bun run check` (biome + tsc + 480 vitest, + tauri-not-in-core guard) — PASS. `bindings:check`'s git-clean clause is satisfied once the regenerated `InboxRoomVm.ts` is committed (done in this run's commit).

**Residual risks:** Pin mutations are best-effort with no optimistic overlay (like 4.2): a pin/unpin/reorder moves the row only when the merger re-emits (sub-frame on a local write; a genuinely-failed registry write surfaces as an `Err`, a failed re-emit is a silent no-op). Reorder is disabled while an account filter is active (a deliberate constraint, since a partial order would corrupt hidden pins) — reorder is a global-view operation. A pinned room not currently in the SlidingSync window is not shown until it syncs (deferred, architectural). Cross-client pin sync is intentionally absent (pins have no server representation).

**Follow-up review recommended:** false — the follow-up review pass (2026-07-04, two fresh adversarial reviewers) confirmed the first pass's triage held up and applied only a single localized low-consequence patch (a `title` tooltip on the avatar-only Pins strip so sighted users can distinguish initials-only pins on hover; screen-reader labels were already present). All three HIGH classes the reviewers re-raised were already handled — partial-order-under-filter corruption was patched in pass 1, and non-atomic reorder + absent-from-window pins were already deferred (in the ledger, not re-deferred). Everything else was already-handled, by-design, or noise. No further independent review is warranted.

### Follow-up review pass (2026-07-04)

An independent second review (Blind Hunter + Edge Case Hunter, fresh context) re-examined the full baseline→HEAD diff. Outcome: **1 patch, 0 defer, 0 bad_spec, 0 intent_gap, 25 reject.** The single patch adds `title={room.displayName}` to the Pins-strip avatar buttons (hover tooltip; the avatar-only strip already had `aria-label` for screen readers but nothing for sighted-user hover, and rooms render as initials until the mxc media-scheme-handler epic). `bun run check` green (biome + tsc + 480 vitest + tauri-free core guard). The ~90% overlap with pass 1's findings all reconciled to already-triaged: HIGHs already patched (filtered-reorder corruption) or already deferred (non-atomic reorder, absent-from-window pins — NOT re-deferred, per orchestrator ledger ownership); concurrency/ordering re-raises already mitigated by the pass-1 tie-break or subsumed by the filter fix; the rest by-design (best-effort no-overlay mutations, spec-directed `RoomAvatar.tsx` name) or of no user consequence.
