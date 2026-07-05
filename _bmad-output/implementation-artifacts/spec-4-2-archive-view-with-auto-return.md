---
title: 'Archive View with Auto-Return'
type: 'feature'
created: '2026-07-04'
status: 'done'
baseline_revision: '824e4c7fbbd22afd56a28ae131aa33efe6c44c2e'
final_revision: 'de86515161af50a71b4fcbe121df7f2769a4318d'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** The Unified Inbox has no Archive. A user cannot move a quiet chat out of the inbox and trust it to resurface on new activity, so inbox-zero triage is unsafe (archiving = risk of missing a reply). FR-20 (Archive with auto-return) is unbuilt.

**Approach:** Source per-room archive state in Rust from the Matrix low-priority tag (`m.lowpriority`) as a new `is_archived` field on the streamed VMs. Partition the already-merged, recency-ordered inbox in the Rust merger into two windows over one subscription — **Inbox** (`!is_archived || is_unread`) and **Archive** (`is_archived && !is_unread`) — so an archived chat auto-returns to the inbox purely as a view rule the instant it becomes unread, with no tag mutation on activity. Add `archive_room` / `unarchive_room` commands that toggle the tag best-effort, a second always-on archive mirror store, and a sidebar Archive view the user switches to.

## Boundaries & Constraints

**Always:**
- Archive partitioning, ordering, and the inbox/archive split are computed in Rust and streamed; TypeScript only applies diff batches and renders — it never filters, sections, or re-sorts (AD-20). The two windows are two `InboxBatch` streams from one subscription/merger.
- Archive state is the Matrix low-priority tag: source via `RoomListItem`→`Room::is_low_priority()`; mutate via `Room::set_is_low_priority(bool, None)`. This persists across relaunch and syncs to other Matrix clients for free.
- Auto-return is a **view rule only** (`!is_archived || is_unread` includes an archived-unread room in the inbox partition); new activity re-emits a room-list diff (notable update), so the merger re-partitions live. Reading the returned chat lets it settle back into Archive.
- Archive/unarchive are best-effort like existing mutations (`mark_room_unread`): resolve the live `Room` via `room_for`, dispatch, log-and-swallow failures (`Ok`), never a UI error. They live in `account.rs` (tags are account-data, not receipts — outside the AD-14 signals seam).
- Selection stays single-source in `roomsStore`; opening an archived row records the same `{accountId, roomId}` and streams normally.

**Block If:**
- matrix-sdk 0.18 does not expose `Room::is_low_priority()` / `set_is_low_priority(bool, Option<f64>)` as assumed (verified present in planning: `matrix-sdk-base` `room/tags.rs`, `matrix-sdk` `room/mod.rs` ~L1767 — block only if the build disproves it).

**Never:**
- No clearing/mutating the tag on new activity (auto-return must not permanently unarchive; that is what the explicit "Unarchive" action does).
- No TS-side partitioning of a single combined stream into inbox vs archive (violates AD-20) and no optimistic within-one-frame overlay for archive (row membership between windows is exactly the Rust-authoritative filtering; a sub-second round-trip before the row moves is accepted).
- No `⌘1`/`⌘2` keyboard shortcuts (AC defers them: "sidebar entry, later ⌘2") and no single-key `e` verb (Epic 9); sidebar entry + row context menu only.
- No Pins/Favorites/Spaces/Network-filter logic (Stories 4.3–4.6). No favourite-tag handling (only low-priority).
- No new source-of-truth store; the archive mirror is a pure streamed window like `roomsStore`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Non-archived chat | `is_archived == false` | In Inbox window at its recency position; absent from Archive | n/a |
| Archived + read | `is_archived == true`, `is_unread == false` | In Archive window; absent from Inbox | n/a |
| Auto-return (archived + unread) | `is_archived == true`, `is_unread == true` (new message / mention / manual unread) | In Inbox window at chronological position; absent from Archive | n/a |
| Archive action | user picks "Archive" on an Inbox row | `set_is_low_priority(true)` dispatched; on the room-list diff the row leaves Inbox and enters Archive | Dispatch failure logged/swallowed; window unchanged |
| Unarchive action | user picks "Unarchive" on an Archive row | `set_is_low_priority(false)`; row returns to its chronological Inbox position | Dispatch failure logged/swallowed |
| Switch to Archive view, empty | no archived-read chats | Archive pane shows "Nothing archived. `E` archives a chat and keeps it searchable." (UX-DR13) | n/a |
| Sign-out / re-subscribe | inbox subscription torn down | Both Inbox and Archive mirrors cleared; one `unsubscribeInbox` covers both channels | n/a |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- `RoomVm` (~L320) + `InboxRoomVm` (~L901): add `is_archived: bool` (`#[ts(export)]` regenerates TS); update sample builders.
- `src-tauri/crates/keeper-core/src/account.rs` -- `room_item_to_vm` (~L2402): source `is_archived` from `item.is_low_priority()`. Add `archive_room` / `unarchive_room` (mirror `mark_room_unread` ~L1547 via `room_for` → `set_is_low_priority`).
- `src-tauri/crates/keeper-core/src/inbox.rs` -- `InboxMerger` (L51): hold an inbox sink **and** an archive sink; `emit` (L118) partitions the merged Vec and emits a `Reset` to each; `to_inbox_room` (L185) carries `is_archived`; update fixtures/tests.
- `src-tauri/crates/keeper/src/ipc.rs` -- `inbox_subscribe` (L1250): add a second `Channel<InboxBatch>` for archive; add `archive_room` / `unarchive_room` commands (mirror `mark_room_unread` ~L1105, `to_ipc_error`).
- `src-tauri/crates/keeper/src/lib.rs` -- `generate_handler!` (~L47): register `archive_room`, `unarchive_room`.
- `src-tauri/crates/keeper-core/src/account.rs` (`subscribe_inbox` L308) & inbox producer wiring -- thread the archive sink through `InboxMerger::new`.
- `src/lib/ipc/client.ts` -- `subscribeInbox` (L263): take `(onInbox, onArchive)`, create two channels; add `archiveRoom` / `unarchiveRoom` wrappers.
- `src/lib/stores/archive-rooms.ts` -- NEW: slim mirror store (`rooms`, `total`, `applyBatch`, `clear`) reusing `applyDiffOp`; no unread overlay (archive rows are read), no selection.
- `src/lib/stores/primary-view.ts` -- NEW: vanilla store `view: "inbox" | "archive"` + `setView` (pattern of `settings-ui.ts`).
- `src/components/layout/chat-list-pane.tsx` -- subscribe both channels; render inbox vs archive rows and per-view empty state by `primaryView`; account filter applies to both.
- `src/components/layout/sidebar-pane.tsx` -- add "Archive" entry; wire "Chats"(=Inbox) + "Archive" to `setView`; mark the active view.
- `src/components/chat/chat-row.tsx` -- add "Archive"/"Unarchive" context-menu item gated on `room.isArchived`, calling the new best-effort commands.
- Tests: `inbox.rs` (partition), `chat-row.test.tsx`, `chat-list-pane.test.tsx`, `sidebar-pane.test.tsx`, `archive-rooms.test.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `is_archived: bool` to `RoomVm` and `InboxRoomVm` with doc comments; update every sample/test builder. -- carry authoritative archive state on the streamed VMs.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- in `room_item_to_vm`, set `is_archived = item.is_low_priority()`. Add `archive_room(account_id, room_id)` / `unarchive_room(...)` resolving via `room_for` and calling `room.set_is_low_priority(true|false, None)` best-effort (log-and-swallow, return `Ok`). -- source archive state; make archive/unarchive round-trip.
- [x] `src-tauri/crates/keeper-core/src/inbox.rs` -- give `InboxMerger` an archive sink alongside the inbox sink; in `emit`, `merge` then `partition` into inbox (`!is_archived || is_unread`) and archive (`is_archived && !is_unread`), preserving recency order, and emit a `Reset` batch (total = partition len) to each sink; thread `is_archived` through `to_inbox_room`; close on either sink closing. -- compute both windows in Rust from one merge.
- [x] `src-tauri/crates/keeper-core/src/account.rs` (`subscribe_inbox`) -- construct the merger with both sinks (inbox + archive). -- wire the second stream.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- extend `inbox_subscribe` with an `archive` `Channel<InboxBatch>` (wrap both channels into sinks); add `#[tauri::command] archive_room` / `unarchive_room` mapping via `to_ipc_error`. -- expose the archive stream + mutations.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register `archive_room`, `unarchive_room` in `generate_handler!`. -- wire IPC.
- [x] `src/lib/ipc/client.ts` -- `subscribeInbox(onInbox, onArchive)` creating two channels for `inbox_subscribe`; add `archiveRoom` / `unarchiveRoom`. -- typed access to both streams + commands.
- [x] `src/lib/stores/archive-rooms.ts` -- NEW slim mirror (`archiveRoomsStore` + `useArchiveRoomsStore`): `applyBatch` via `applyDiffOp`, `clear`; no overlay/selection. -- mirror the archive window.
- [x] `src/lib/stores/primary-view.ts` -- NEW `primaryViewStore` (`view`, `setView`) + hook. -- drive which pane the user sees.
- [x] `src/components/layout/chat-list-pane.tsx` -- subscribe both channels into `roomsStore`/`archiveRoomsStore`; select rows + empty state by `primaryView` ("No conversations yet." vs the UX-DR13 archive empty text); apply the account filter to whichever window is active. -- render the active view.
- [x] `src/components/layout/sidebar-pane.tsx` -- add an "Archive" entry; make "Chats" and "Archive" call `setView("inbox"|"archive")` and reflect the active view (e.g. `aria-current` + accent). -- provide the sidebar switch.
- [x] `src/components/chat/chat-row.tsx` -- add a context-menu item: "Archive" when `!room.isArchived` (calls `archiveRoom`), "Unarchive" when `room.isArchived` (calls `unarchiveRoom`), best-effort `.catch` swallow. -- per-row archive control.
- [x] Tests -- `inbox.rs`: partition places non-archived + archived-unread in inbox and archived-read in archive, recency preserved, both sinks emit. `chat-row.test.tsx`: Archive vs Unarchive item by `isArchived` + invoke. `archive-rooms.test.ts`: applyBatch/clear. `chat-list-pane.test.tsx`: both channels feed their stores; switching view renders archive rows + archive empty text. `sidebar-pane.test.tsx`: Archive entry switches the view. -- cover behavior.

**Acceptance Criteria:**
- Given a chat in the Unified Inbox, when the user archives it via the row context menu, then it leaves the Inbox and appears in the Archive view (reachable from the sidebar), and unarchiving returns it to its correct chronological Inbox position — the inbox/archive split being computed in Rust and streamed, never derived in TypeScript (FR-20, AD-20).
- Given an archived chat, when a new message (or mention, or manual mark-unread) makes it unread, then it automatically returns to the Unified Inbox at its chronological position without any tag mutation, and settles back into Archive once read.
- Given a relaunch or another Matrix client, then archive state persists and syncs via the low-priority tag, and the empty Archive view shows "Nothing archived. `E` archives a chat and keeps it searchable." (UX-DR13).
- Given a code audit, then `.mark_as_read(` / `.send_single_receipt(` remain only in `signals.rs` (guard test green — archive uses tag account-data, not the receipt seam) and no inbox/archive filtering is done in TypeScript.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 1, low 1)
- defer: 0
- reject: 13: (high 0, medium 0, low 13)
- addressed_findings:
  - `[medium]` `[patch]` A single shared `loaded` flag dismissed the skeleton when *either* window's channel delivered, so switching to Archive before its channel emitted could flash "Nothing archived." prematurely — split into per-window `loadedInbox`/`loadedArchive`, gating each view's skeleton on its own stream (`chat-list-pane.tsx`), with a new regression test.
  - `[low]` `[patch]` The inherited `total` doc comments (`vm.rs` `InboxBatch`, `rooms.ts`, `archive-rooms.ts`) still claimed a cross-account server aggregate, but Story 4.2 redefined `total` as the per-window partition length — corrected the comments (regenerated `InboxBatch.ts`).
  - Rejected (13, all noise or by-design): cross-window selection "stranding" (selection is single-source by spec — the open chat stays readable across the move); the `E` empty-state verb (prescribed verbatim by UX-DR13 as a forward-reference to the Epic 9 single-key verb); two-sink non-atomicity + shared-close + one-frame flicker (inherent to the specced two-channel design; only one window renders at a time, no cross-store consumer); `debug!`-level best-effort logging (matches the crate-wide convention used by `mark_room_read`/`mark_room_unread`/typing); `room_for` requiring a live account (every displayed row is from an activated account; identical to 4.1's `mark_room_unread`); account-filter + empty-state copy (identical pre-existing inbox pattern); archiving an unread room being a no-op until read (the auto-return rule, by design); no toggle debounce (best-effort, last-write-wins); archive stalling forever (both windows are co-emitted from one `emit`, so the archive channel cannot stall independently).

## Design Notes

**SDK surface (matrix-sdk 0.18, verified):** `RoomListItem` derefs to `Room`; `Room::is_low_priority()` reads the `m.lowpriority` notable tag from cached `RoomInfo` (no await). `Room::set_is_low_priority(is_low_priority, tag_order)` sets/removes the tag (and clears `m.favourite` when setting) and sends to the server. A tag change flows back through sync as an `AnyRoomAccountDataEvent::Tag`, which updates `notable_tags` and — via `broadcast_room_info_notable_updates` (empty reasons → `NONE`, and `NONE != RECENCY_STAMP`) — makes the room-list `entries_with_dynamic_adapters` stream emit a `VectorDiff::Set` for that room. So both the archive toggle and auto-return propagate live through the **existing** merged stream; no polling, no extra subscription.

**Why partition, not two subscriptions:** the merger already holds every account's rooms and computes one sorted `Vec<InboxRoomVm>` per emit. Partitioning that Vec into two `Reset` batches (one merge, two sinks, one set of producers) is cheaper than a second per-account producer set and keeps a single recency ordering authoritative. Each window is a normal `InboxBatch`, so the frontend reuses `applyDiffOp` and the existing `InboxOp`/`InboxBatch` types — no new generated types.

**Auto-return golden case:**
```
merged (recency): [A !archived unread, B archived read, C archived unread, D !archived read]
inbox   = [A, C, D]   // !is_archived || is_unread   → C auto-returned
archive = [B]         // is_archived && !is_unread
```

**No optimistic overlay (unlike 4.1 unread):** which window a row belongs to is Rust-authoritative filtering that AD-20 protects, and a TS overlay that hid/moved a row would be a source-of-truth for filtering. The archive action's visible move waits on the tag round-trip (sub-second on a healthy link); the AC bounds correctness, not latency.

## Verification

**Commands:**
- `bun run test:rust` -- cargo-nextest green; regenerated `RoomVm.ts`/`InboxRoomVm.ts` now include `isArchived`.
- `bun run bindings:check` -- no uncommitted drift under `src/lib/ipc/gen`.
- `bun run check:rust` -- rustfmt + clippy `-D warnings`; AD-14 guard `signals_is_the_sole_receipt_typing_gate` green (archive touches no receipt/typing API).
- `bun run check` -- biome + tsc + vitest pass, including new chat-row, chat-list-pane, sidebar, and archive-store tests.

## Auto Run Result

Status: done

**Summary:** Implemented Story 4.2 Archive View with Auto-Return. Per-room archive state is sourced in Rust from the Matrix low-priority tag (`m.lowpriority`) via `RoomListItem`→`Room::is_low_priority()`, carried on `RoomVm`/`InboxRoomVm` as `isArchived`. The `InboxMerger` now partitions its single recency-ordered merge into two windows streamed over one subscription (two Tauri channels): **Inbox** (`!is_archived || is_unread`) and **Archive** (`is_archived && !is_unread`). Auto-return is a pure view rule — an archived chat becomes unread on new activity (or manual mark-unread) and re-partitions into the Inbox with no tag mutation, settling back into Archive once read. `archive_room`/`unarchive_room` best-effort toggle the tag (`set_is_low_priority`), reflected back through the stream. The frontend adds a slim `archiveRoomsStore` mirror, a `primaryViewStore`, a sidebar "Archive" entry, a per-row Archive/Unarchive context-menu item, and per-view rendering with the UX-DR13 empty state. All filtering/ordering stays Rust-authoritative (AD-20); tags are account-data, so the AD-14 receipt/typing seam is untouched.

**Files changed (code):**
- `src-tauri/crates/keeper-core/src/vm.rs` — `is_archived` on `RoomVm`/`InboxRoomVm`; `InboxBatch.total` doc corrected to per-window.
- `src-tauri/crates/keeper-core/src/account.rs` — `room_item_to_vm` sources `is_archived`; `archive_room`/`unarchive_room` (mirror `mark_room_unread`, best-effort); `subscribe_inbox` wires both sinks.
- `src-tauri/crates/keeper-core/src/inbox.rs` — two-sink `InboxMerger`; `emit` partitions the merge; `to_inbox_room` carries `is_archived`; removed now-dead `total_across`/`AccountSlot.total`; partition golden test.
- `src-tauri/crates/keeper/src/ipc.rs`, `lib.rs` — `inbox_subscribe` second channel; `archive_room`/`unarchive_room` commands + registration.
- `src/lib/ipc/client.ts` — `subscribeInbox(onInbox, onArchive)`; `archiveRoom`/`unarchiveRoom`.
- `src/lib/stores/archive-rooms.ts` (new), `src/lib/stores/primary-view.ts` (new).
- `src/components/layout/chat-list-pane.tsx` — one subscription → two stores; per-view render + per-window `loaded`.
- `src/components/layout/sidebar-pane.tsx` — Archive entry + view switch + active-view marking.
- `src/components/chat/chat-row.tsx` — Archive/Unarchive context-menu item.
- `src/lib/ipc/gen/{RoomVm,InboxRoomVm,InboxBatch}.ts` — regenerated.
- Tests: `inbox.rs` partition, `chat-row.test.tsx`, `chat-list-pane.test.tsx` (+ per-window-loaded regression), `sidebar-pane.test.tsx`, `archive-rooms.test.ts`; fixtures in `rooms.test.ts`, `use-sign-out.test.ts`.

**Review findings:** 2 patches applied (1 medium: per-window `loaded` so the Archive view never flashes a premature empty-state; 1 low: corrected the `total` doc comments to the new per-window meaning) — both with test coverage. 0 deferred. 13 rejected as noise or by-design (cross-window selection is single-source by spec; the `E` empty-state verb is prescribed by UX-DR13; two-sink non-atomicity/one-frame flicker is inherent to the specced design with no cross-store consumer; `debug!` logging matches the crate convention; `room_for` live-account requirement mirrors 4.1).

**Verification:** `bun run check` (biome + tsc + 463 vitest) — PASS; `bun run check:rust` (rustfmt + clippy `-D warnings`, AD-14 guard green) — PASS; `bun run test:rust` (296 cargo-nextest; bindings regenerated) — PASS. `bindings:check`'s git-clean clause is satisfied once the regenerated gen files are committed (done in this run's commit).

**Residual risks:** Archive/unarchive have no optimistic overlay (by design, unlike 4.1's unread): the row moves only when the tag's account-data echo re-emits a room-list diff, so the visible move waits on a server round-trip (sub-second on a healthy link) and a genuinely-failed tag write is a silent no-op logged only at `debug!` — consistent with every other best-effort mutation in the crate. Auto-return correctness after real-homeserver convergence is asserted by the partition design and unit-tested on the merge, but not covered by an integration test against a live sync.
</content>
</invoke>
