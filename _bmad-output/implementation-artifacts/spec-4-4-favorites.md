---
title: 'Favorites'
type: 'feature'
created: '2026-07-04'
status: 'done'
baseline_revision: '92706353d03cd1da42aca4553e588b2d98b5bdb3'
final_revision: 'ce14e2a7e5333815852ff3bde8bf2340995c8128'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** The Unified Inbox has no Favorites. A user cannot curate an always-visible section of key people that stays one interaction away regardless of inbox scroll position. FR-21 / UX-DR4 / UX-DR13 (Favorites section) is unbuilt.

**Approach:** Add a fourth inbox window — **Favorites** — to the Rust `InboxMerger` alongside Inbox, Archive, and Pins. Unlike Pins (keeper-local), favorite state rides the Matrix **`m.favourite` tag**, which is a *notable* tag in matrix-sdk-base 0.18 (`RoomNotableTags::FAVOURITE`, see `handle_notable_tags`), so `Room::set_is_favourite` re-emits the room-list stream live and the state syncs cross-client — architecturally a clone of Archive (Story 4.2's low-priority tag). The frontend mirrors the favourites window into a slim store and renders an always-visible labeled "FAVORITES" section of compact 48 px rows between the Pins strip and the inbox scroll, with a persisted collapse/expand toggle, hidden entirely when empty.

## Boundaries & Constraints

**Always:** Favourite membership, sectioning, and the four-way partition are computed in Rust and streamed as an authoritative windowed `InboxBatch` (AD-20) — TS never derives, sorts, or filters favourite state. Favourite state comes from `item.is_favourite()` (the cached `m.favourite` notable tag), read exactly as `is_archived` reads `item.is_low_priority()`. `favourite_room`/`unfavourite_room` call `Room::set_is_favourite(true/false, None)` best-effort (mirror `archive_room`), relying on the SDK's live notable-tag re-emit — **no** out-of-band merger poke (that is a pins-only device). Favourited rooms are removed from Inbox/Archive (they live only in the Favorites window) and keep merged recency order. Precedence is Pins > Favorites > Archive/Inbox, so a pinned-and-favourited room shows only in Pins. Collapse/expand state persists via the registry `settings` table (`get_setting`/`set_setting`, key `favorites_collapsed`) surviving restart and re-login. Mutations are `domain_verb` snake_case Tauri commands (AD-8); the frontend uses the single `inbox`-family mirror-store pattern (AD-9) applying diffs via `applyDiffOp`, extending Story 4.3's `inbox_subscribe` to a fourth `Channel<InboxBatch>`.

**Block If:** (none expected — additive fourth window over an established pattern; favourite maps to a standard notable Matrix tag with a native SDK setter, so no external decision is required.)

**Never:** No keeper-local persistence of favourite membership (it is server-representable — use the tag; local persistence is only for the collapse-toggle UI chrome). No out-of-band `update_pins`-style re-emit for favouriting (the notable tag drives the stream). No user-controlled ordering / drag-reorder of favorites (recency order only; ordering is a pins-only feature). No `m.lowpriority` handling here (archive is Story 4.2; note the SDK makes favourite and low-priority mutually exclusive — favouriting auto-clears archive and vice-versa). No optimistic TS overlay that hoists/hides a row (Rust-authoritative). No single-key `f` verb (deferred to Epic 9 — context-menu only). No Network badge work (Story 4.6).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Favorite a chat | inbox row, `favourite_room(acct,room)` | `set_is_favourite(true)` sets `m.favourite`; on the SDK's live re-emit the row leaves the Inbox window and appears in the Favorites window (compact row) in recency order | best-effort; dispatch error → `debug!` and swallowed, no partial state |
| Unfavorite a chat | favorites row / chat-row, `unfavourite_room(acct,room)` | `set_is_favourite(false)` clears the tag; on re-emit the row returns to its correct chronological Inbox position | best-effort; on error the favourite remains |
| Favorite an archived chat | archived row favourited | SDK removes `m.lowpriority` too; room appears in Favorites, not Archive (mutually exclusive tags) | n/a |
| Pinned + favourited | room is both pinned and favourite | stays in Pins window only (pins win); not duplicated into Favorites | n/a |
| Collapse the section | toggle chevron | list hides (label + toggle remain); `favorites_collapsed=true` persisted; restored on next launch/login | set error → `debug!`, in-memory state still toggles |
| No favorites yet | favourites window empty | section hidden entirely (label, toggle, rows all absent); chat-row context menu shows a one-time Favorites hint instead | n/a |
| Relaunch / re-login | favourite tags synced from server; `favorites_collapsed` in `keeper.db` | Favorites window restores from sync; collapse state restores from the setting | missing setting → default expanded |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- `RoomVm` (~L320-347): add `is_favourite: bool`. `InboxRoomVm` (~L906-944): add `is_favourite: bool` (`#[ts(export)]` regenerates `InboxRoomVm.ts` → `isFavourite`); update every sample/test builder.
- `src-tauri/crates/keeper-core/src/account.rs` -- `room_item_to_vm` (~L2565): add `let is_favourite = item.is_favourite();` (cached `m.favourite` notable tag, no await) and set it on `RoomVm`, mirroring `is_archived`. Add `favourite_room`/`unfavourite_room(account_id, room_id)` (~after L1608) mirroring `archive_room`/`unarchive_room`: `room_for` → `room.set_is_favourite(true/false, None)` best-effort `debug!`. `subscribe_inbox` (~L312): accept a fourth `favourites_sink` and pass to `InboxMerger::new`.
- `src-tauri/crates/keeper-core/src/inbox.rs` -- `MergeState`/`InboxMerger` (~L49-98): add `favourites_sink: InboxSink`; `new(inbox_sink, archive_sink, pins_sink, favourites_sink, pins)`. `emit` (~L160-216): after hoisting pins, build the **Favorites** window (`!pinned && is_favourite`, merged recency order), and partition the rest: Inbox `!pinned && !is_favourite && (!is_archived || is_unread)`, Archive `!pinned && !is_favourite && is_archived && !is_unread`; Reset all four sinks with per-window `total`. `to_inbox_room` (~L256-272): copy `is_favourite` from `RoomVm` (SDK-sourced, like `is_archived` — not merger-defaulted like `is_pinned`). Extend `capturing_merger`/`capturing_merger_with_pins` (~L488-518) to four captures.
- `src-tauri/crates/keeper/src/ipc.rs` -- `inbox_subscribe` (~L1356): add a fourth `Channel<InboxBatch>` (favourites) wrapped into a sink. Add `#[tauri::command] favourite_room`/`unfavourite_room` (mirror `archive_room` ~L1125, `to_ipc_error`). Add `get_favorites_collapsed` (→ `bool`) / `set_favorites_collapsed(collapsed: bool)` resolving `data_dir` via `state.platform.data_dir()`, using `registry::get_setting`/`set_setting` with key `favorites_collapsed` (store `"true"`/`"false"`; unset → `false`).
- `src-tauri/crates/keeper/src/lib.rs` -- `generate_handler!` (~L92-105): register `favourite_room`, `unfavourite_room`, `get_favorites_collapsed`, `set_favorites_collapsed`.
- `src/lib/ipc/client.ts` -- `subscribeInbox` (~L270): take a fourth `onFavourites`, create a fourth channel. Add `favoriteRoom`/`unfavoriteRoom(accountId, roomId)` (best-effort) and `getFavoritesCollapsed(): Promise<boolean>` / `setFavoritesCollapsed(collapsed): Promise<void>` wrappers mirroring `archiveRoom`.
- `src/lib/stores/favorites-rooms.ts` -- NEW: slim mirror (`favoritesRoomsStore` + `useFavoritesRoomsStore`) — `rooms`, `total`, `applyBatch` via `applyDiffOp`, `clear` (copy of `pins-rooms.ts`).
- `src/lib/stores/favorites-ui.ts` -- NEW: vanilla zustand `{ isCollapsed, setCollapsed(v) }` (ephemeral in-memory; hydrated from `getFavoritesCollapsed()` and persisted through `setFavoritesCollapsed`).
- `src/components/layout/favorites-section.tsx` -- NEW: always-visible labeled section. Hidden entirely (`return null`) when `favorites.length === 0`. Header: uppercase "FAVORITES" `section-label` + a collapse/expand chevron button (`aria-expanded`). When expanded, a list of compact 48 px rows (`<RoomAvatar size="lg">` + `displayName`, single line), each selectable (`onSelect`) with a per-row context menu offering Unfavorite (`unfavoriteRoom`). `sticky`/`shrink-0` so it stays visible above the inbox scroll.
- `src/components/layout/chat-list-pane.tsx` -- subscribe the fourth channel into `favoritesRoomsStore`; compute `visibleFavorites` under the account filter (like `visiblePins`); render `<FavoritesSection>` between `<PinsStrip>` and the inbox `<ScrollArea>`, inbox view only; on mount hydrate collapse via `getFavoritesCollapsed()` into `favorites-ui`.
- `src/components/chat/chat-row.tsx` -- add a Favorite/Unfavorite context-menu item gated on `room.isFavourite` (`favoriteRoom`/`unfavoriteRoom` best-effort); when the favorites window is empty (`useFavoritesRoomsStore(s => s.total) ?? 0 === 0`), render a one-time muted hint near the Favorite item (UX-DR13).
- Tests: `inbox.rs` (four-window partition + precedence), `favorites-rooms.test.ts`, `favorites-ui.test.ts`, `favorites-section.test.tsx`, `chat-row.test.tsx`, `chat-list-pane.test.tsx`; fixtures updated in `rooms.test.ts`/`archive-rooms.test.ts`/`pins-rooms.test.ts`/`use-sign-out.test.ts` for the new VM field.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `is_favourite: bool` to `RoomVm` and `InboxRoomVm` with doc comments; update every sample/test builder; regenerate `InboxRoomVm.ts`. -- carry favourite state on the streamed VM.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- read `item.is_favourite()` into `RoomVm`; add `favourite_room`/`unfavourite_room` (best-effort `set_is_favourite`); `subscribe_inbox` takes `favourites_sink`. -- wire favourite read + mutations (SDK-live, no poke).
- [x] `src-tauri/crates/keeper-core/src/inbox.rs` -- add `favourites_sink`; in `emit` build the Favorites window (`!pinned && is_favourite`, recency order) and exclude favourites from Inbox/Archive; Reset all four sinks; copy `is_favourite` through `to_inbox_room`; extend fixtures to four sinks + golden four-window/precedence test. -- compute all four windows from one merge.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- fourth `inbox_subscribe` channel; `favourite_room`/`unfavourite_room`; `get_favorites_collapsed`/`set_favorites_collapsed` (registry `favorites_collapsed`). -- expose stream + mutations + collapse persistence.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register the four new commands in `generate_handler!`. -- wire IPC.
- [x] `src/lib/ipc/client.ts` -- `subscribeInbox(…, onFavourites)`; `favoriteRoom`/`unfavoriteRoom`; `getFavoritesCollapsed`/`setFavoritesCollapsed`. -- typed access to the fourth stream + commands.
- [x] `src/lib/stores/favorites-rooms.ts` -- NEW slim mirror via `applyDiffOp`. -- mirror the favourites window.
- [x] `src/lib/stores/favorites-ui.ts` -- NEW collapse-state store. -- hold + expose collapse/expand.
- [x] `src/components/layout/favorites-section.tsx` -- NEW labeled section: compact 48 px rows, collapse toggle, per-row Unfavorite, hidden when empty. -- render favorites.
- [x] `src/components/layout/chat-list-pane.tsx` -- subscribe the fourth channel → store; `visibleFavorites` under the account filter; render `<FavoritesSection>` between the strip and the scroll; hydrate collapse on mount. -- surface the section.
- [x] `src/components/chat/chat-row.tsx` -- Favorite/Unfavorite context-menu item gated on `room.isFavourite`; one-time hint when favourites empty. -- per-row favorite control + discovery hint.
- [x] Tests -- `inbox.rs`: favourited rooms populate the Favorites window in recency order, excluded from Inbox/Archive, pins win over favourites, favourite+archived resolves to Favorites. `favorites-rooms.test.ts`: applyBatch/clear. `favorites-ui.test.ts`: toggle. `favorites-section.test.tsx`: renders in stream order, hidden when empty, collapse hides list + calls `setFavoritesCollapsed`, Unfavorite invokes `unfavoriteRoom`, click selects. `chat-row.test.tsx`: Favorite vs Unfavorite by `isFavourite` + invoke; hint shows only when favourites empty. `chat-list-pane.test.tsx`: fourth channel feeds the store; section renders in inbox view, hides when empty; collapse hydrated. -- cover behavior.

**Acceptance Criteria:**
- Given a chat in the Unified Inbox, when the user favourites it via the row context menu, then it leaves the chronological flow and appears in an always-visible FAVORITES section of compact 48 px rows between the Pins strip and the inbox scroll — reachable in one interaction from any scroll position; unfavouriting returns it to its correct chronological position, the four-way split computed in Rust and streamed, never derived in TypeScript (FR-21, UX-DR4, AD-20).
- Given favourite state, when the app restarts or the user re-logs in, then Favorites persist (the `m.favourite` server-side tag re-hydrates from sync) and the section's collapse/expand state persists (registry `favorites_collapsed`).
- Given no favourites yet, then the section is hidden entirely and a one-time hint appears in the chat-row context menu instead (UX-DR13).
- Given a code audit, then favourite state uses the Matrix `m.favourite` tag via `Room::set_is_favourite` (no keeper-local membership table), favouriting relies on the SDK's live notable-tag re-emit (no out-of-band merger poke), `.mark_as_read(`/`.send_single_receipt(` remain only in `signals.rs` (AD-14 guard green), and no inbox ordering/sectioning is done in TypeScript.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 1: (high 0, medium 1, low 0)
- reject: 13: (high 0, medium 0, low 13)
- addressed_findings:
  - `[low]` `[patch]` The chat-row Favorites discovery hint gated on `useFavoritesRoomsStore(s => s.total) ?? 0`, coercing the pre-load `null` total to `0`, so a user who actually has favourites could see the "zero favourites" hint flash on any row context-menu opened before the first Favorites batch streamed in. Dropped the `?? 0` so the hint requires a known-empty window (`total === 0`) and stays hidden while `total` is `null`; added a regression test asserting the hint is hidden pre-first-batch. `bun run check` green (biome + tsc + 499 vitest).
  - Deferred (1): a favourited chat has no unread/mention affordance anywhere in the inbox — the compact Favorites row is avatar + name only (per spec) and favourited rooms are removed from the Inbox window, so their unread state (the 4.1 bold-name/dot/mention cue) is invisible. Consistent with the spec-as-written and the Pins-strip avatar-only precedent, so logged for a UX-design decision rather than fixed here.
  - Rejected (13, by-design / pre-existing / noise): hint uses global favourites `total` while section visibility uses the account-filtered count (spec's hint rule is global — "while the user has zero favourites"); `total`-vs-`rooms.length` can diverge in the mirror store (by-design inbox-family pattern — the merger always emits a `Reset` with `Some(len)`, identical to `archive-rooms`/`pins-rooms`); `favoritesUiStore` not cleared on sign-out (the collapse setting is app-level in `keeper.db`, not per-user, so no cross-user bleed — hydrate re-confirms it on remount); collapse hydration race / remount re-hydrate (low-consequence UI-chrome race on a standard hydrate-on-mount idiom, self-healing on next mount); no Rust unit test for the `favourite_room`/`unfavourite_room` best-effort swallow + parse-error path (mirrors the untested-by-pattern `archive_room` template, shared plumbing); `get_favorites_collapsed` coerces any non-`"true"` value to expanded (safe default; the write side only ever stores `"true"`/`"false"`); four-sink `emit` fails-closed on any single channel closure (pre-existing shared-`closed`-latch from 4.2/4.3, already triaged by-design there — channels share one subscription lifecycle); favourite+archived+*unread* permutation untested (handled by the same `is_favourite` short-circuit already proven by the four-window and favourite+archived+read tests); `aria-current="true"` token (consistent with `pins-strip`/`chat-row`); `chat-row.test` seeds `total` via an empty-ops batch (exercises the by-design store contract); `hydrateFavoritesCollapsed` bare `catch` with no logging (intended graceful default for a best-effort UI read); rapid double-toggle persistence ordering (single-user, self-heals on next toggle/mount); concurrent favourite/unfavourite one-frame flicker across emits (inherent to Rust-authoritative streaming, self-corrects — shared with archive/pins).

## Design Notes

**Why the tag, not local (contrast with Pins):** matrix-sdk-base 0.18 `handle_notable_tags` maps `m.favourite`→`RoomNotableTags::FAVOURITE`. Because favourite is *notable*, a tag change updates `RoomInfo` and drives a room-list `VectorDiff::Set` — the merged stream re-emits live with no poke. This is exactly Archive's mechanism (`m.lowpriority`). Pins had to be keeper-local precisely because no standard pin tag is notable; Favorites must **not** copy that local machinery. `favourite_room` is therefore a two-line best-effort SDK call, and the merger stays a pure projection of the SDK stream.

**Four-window partition (extends 4.3's three), precedence Pins > Favorites > Archive/Inbox:**
```
merged (recency): [A pin, B fav, C archived-read, D unread, E read, F fav]
pins       = [A]                 // pinned (keeper-local order)
favorites  = [B, F]              // !pinned && is_favourite (recency order)
inbox      = [D, E]              // !pinned && !fav && (!archived || unread)
archive    = [C]                 // !pinned && !fav && archived && !read
```
Favourite and low-priority are SDK-mutually-exclusive (`set_is_favourite(true)` removes `m.lowpriority`), so `fav && archived` cannot persist; the `!is_favourite` guard on the archive/inbox predicates keeps the four windows strictly disjoint even under a transient sync state. Each window is a normal `InboxBatch` reusing `applyDiffOp`; the only new generated field is `InboxRoomVm.is_favourite`.

**Collapse persistence:** collapse/expand is pure UI chrome (not Matrix state), so it persists via the existing app-level `settings` key/value table in `keeper.db` (Story 2.6's `get_setting`/`set_setting`), which survives restart and sign-out/re-login — reached through two thin dedicated IPC commands rather than introducing a browser `localStorage` precedent (none exists in the codebase). Default when unset: expanded.

**One-time hint (UX-DR13):** interpreted as: while the user has zero favourites (`favourites total == 0`), the chat-row context menu shows a muted helper line by the Favorite item explaining the section; once any favourite exists the hint naturally disappears. This keeps the teaching affordance without a separate persisted "seen" flag.

## Verification

**Commands:**
- `bun run test:rust` -- cargo-nextest green; regenerated `InboxRoomVm.ts` includes `isFavourite`; new `inbox` four-window/precedence tests pass.
- `bun run bindings:check` -- no uncommitted drift under `src/lib/ipc/gen`.
- `bun run check:rust` -- rustfmt + clippy `-D warnings`; AD-14 guard `signals_is_the_sole_receipt_typing_gate` green (favourites touch no receipt/typing API).
- `bun run check` -- biome + tsc + vitest pass, including new favorites-section, favorites-rooms, favorites-ui, chat-row, and chat-list-pane tests.

## Auto Run Result

Status: done

**Summary:** Implemented Story 4.4 Favorites. Favorites ride the Matrix **`m.favourite`** tag — a *notable* tag (`RoomNotableTags::FAVOURITE`) in matrix-sdk-base 0.18 — so `Room::set_is_favourite` re-emits the room-list stream live and the state syncs cross-client. This makes Favorites architecturally a clone of Story 4.2 Archive (low-priority tag), **not** local like Story 4.3 Pins: `favourite_room`/`unfavourite_room` are best-effort two-line SDK calls with no out-of-band merger poke. The `InboxMerger` now partitions its single recency-ordered merge into **four** disjoint windows over one subscription (four Tauri channels) with precedence **Pins > Favorites > Archive/Inbox**: pins hoisted first, then `!pinned && is_favourite` → Favorites (recency order), then `!pinned && !is_favourite && (!is_archived || is_unread)` → Inbox, else Archive. Favourite and low-priority are SDK-mutually-exclusive (favouriting auto-clears archive), and the `!is_favourite` guard keeps the four windows strictly disjoint even under a transient sync state. `is_favourite` is read from `item.is_favourite()` (cached tag, no await) and carried through `to_inbox_room` like `is_archived`. The frontend adds a `favoritesRoomsStore` mirror, a `favorites-ui` collapse store, an always-visible labeled **FAVORITES** section of compact 48 px rows (collapse/expand chevron, per-row Unfavorite, hidden entirely when empty) between the Pins strip and the inbox scroll, and a per-row Favorite/Unfavorite context-menu item with a one-time discovery hint (UX-DR13). Collapse/expand persists via the app-level registry `settings` table (key `favorites_collapsed`), surviving restart and re-login; favourite membership re-hydrates from server sync. All ordering/sectioning stays Rust-authoritative (AD-20); favourites touch no receipt/typing API (AD-14 seam untouched).

**Files changed (code):**
- `src-tauri/crates/keeper-core/src/vm.rs` — `is_favourite` on `RoomVm` and `InboxRoomVm` (+ builders).
- `src-tauri/crates/keeper-core/src/account.rs` — read `item.is_favourite()`; `favourite_room`/`unfavourite_room` (best-effort `set_is_favourite`); `subscribe_inbox` takes `favourites_sink`.
- `src-tauri/crates/keeper-core/src/inbox.rs` — `favourites_sink` + four-window `emit` (pins/favorites/inbox/archive, `!is_favourite` disjointness guards); `is_favourite` through `to_inbox_room`; four-capture fixtures + golden/precedence/coexistence tests.
- `src-tauri/crates/keeper/src/ipc.rs`, `lib.rs` — fourth `inbox_subscribe` channel; `favourite_room`/`unfavourite_room`; `get_favorites_collapsed`/`set_favorites_collapsed` (registry `favorites_collapsed`) + registration.
- `src/lib/ipc/client.ts` — `subscribeInbox(…, onFavourites)`; `favoriteRoom`/`unfavoriteRoom`; `getFavoritesCollapsed`/`setFavoritesCollapsed`.
- `src/lib/stores/favorites-rooms.ts` (new), `src/lib/stores/favorites-ui.ts` (new).
- `src/components/layout/favorites-section.tsx` (new) — labeled section, collapse toggle, per-row Unfavorite, hidden when empty; `hydrateFavoritesCollapsed`.
- `src/components/layout/chat-list-pane.tsx` — fourth channel → store; `visibleFavorites` under the account filter; `<FavoritesSection>` between strip and scroll; collapse hydration on mount.
- `src/components/chat/chat-row.tsx` — Favorite/Unfavorite context-menu item + one-time hint (null-total-safe gating).
- `src/lib/ipc/gen/InboxRoomVm.ts`, `RoomVm.ts` — regenerated (`isFavourite`).
- Tests: `inbox.rs` (four-window + pins-over-favorites + favourite+archived→Favorites + leaves-inbox/returns), `favorites-rooms.test.ts`, `favorites-ui.test.ts`, `favorites-section.test.tsx`, `chat-row.test.tsx` (Favorite/Unfavorite + hint + pre-load hint-hidden), `chat-list-pane.test.tsx`; fixtures in `rooms.test.ts`/`archive-rooms.test.ts`/`pins-rooms.test.ts`/`pins-strip.test.tsx`/`use-sign-out.test.ts`.

**Review findings:** 1 patch applied (low: the discovery-hint gating coerced a pre-load `null` favourites `total` to `0`, risking a spurious hint flash before the window streamed in → require a known-empty `total === 0`, + regression test). 1 deferred (favourited rooms carry no unread/mention affordance anywhere in the inbox — consistent with the spec and the Pins avatar-only precedent; logged as a UX-design decision). 13 rejected as by-design / pre-existing / noise (global-vs-filtered hint signal; `total`-vs-`rooms.length` store split; app-level collapse setting not cleared on sign-out; hydrate-on-mount races; Rust swallow-path test parity with archive; safe collapse-value coercion; the shared fails-closed channel latch already triaged in 4.3; untested unread permutation subsumed by the `is_favourite` short-circuit; and assorted consistency/noise).

**Verification:** `bun run check:rust` (rustfmt + clippy `-D warnings`, AD-14 guard `signals_is_the_sole_receipt_typing_gate` green) — PASS; `bun run test:rust` (305 cargo-nextest; bindings regenerated) — PASS; `bun run check` (biome + tsc + 499 vitest, + core-tauri-free guard) — PASS. `bindings:check`'s `git status --porcelain` clause is satisfied once the regenerated `InboxRoomVm.ts`/`RoomVm.ts` are committed (done in this run's commit); the regeneration is idempotent and purely additive (`isFavourite`).

**Residual risks:** Favourite mutations are best-effort with no optimistic overlay (like Archive/Pins): a row moves window only when the merger re-emits — sub-frame on the SDK's live notable-tag re-emit; a genuinely-failed dispatch is a silent best-effort no-op. A favourited room not currently in any account's live SlidingSync window is not shown until it syncs (the same windowed-merge limitation already deferred for Pins/Inbox/Archive). Unread state of favourited rooms is not surfaced in the inbox view (deferred). Collapse/expand is app-level device chrome (single `favorites_collapsed` setting), not per-account.

**Follow-up review recommended:** false — the review pass applied a single localized, low-consequence frontend patch (null-safe hint gating) with a regression test; no behavior/API/security/data-shape change and no bad_spec/intent_gap. Below the bar for an independent follow-up review.
