---
title: 'Unread Management'
type: 'feature'
created: '2026-07-04'
status: 'done'
baseline_revision: '2a4a7043449a2e48dfac19a40c514e1d5438a09b'
final_revision: 'f0bd2b2f6f3c9a3572fc7b7b26ceef0bf88d43b8'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** The Unified Inbox rows carry no unread state — every chat looks identical whether it has new messages, a mention, or nothing, and the user cannot manually mark a chat read or unread. The inbox can't function as a triage surface without honest unread signals.

**Approach:** Source per-room unread state (unread messages, unread mentions, manual unread flag) in Rust from the SDK room-list item, carry it through the inbox projection as new `InboxRoomVm` fields, and render it as bold name + mention badge / neutral dot. Add a chat-row context menu with "Mark read" / "Mark unread" that round-trips to the Matrix server (read receipt + `m.marked_unread`) with an optimistic within-one-frame local update.

## Boundaries & Constraints

**Always:**
- Unread state is computed in Rust and streamed on the view model; TypeScript renders it and never re-derives it from events (AD-20).
- Read-receipt emission stays behind the `signals` seam — `.mark_as_read(` / `.send_single_receipt(` may appear only in `signals.rs` (AD-14); the crate-wide guard test stays green.
- Use client-side counts (`num_unread_messages`, `num_unread_mentions`) — precise for E2EE where server counts are not.
- Mutations are best-effort like existing signals: a dispatch failure is logged and swallowed (`Ok`), never a UI error.
- Mark-read must work for any inbox row whether or not its timeline is open.

**Block If:**
- matrix-sdk 0.18 does not expose `Room::num_unread_messages` / `num_unread_mentions` / `is_marked_unread` / `set_unread_flag` as assumed (verified present in planning; block only if the build disproves it).

**Never:**
- No archive/pin/favorite/space/filter logic (Stories 4.2–4.6).
- No numeric bubble for non-mention unread — plain dot only (UX-DR3).
- No single-key `u` shortcut (Epic 9); context menu only.
- No new zustand store as a source of truth — only an ephemeral optimistic overlay reconciled against the Rust stream.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Unread, no mention | room `num_unread_messages > 0`, `num_unread_mentions == 0` | Row: name weight 600, neutral dot (`bg-muted-foreground`), no count | n/a |
| Unread mention | `num_unread_mentions > 0` | Row: name weight 600, filled primary `Badge` showing mention count | n/a |
| Manually unread | `is_marked_unread == true`, zero counts | Row treated as unread: name weight 600 + neutral dot | n/a |
| Read | zero counts, not marked | Name weight 400/normal, no badge, no dot | n/a |
| Mark read (menu) | user picks "Mark read" on an unread row | Row renders read within one frame (optimistic); read receipt advanced + `marked_unread` cleared server-side | Dispatch failure logged/swallowed; row reconciles to server truth on convergence |
| Mark unread (menu) | user picks "Mark unread" on a read row | Row renders unread (dot) within one frame; `m.marked_unread=true` set server-side | Dispatch failure logged/swallowed |
| Passive convergence | another client reads the room / new mention arrives | Row unread state updates from the streamed `Set` diff (READ_RECEIPT / UNREAD_MARKER notable update) | n/a |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- `RoomVm` (~L320) and `InboxRoomVm` (~L891) structs; add unread fields (`#[ts(export)]` regenerates TS).
- `src-tauri/crates/keeper-core/src/account.rs` -- `room_item_to_vm` (~L2354) sources unread from the SDK item; `mark_room_read` (~L1487) and `room_for` (~L1709) helpers; add `mark_room_unread`.
- `src-tauri/crates/keeper-core/src/inbox.rs` -- `to_inbox_room` (~L185) copies `RoomVm`→`InboxRoomVm`; thread new fields; test fixtures at L247+.
- `src-tauri/crates/keeper-core/src/signals.rs` -- AD-14 receipt/typing seam; sole home of `.mark_as_read(` / `.send_single_receipt(`.
- `src-tauri/crates/keeper/src/ipc.rs` -- `mark_room_read` command (~L1085); add `mark_room_unread`.
- `src-tauri/crates/keeper/src/lib.rs` -- `generate_handler!` list (~L47); register `mark_room_unread`.
- `src/lib/stores/rooms.ts` -- vanilla zustand inbox mirror; `applyBatch`; add optimistic-unread overlay + reconcile.
- `src/components/chat/chat-row.tsx` -- 64px row; render unread affordances + context menu.
- `src/lib/ipc/client.ts` -- typed invoke wrappers; add `markRoomRead`/`markRoomUnread`.
- `src/components/ui/context-menu.tsx`, `src/components/ui/badge.tsx` -- existing shadcn primitives to reuse.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `is_unread: bool` and `mention_count: u32` (`#[ts(type = "number")]`) to `RoomVm` and `InboxRoomVm`; update doc comments and test sample builders. -- carry authoritative unread state on the streamed VMs.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- in `room_item_to_vm`, read `num_unread_messages()`, `num_unread_mentions()`, `is_marked_unread()` (via `RoomListItem` Deref to `Room`); compute `is_unread = marked || unread>0 || mentions>0` through a pure, unit-tested helper `room_unread_state(...) -> (bool, u32)`. Extend `mark_room_read` to resolve the room via `room_for` (works with no open timeline), advance the read receipt through the signals seam best-effort, and clear the manual flag via `room.set_unread_flag(false)` when set. Add `mark_room_unread(account_id, room_id)` → `room.set_unread_flag(true)` best-effort. -- source unread in Rust; make read/unread round-trip.
- [x] `src-tauri/crates/keeper-core/src/signals.rs` -- if mark-read from a bare room needs a receipt without an open timeline, add a room-based helper here so `.mark_as_read(` / `.send_single_receipt(` stay inside this file (AD-14). `set_unread_flag`/`is_marked_unread` are account-data, not receipt APIs, so they stay in `account.rs`. -- preserve the AD-14 sole-gate invariant.
- [x] `src-tauri/crates/keeper-core/src/inbox.rs` -- thread `is_unread` + `mention_count` through `to_inbox_room`; update fixtures. -- carry fields across the merge.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- add `#[tauri::command] mark_room_unread` mirroring `mark_room_read`, mapping errors via `to_ipc_error`. -- expose the command.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register `mark_room_unread` in `generate_handler!`. -- wire IPC.
- [x] `src/lib/ipc/client.ts` -- add/confirm typed `markRoomRead(accountId, roomId)` and `markRoomUnread(accountId, roomId)` wrappers over `invoke`. -- typed command access.
- [x] `src/lib/stores/rooms.ts` -- add an ephemeral `Map<accountId|roomId, boolean>` optimistic-unread overlay with `setOptimisticUnread`/reconcile; in `applyBatch`, drop an override once the authoritative VM for that room matches the intended `isUnread`; expose an `effectiveIsUnread(room)` helper. Not a source of truth — mirrors the send local-echo pattern. -- guarantee the within-one-frame update while the stream reconciles.
- [x] `src/components/chat/chat-row.tsx` -- render: name `cn("...", isUnread && "font-semibold")`; when `mentionCount>0` a filled primary `Badge` with the count; else when unread a neutral dot (`size-2 rounded-full bg-muted-foreground`); nothing when read. Wrap the row in `ContextMenu`/`ContextMenuTrigger` (asChild) with items "Mark read" (when unread) / "Mark unread" (when read) that set the optimistic override then invoke the command; `isUnread` from `effectiveIsUnread`. -- render unread state + manual control.
- [x] `src/components/chat/chat-row.test.tsx`, `src/lib/stores/rooms.test.ts` -- test row rendering for each I/O-matrix unread scenario, context-menu action + invoke, and the store's optimistic set + reconcile-on-matching-diff + revert paths. -- cover behavior.

**Acceptance Criteria:**
- Given synced accounts, when the inbox renders, then each row's unread affordance (bold name; primary mention badge with count; neutral dot; or none) reflects the Rust-computed `InboxRoomVm` state and matches server read-marker state after sync convergence.
- Given an unread or read row, when the user picks "Mark read" / "Mark unread" from the context menu, then the row's appearance changes within one frame and the change round-trips to the server (read receipt + `m.marked_unread`), reconciling with the authoritative stream afterward.
- Given a code audit, then `.mark_as_read(` / `.send_single_receipt(` appear only in `signals.rs` (guard test green) and no unread state is computed in TypeScript beyond rendering the streamed fields.

## Design Notes

**SDK surface (matrix-sdk 0.18, verified):** `RoomListItem` derefs to `matrix_sdk::Room`, exposing `num_unread_messages()->u64`, `num_unread_mentions()->u64` (client-side, precise for E2EE), `is_marked_unread()->bool`, and `async set_unread_flag(bool)` (writes `m.marked_unread` account data, no-op if unchanged). Passive updates arrive because `READ_RECEIPT` and `UNREAD_MARKER` are `RoomInfoNotableUpdateReasons`, so the room-list stream emits a `Set` diff → the inbox merger re-emits.

**Mark-read from the inbox:** the existing `mark_room_read` needed an *open* timeline; an inbox row may have none — resolve the live `Room` via `room_for`. For the receipt, reuse an open timeline via `signals::mark_read` or build a transient `TimelineBuilder::new(&room).build().await`, keeping the `.mark_as_read(` call in `signals.rs`. `set_unread_flag`/`is_marked_unread` are account-data, so they stay in `account.rs`.

**Rendering:** `mention_count` drives the filled primary badge (shows the number); any other unread is a plain dot — never a count (UX-DR3). `is_unread` is the single authoritative bold-name/menu-label flag so TS derives nothing.

## Verification

**Commands:**
- `bun run test:rust` -- runs cargo-nextest and regenerates `src/lib/ipc/gen/*.ts`; expected: green, and `InboxRoomVm.ts`/`RoomVm.ts` now include `isUnread` + `mentionCount`.
- `bun run bindings:check` -- expected: no uncommitted drift under `src/lib/ipc/gen`.
- `bun run check:rust` -- expected: rustfmt + clippy `-D warnings` pass; AD-14 guard test `signals_is_the_sole_receipt_typing_gate` green.
- `bun run check` -- expected: biome + tsc + vitest pass, including new chat-row and store tests.

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 2, low 2)
- defer: 1: (high 0, medium 1, low 0)
- reject: 4: (high 0, medium 0, low 4)
- addressed_findings:
  - `[medium]` `[patch]` Mention badge read `room.mentionCount` raw, so an optimistic "Mark read" un-bolded the name but left the filled mention badge for a frame — gated the badge (and the a11y label) on effective unread (`showMention = isUnread && mentionCount > 0`) in `chat-row.tsx`.
  - `[medium]` `[patch]` Optimistic override could strand forever on a hard command rejection or when a room left the streamed window — added `clearOptimisticUnread` (reverted from the mark handlers' `.catch`) and made `reconcileOptimisticUnread` also drop overrides whose room is absent from the window (`rooms.ts`).
  - `[low]` `[patch]` `mark_room_read` gated the marked-unread clear on a racy `is_marked_unread()` read — now calls `set_unread_flag(false)` unconditionally (SDK no-ops when already unset) so a concurrently-set flag is still cleared (`account.rs`).
  - `[low]` `[patch]` Unread state was invisible to assistive tech (dot/badge are `aria-hidden`, badge outside the button's accessible name) — folded an unread/mention cue into the row button's `aria-label` and marked the badge `aria-hidden` to avoid a double announce (`chat-row.tsx`).

## Auto Run Result

Status: done

**Summary:** Implemented Story 4.1 Unread Management. Per-room unread state (unread messages, unread mentions, manual `m.marked_unread` flag) is sourced in Rust from the SDK room-list item, computed via a pure helper, carried on `RoomVm`/`InboxRoomVm` as `isUnread` + `mentionCount`, streamed through the inbox projection, and rendered on each chat row (bold name + filled primary mention badge / neutral dot). A row context menu offers "Mark read" / "Mark unread" that round-trips to the server (read receipt via the AD-14 signals seam using a transient timeline when no timeline is open + `set_unread_flag`) with an optimistic within-one-frame overlay that reconciles against the authoritative stream. Passive convergence rides the room-list `READ_RECEIPT`/`UNREAD_MARKER` notable-update diffs.

**Files changed (code):**
- `src-tauri/crates/keeper-core/src/vm.rs` — added `is_unread` + `mention_count` to `RoomVm` and `InboxRoomVm`.
- `src-tauri/crates/keeper-core/src/account.rs` — pure `room_unread_state` helper; unread sourcing in `room_item_to_vm`; timeline-independent `mark_room_read` (transient-timeline receipt + unconditional marked-unread clear); new `mark_room_unread`.
- `src-tauri/crates/keeper-core/src/inbox.rs` — threaded the two fields through `to_inbox_room`.
- `src-tauri/crates/keeper/src/ipc.rs`, `src-tauri/crates/keeper/src/lib.rs` — `mark_room_unread` command + registration.
- `src/lib/ipc/client.ts` — `markRoomUnread` wrapper (reused existing `markRoomRead`).
- `src/lib/stores/rooms.ts` — optimistic-unread overlay (`optimisticUnread`, `setOptimisticUnread`, `clearOptimisticUnread`, `effectiveIsUnread`, absent-or-converged reconcile).
- `src/components/chat/chat-row.tsx` — unread rendering, mention badge, context menu, accessible unread cue.
- `src/lib/ipc/gen/InboxRoomVm.ts`, `src/lib/ipc/gen/RoomVm.ts` — regenerated ts-rs bindings.
- Tests: `chat-row.test.tsx`, `rooms.test.ts` (+ `chat-list-pane.test.tsx`, `use-sign-out.test.ts` fixtures), Rust unit tests for `room_unread_state` and `to_inbox_room`.

**Review findings:** 4 patches applied (2 medium: mention-badge honoring the optimistic overlay, override stranding on rejection / window-exit; 2 low: unconditional marked-unread clear, accessible unread cue) — each with added test coverage. 1 deferred (no mock-sync harness to test the `mark_room_read`/`mark_room_unread` SDK round-trips → deferred-work ledger). 4 rejected as noise, incl. a "silent no-op mark-read" claim disproven by reading the matrix-sdk-ui source (Live `TimelineBuilder::build` awaits the room event cache before returning, so `mark_as_read` sees the latest event).

**Verification:** `bun run check` (biome + tsc + 450 vitest tests) — PASS; `bun run check:rust` (rustfmt + clippy `-D warnings`, AD-14 guard green) — PASS; `bun run test:rust` (295 cargo-nextest tests; regenerated bindings deterministic) — PASS. `bindings:check`'s git-clean clause is satisfied once the two regenerated gen files are committed (done in this run's commit).

**Residual risks:** Client-side unread counts and closed-timeline mark-read depend on the room event cache being populated for never-opened rooms; correctness after real-homeserver sync convergence is asserted by design but not covered by an automated integration test (see the deferred item). The "never-converges" optimistic-overlay edge (server settles on a value different from the user's intent, e.g. a concurrent read on another client) leaves an override until app restart — accepted as a low-consequence ephemeral degradation.
