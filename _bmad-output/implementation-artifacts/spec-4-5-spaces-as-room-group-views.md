---
title: 'Spaces as Room-Group Views'
type: 'feature'
created: '2026-07-04'
status: 'done'
baseline_revision: '951e172d341e05e471eab031291b75b61790206b'
final_revision: '968801b7114cea9f368fd9cbab99fb1c00ef6b02'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** The Unified Inbox cannot be scoped to a Matrix Space. FR-22 / UX-DR (SPACES sidebar group + single-select Space filter) is unbuilt: the user who belongs to Spaces has no way to list them or narrow the inbox to one Space's rooms.

**Approach:** Add a Rust-side, sync-reactive Spaces layer. A per-account **spaces producer** enumerates joined Spaces (`Client::joined_space_rooms()`) and computes each Space's joined child rooms locally from `m.space.child` state (`Room::get_state_events_static::<SpaceChildEventContent>()`), recomputing on every sync batch (`Client::subscribe_to_all_room_updates()`). It pushes both the Space list and the membership map into the live `InboxMerger`, which streams the aggregated Space list on a fifth `inbox_subscribe` channel and, when a Space is selected, filters all four inbox windows (Inbox/Archive/Pins/Favorites) to that Space's rooms before partitioning — never in TypeScript. Selection is an ephemeral filter poked into the merger via a `set_space_filter` command (mirroring `reorder_pins` → `update_pins`). The frontend adds a SPACES sidebar group (single-select, hidden when empty), a dismissible filter chip above the chat list, Esc-to-clear, and a "No chats in {Space}" empty state.

## Boundaries & Constraints

**Always:** Space enumeration, child-membership, filtering, and window totals are computed in Rust and streamed as authoritative view models (AD-20) — TS never derives, sorts, or filters inbox membership. Space membership is read from **local state only** (`get_state_events_static::<SpaceChildEventContent>()`, each child event's `state_key` is the child room id; keep Sync `Original` + `Stripped`, drop `Redacted`), cross-referenced against the account's joined rooms — no network `/hierarchy` call. The producer recomputes on `subscribe_to_all_room_updates()` batches (treat `Lagged` as "recompute", `Closed` as "stop"), so Space list and filter results update live on sync. The Space filter is applied **inside the merger's `emit`** over the merged, recency-ordered set before the pins/favorites/inbox/archive partition, so precedence (Pins > Favorites > Archive/Inbox) and recency order are preserved within the filtered subset and each window `total` reflects the filtered count. Space rooms themselves (`Room::is_space()`) are excluded from all four chat windows. Selection is single-Space, identified by `(account_id, space_id)`; `set_space_filter(None, None)` clears. Mutations/streams follow the IPC contract (AD-8): `set_space_filter` is a `domain_verb` command routed to the live merger; the Space list rides a fifth `Channel` on `inbox_subscribe`. Frontend Space state lives in one `spaces` zustand vanilla mirror (AD-9); the SPACES group is hidden entirely when the aggregated list is empty.

**Block If:** (none expected — every SDK call is verified present in matrix-sdk 0.18 [`joined_space_rooms` client/mod.rs:1334, `get_state_events_static` room/mod.rs:1320, `subscribe_to_all_room_updates` client/mod.rs:1280, `is_space` base room/mod.rs:153]; additive layer over the established merger/poke pattern, no external decision required.)

**Never:** No create/edit/join/leave/invite or any Space **management** — view-and-filter only. No recursive sub-space traversal or hierarchy flattening — a selected Space filters to its **direct joined children** only (nested sub-spaces are themselves Space rooms, excluded from chat windows). No network `SpaceRoomList::paginate()` / `/hierarchy` fetch (would surface non-joined rooms — out of scope). No persisted filter (the Space filter is ephemeral view state, cleared on relaunch — unlike the Favorites collapse setting). No per-row `space_ids` on `InboxRoomVm` and no TS-side membership filtering. No Network filter, Network badge, or per-row Account attribution (Story 4.6 — it will add its chip alongside this one). No single-key Space verbs (Epic 9). No `matrix_sdk_ui::spaces::SpaceService` (its `children_of` graph is `pub(super)` and `space_room_list` hits the network — build the local list/membership directly).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| User belongs to Spaces | sync converged, ≥1 joined Space | SPACES group lists each Space (avatar + name) from all accounts; inbox unfiltered | producer read error on one Space → skip it, log `debug!`, keep others |
| Select a Space | click a SPACES row | `set_space_filter(acct, space_id)`; merger re-emits all four windows filtered to that Space's joined children (recency + precedence preserved); dismissible chip "{Space name}" appears above the chat list | filter poke on a torn-down merger → best-effort no-op |
| Clear the filter | click active row again / chip ✕ / Esc from list | `set_space_filter(None, None)`; merger re-emits full unfiltered windows; chip removed | n/a |
| Selected Space has no joined chats | filter active, 0 rooms match | Inbox window empty; chat list shows "No chats in {Space name}." with a Clear filter action | n/a |
| No Spaces at all | joined_space_rooms empty | SPACES group hidden entirely (label + rows absent) | n/a |
| Space membership changes from sync | `m.space.child` added/removed, or a child (un)joined | producer recomputes on the sync batch → merger updates the membership map and re-emits; the filtered list updates live | `Lagged` broadcast → force full recompute |
| A room is itself a Space | `is_space()` room in the room list | excluded from Inbox/Archive/Pins/Favorites windows (containers, not chats) | n/a |
| Account signed out while its Space selected | removed account owned the active Space | merger drops that account's Spaces/membership; if it owned the selection, clears the filter and re-emits full windows + updated Space list | n/a |
| Relaunch | filter was active | starts unfiltered (ephemeral); Spaces re-list from sync | n/a |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- `RoomVm` (~L320): add `is_space: bool` (used only to exclude Space rooms from windows; **not** copied to `InboxRoomVm`). Update every sample/test builder. NEW `SpaceVm { account_id, space_id, name, avatar_url: Option<String> }` and `SpacesSnapshot { spaces: Vec<SpaceVm> }`, both `#[serde(rename_all="camelCase")] #[ts(export)]` (generates `SpaceVm.ts`/`SpacesSnapshot.ts`). `InboxRoomVm` (~L912) unchanged.
- `src-tauri/crates/keeper-core/src/account.rs` -- `room_item_to_vm` (~L2585): read `item.is_space()` into `RoomVm.is_space` (no await). NEW `type SpacesSink = Box<dyn Fn(SpacesSnapshot) -> bool + Send + Sync>`. `subscribe_inbox` (~L313): take a fifth `spaces_sink`, pass to `InboxMerger::new`, and per account spawn `run_spaces_producer(client, merger, account_id)` alongside the room-list producer; track its `JoinHandle` in `InboxHandle` (~L164) so teardown aborts it. NEW `run_spaces_producer`: subscribe `client.subscribe_to_all_room_updates()`, compute once then on each batch: for each `client.joined_space_rooms()` build a `SpaceVm` (name via `room.display_name().await` → `RoomDisplayName.to_string()`, exactly as `room_item_to_vm` resolves names at account.rs:2587; `room.avatar_url()`) and its child set from `space.get_state_events_static::<SpaceChildEventContent>().await` (Sync `Original` + `Stripped` `state_key`s) filtered to rooms the account has joined; call `merger.update_spaces(account_id, spaces, memberships)`. NEW `set_space_filter(&self, selection: Option<(String,String)>)` (~near L1705 `reorder_pins`): `self.inbox.lock().await` → `handle.merger.set_space_filter(selection)`.
- `src-tauri/crates/keeper-core/src/inbox.rs` -- `MergeState`/`InboxMerger` (~L49): add `spaces_sink: SpacesSink`, `account_spaces: HashMap<String, Vec<SpaceVm>>`, `space_children: HashMap<(String,String), HashSet<String>>`, `selected_space: Option<(String,String)>`. `new(...)` (~L91): add `spaces_sink` (last arg). NEW `update_spaces(account_id, Vec<SpaceVm>, HashMap<String,HashSet<String>>)`: replace that account's entries, `emit_spaces()`, then `emit()`. NEW `set_space_filter(Option<(String,String)>)`: store, `emit()`. NEW `emit_spaces()`: flatten `account_spaces` (stable account-id order) → `SpacesSnapshot` → `spaces_sink`. `emit` (~L174): in/after `merge` (~L256) drop `is_space` rooms; if `selected_space=Some((a,s))`, retain only rooms with `account_id==a && space_children[(a,s)].contains(room_id)` before the pins/favorites/inbox/archive partition; totals per filtered window. `remove_account` (~L120): also drop the account's `account_spaces`/`space_children`; if `selected_space`'s account matches, set it `None`; `emit_spaces()` + `emit()`. Extend `capturing_merger`/`capturing_merger_with_pins` (~L530) to a fifth (spaces) capture and add a helper to feed spaces/memberships.
- `src-tauri/crates/keeper/src/ipc.rs` -- `inbox_subscribe` (~L1429): add a fifth `spaces: Channel<SpacesSnapshot>` wrapped into a `SpacesSink`. NEW `#[tauri::command] set_space_filter(state, account_id: Option<String>, space_id: Option<String>)` → `AccountManager::set_space_filter(account_id.zip(space_id))` (mirror `reorder_pins` ~L1279; `to_ipc_error`).
- `src-tauri/crates/keeper/src/lib.rs` -- `generate_handler!` (~L47): register `set_space_filter`.
- `src/lib/ipc/client.ts` -- `subscribeInbox` (~L271): take a fifth `onSpaces`, create a fifth `Channel<SpacesSnapshot>`, pass `spaces` to `inbox_subscribe`. NEW `setSpaceFilter(accountId: string | null, spaceId: string | null): Promise<void>` → invoke `set_space_filter`.
- `src/lib/stores/spaces.ts` -- NEW slim mirror: `spaces: SpaceVm[]`, `activeSpace: { accountId: string; spaceId: string } | null`, `applySnapshot(snapshot)` (replace list), `setActiveSpace(sel | null)`, `clear()`; `spacesStore` + `useSpacesStore`.
- `src/components/layout/spaces-group.tsx` -- NEW: uppercase "SPACES" `section-label` group of single-select rows (`RoomAvatar` + name), active via `aria-current`/`bg-accent`; `return null` when `spaces.length === 0`. Row click toggles: if already active → clear, else select; calls `setActiveSpace(...)` **and** `setSpaceFilter(...)`.
- `src/components/layout/sidebar-pane.tsx` -- render `<SpacesGroup>` after the Archive view, before Settings (~L108).
- `src/components/layout/chat-list-pane.tsx` -- subscribe the fifth channel → `spacesStore.applySnapshot`; clear `spacesStore` on unsubscribe; after (re)subscribe re-apply `setSpaceFilter(activeSpace)` if one is set (survive account-set re-subscribe). Render a dismissible filter chip (space name + ✕ → clear) above the `ScrollArea` when `activeSpace !== null`; Esc from the list clears an active filter. When the active view's rows are empty **and** `activeSpace !== null`, render "No chats in {space name}." with a Clear filter action.
- Tests: `inbox.rs` (filter partitions all four windows + preserves precedence/recency; `is_space` excluded; `update_spaces` re-emits; `remove_account` clears a selection it owned; empty-membership → empty windows); `spaces.test.ts` (applySnapshot/setActiveSpace/clear); `spaces-group.test.tsx` (renders, hidden when empty, select/clear toggle invokes `setSpaceFilter`); `chat-list-pane.test.tsx` (fifth channel feeds store; chip shows/dismisses; Esc clears; "No chats in {Space}" empty state; re-apply on resubscribe); `client.ts` wrapper; fixtures updated in `rooms.test.ts`/`archive-rooms.test.ts`/`pins-rooms.test.ts`/`favorites-rooms.test.ts`/`use-sign-out.test.ts` for the new `RoomVm.is_space` field.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `RoomVm.is_space`; NEW `SpaceVm` + `SpacesSnapshot` (`#[ts(export)]`); update builders; regenerate bindings. -- carry Space room-type + stream shapes.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- read `item.is_space()`; `SpacesSink`; `subscribe_inbox` fifth sink + spawn `run_spaces_producer` (local child computation, sync-reactive); `set_space_filter` routing; track/abort the spaces producer in `InboxHandle`. -- enumerate Spaces + membership and wire the filter poke.
- [x] `src-tauri/crates/keeper-core/src/inbox.rs` -- merger `spaces_sink`/`account_spaces`/`space_children`/`selected_space`; `update_spaces`, `set_space_filter`, `emit_spaces`; `emit` excludes `is_space` and applies the Space filter pre-partition; `remove_account` cleanup + selection clear; five-capture fixtures + golden tests. -- compute the filtered windows + Space list from one merge.
- [x] `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- fifth `inbox_subscribe` channel; `set_space_filter` command + registration. -- expose the Space stream + filter mutation.
- [x] `src/lib/ipc/client.ts` -- `subscribeInbox(…, onSpaces)`; `setSpaceFilter`. -- typed access to the fifth stream + command.
- [x] `src/lib/stores/spaces.ts` -- NEW mirror (list + active selection). -- hold Spaces + the active filter.
- [x] `src/components/layout/spaces-group.tsx` -- NEW SPACES sidebar group, single-select, hidden when empty. -- list + select Spaces.
- [x] `src/components/layout/sidebar-pane.tsx` -- render `<SpacesGroup>`. -- place the group.
- [x] `src/components/layout/chat-list-pane.tsx` -- fifth channel → store; filter chip + Esc-clear; re-apply on resubscribe; "No chats in {Space}" empty state. -- surface the filter + empty state.
- [x] Tests -- Rust merger filter/exclude/membership/removal; TS store, spaces-group, chat-list-pane (chip/Esc/empty/resubscribe), client wrapper; VM-field fixtures. -- cover behavior.

**Acceptance Criteria:**
- Given the user belongs to ≥1 Matrix Space, when sync converges, then each joined Space is listed in a SPACES sidebar group (hidden entirely when there are none); selecting a Space filters the Unified Inbox to that Space's joined rooms with the four-way split (Pins/Favorites/Inbox/Archive) and recency order computed and streamed from Rust — never re-derived in TypeScript (FR-22, AD-20) — and shows a dismissible chip above the chat list.
- Given an active Space filter, when the user clicks the active row again, dismisses the chip, or presses Esc from the chat list, then the filter clears and the full inbox is restored; when the filtered inbox is empty, then "No chats in {Space name}." with a Clear filter action is shown.
- Given `m.space.child` membership or a child room's join state changes on sync, then the Space list and the filtered results update live (recomputed in Rust off `subscribe_to_all_room_updates`), and Space rooms themselves never appear as chat rows.
- Given a code audit, then Space membership is read from local state (`get_state_events_static::<SpaceChildEventContent>()`, no `/hierarchy` network call), the filter is applied inside `InboxMerger::emit`, the selection is ephemeral (no registry key), no `space_ids` field is added to `InboxRoomVm`, and `.mark_as_read(`/`.send_single_receipt(` remain solely in `signals.rs` (AD-14 guard green).

## Spec Change Log

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 1, low 2)
- defer: 3: (high 0, medium 1, low 2)
- reject: 13: (high 0, medium 0, low 13)
- addressed_findings:
  - `[medium]` `[patch]` A stale Space selection was never reconciled against the streamed list: after signing out the account that owned the active Space filter (with other accounts still signed in) — or leaving the selected Space — the frontend re-poked `setSpaceFilter` for the now-absent `(account, space)`, so the merger filtered on a Space with no members and emptied every window indefinitely while the chip showed a nameless Space. Fixed in `chat-list-pane.tsx` `onSpaces`: when the active selection is absent from an incoming snapshot, clear the store selection and poke `setSpaceFilter(null, null)`. Regression test added (`chat-list-pane.test.tsx` "reconciles a stale Space selection absent from a streamed snapshot"). `bun run check` green.
  - `[low]` `[patch]` `InboxMerger::update_spaces` re-emitted all four inbox windows on every Space recompute — which fires on every sync `RoomUpdates` — doubling inbox emissions even when no filter was active (windows can't change from Space data when unfiltered). Gated the window re-emit on `selected_space.is_some()` (the spaces snapshot always emits). Regression test added (`inbox.rs` `update_spaces_without_active_filter_skips_window_re_emit`).
  - `[low]` `[patch]` `joined_space_rooms()` order is unspecified (store-map iteration), so the sidebar Space list could reshuffle between snapshots. Sorted each account's Spaces deterministically (by name, then id) in `compute_and_push_spaces` before pushing to the merger.
- Deferred (3): multi-account Space name collisions render indistinguishably (per-row account attribution is Story 4.6/FR-24 scope); the spaces producer recomputes fully on every `RoomUpdates` with no debounce/diff (bounded, low impact); `run_spaces_producer`'s broadcast handling (Lagged/Closed) + initial-compute ordering have no Rust test (needs the mock-Matrix-sync harness already missing crate-wide).
- Rejected (13, by-design / consistent / self-healing / noise): Space `mxc://` avatar falls back to initials (identical to `RoomAvatar`'s app-wide behavior — the media scheme handler isn't wired for avatars yet, with a matching chat-row test); `m.space.child` membership doesn't check empty-`via` (removed-link) content (matches the referenced matrix-sdk-ui `build_space_state`, and joined-room cross-reference bounds impact); the vanished-account client-clone skip is silent (narrow spawn-gap race on an account already being torn down); abort-without-await of the spaces producer on re-subscribe (writes to a dead channel — benign, matches existing producer teardown); the resubscribe `setSpaceFilter` re-poke race / one-frame flicker (self-healing, inherent to Rust-authoritative streaming, and the correctness half is covered by the reconcile patch); transient empty windows before the producer's first compute after resubscribe (self-heals within one sync); account-switcher filter AND Space filter composing to an empty view with Space-centric empty copy (both filters working as designed; the copy is not incorrect); Esc-clear scoped to focus-within-the-pane + `preventDefault` when active (matches the AC "Esc from the list"); the Space empty-state taking precedence over the Archive empty-state (the filter is genuinely active); a nested sub-Space child excluded from the filtered chat rows (a Space is a container, shown in the SPACES group, never a chat row — by spec); redundant `aria-current` + `aria-pressed` on the row (not broken; consistent with prior epic triage); and an intra-account `space_id` collision (structurally impossible — one state event per child key).

## Design Notes

**Space-centric membership (not per-row), filtered in the merger.** A child room's membership is authoritative only from the **Space** side (`m.space.child` on the Space room); child-side `m.space.parent` links are optional and unreliable, so we never stamp `space_ids` per room. Instead the producer builds `space_children: (account, space) → {room_ids}` exactly as matrix-sdk-ui's own `build_space_state` does (mod.rs:531 — keep `Sync::Original` + `Stripped` `state_key`s, cross-reference the account's joined rooms) and the merger filters the merged set by it. This keeps `InboxRoomVm` unchanged and the filter a pure projection, and makes "10k chats, filter to a Space" a Rust-side retain over the already-merged vector (performance floor honored — no rows shipped to JS for filtering).

**Filter placement mirrors `update_pins`.** `reorder_pins` (ipc.rs:1279) → `AccountManager::reorder_pins` → `handle.merger.update_pins` → `emit()` is the proven "poke the live merger, re-emit windows" path. `set_space_filter` and `update_spaces` reuse it verbatim, so no re-subscribe/teardown (the whole-inbox rebuild flash deferred from Story 2.1 is avoided). Applying the filter *before* the existing three-stage partition means precedence (Pins > Favorites > Archive/Inbox) and per-window recency/tie-breaks are unchanged — the Space filter only narrows the input set.

**Why local, why not SpaceService.** `matrix_sdk_ui::spaces::SpaceService` gives a reactive top-level list but its `children_of` graph is `pub(super)` and `space_room_list()` performs a network `/hierarchy` fetch (surfacing non-joined rooms). Both conflict with "view-and-filter joined rooms, local, no management," so we read `joined_space_rooms()` + `get_state_events_static` directly and drive recomputation off `subscribe_to_all_room_updates()` (the same broadcast SpaceService itself consumes).

**Ephemeral filter.** Unlike the Favorites collapse toggle (persisted UI chrome), a Space filter is transient triage state that "clears to restore the inbox," so it lives only in the merger + the `spaces` store — no registry key, no cross-client sync. Relaunch starts unfiltered.

**Composition with Story 4.6.** The chip bar is built to hold multiple dismissible chips (AND-composed); 4.5 wires only the Space chip. The account filter stays a display-only TS filter (pre-existing); it composes on top of the Rust Space filter without conflict.

## Verification

**Commands:**
- `bun run check:rust` -- rustfmt + clippy `-D warnings`; AD-14 guard `signals_is_the_sole_receipt_typing_gate` green (Spaces touch no receipt/typing API).
- `bun run test:rust` -- cargo-nextest green; new `inbox` Space-filter/exclusion/membership tests pass.
- `bun run bindings:check` -- no uncommitted drift under `src/lib/ipc/gen` after regenerating `SpaceVm.ts`/`SpacesSnapshot.ts`/`RoomVm.ts`.
- `bun run check` -- biome + tsc + vitest pass, including new spaces store, spaces-group, and chat-list-pane tests.

## Auto Run Result

Status: done

**Summary:** Implemented Story 4.5 Spaces as Room-Group Views. A per-account, sync-reactive **spaces producer** (`run_spaces_producer`/`compute_and_push_spaces`) enumerates joined Matrix Spaces via `Client::joined_space_rooms()` and computes each Space's joined child rooms **locally** from `m.space.child` state (`Room::get_state_events_static::<SpaceChildEventContent>()`, keeping Sync `Original` + `Stripped` `state_key`s, cross-referenced to the account's joined rooms — no `/hierarchy` network call), recomputing on every `Client::subscribe_to_all_room_updates()` batch (`Lagged`→recompute, `Closed`→stop). It pushes the Space list + membership map into the live `InboxMerger`, which streams the aggregated `SpacesSnapshot` on a **fifth** `inbox_subscribe` channel and, when a Space is selected via the new `set_space_filter` command (mirroring `reorder_pins`→`update_pins`), filters all four inbox windows to that Space's rooms **before** the Pins>Favorites>Archive/Inbox partition — so precedence and recency are preserved and each window `total` is the filtered count. Space rooms (`is_space`) are excluded from the chat windows. The filter is **ephemeral** (no registry key). The frontend adds a `spaces` mirror store, a single-select SPACES sidebar group (hidden when empty), a dismissible filter chip above the chat list, Esc-to-clear, a "No chats in {Space}." empty state, and re-applies/reconciles the selection across account-set re-subscribes. All ordering/filtering/sectioning stays Rust-authoritative (AD-20); Spaces touch no receipt/typing API (AD-14 seam untouched).

**Files changed (code):**
- `src-tauri/crates/keeper-core/src/vm.rs` — `RoomVm.is_space`; new `SpaceVm` + `SpacesSnapshot` (`#[ts(export)]`).
- `src-tauri/crates/keeper-core/src/account.rs` — `item.is_space()` read; `SpacesSink`; `subscribe_inbox` fifth sink + per-account `run_spaces_producer` (tracked/aborted in `InboxHandle`); `set_space_filter` routing; `compute_and_push_spaces` (local Space list + membership, deterministic sort).
- `src-tauri/crates/keeper-core/src/inbox.rs` — merger `spaces_sink`/`account_spaces`/`space_children`/`selected_space`; `update_spaces`/`set_space_filter`/`emit_spaces`; `emit` drops `is_space` + applies the pre-partition filter; `remove_account` cleanup + selection clear; five-capture fixtures + tests.
- `src-tauri/crates/keeper/src/ipc.rs`, `lib.rs` — fifth `inbox_subscribe` channel; `set_space_filter` command + registration.
- `src/lib/ipc/client.ts` — `subscribeInbox(…, onSpaces)`; `setSpaceFilter`.
- `src/lib/stores/spaces.ts` (new), `src/components/layout/spaces-group.tsx` (new), `src/components/layout/sidebar-pane.tsx` (renders `<SpacesGroup>`), `src/components/layout/chat-list-pane.tsx` (fifth channel → store; chip; Esc-clear; "No chats in {Space}." empty state; re-apply + stale-selection reconcile on resubscribe).
- `src/lib/ipc/gen/RoomVm.ts`, `SpaceVm.ts`, `SpacesSnapshot.ts` — regenerated (additive).
- Tests: `inbox.rs` (filter partition/precedence, `is_space` exclusion, snapshot order, empty-membership, `remove_account` selection clear, unfiltered-recompute skip), `vm.rs` round-trips; `spaces.test.ts`, `spaces-group.test.tsx`, `client.test.ts`, `chat-list-pane.test.tsx` (fifth channel, chip, Esc, empty state, stale-selection reconcile).

**Review findings:** 3 patches applied — (medium) stale Space selection reconciled against the streamed snapshot so signing out the filter-owning account no longer leaves the inbox empty-forever (+ regression test); (low) `update_spaces` window re-emit gated on an active filter to avoid doubling inbox emissions per sync (+ regression test); (low) deterministic Space ordering so the sidebar list doesn't reshuffle. 3 deferred (same-name multi-account Space rows → 4.6 attribution; no debounce/diff on the spaces recompute; producer broadcast-lifecycle untested pending a mock-sync harness). 13 rejected as by-design / consistent-with-precedent / self-healing / noise.

**Verification:** `bun run check:rust` (rustfmt + clippy `-D warnings`, AD-14 guard `signals_is_the_sole_receipt_typing_gate` green) — PASS; `bun run test:rust` — PASS (315 cargo-nextest, bindings regenerated); `bun run check` (biome + tsc + 516 vitest + core-tauri-free guard) — PASS. `bindings:check`'s `git status --porcelain` clause goes green once the regenerated `RoomVm.ts`/`SpaceVm.ts`/`SpacesSnapshot.ts` are committed (done in this run's commit); regeneration is idempotent and additive.

**Residual risks:** The Space filter and mutations are best-effort with no optimistic overlay (like Pins/Favorites/Archive): windows move only when the merger re-emits — sub-frame on the SDK's live updates; a genuinely-failed `set_space_filter` dispatch is a silent no-op. A joined room in a selected Space but not currently in any account's live SlidingSync window won't appear until it syncs (the same windowed-merge limitation already deferred for Pins/Inbox/Archive). The filter is ephemeral (cleared on relaunch, by design). The spaces producer recomputes fully per sync with no debounce (deferred). The producer's broadcast lifecycle is untested pending a mock-sync harness (deferred).

**Follow-up review recommended:** false — the final pass applied three localized patches (one medium recoverable frontend bug with a regression test, two low-consequence Rust efficiency/stability fixes with a regression test), with no bad_spec/intent_gap and no behavior/API/security/data-shape change beyond a bugfix guard. Below the bar for an independent follow-up review.
