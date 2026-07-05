---
title: 'Search UI — Global and In-Chat'
type: 'feature'
created: '2026-07-05'
baseline_revision: 'f66ac5be56f20dee1315cd1886d75446be3ad1f2'
status: 'done'
review_loop_iteration: 0
final_revision: 'bf04a41084e7f2109c235344d0498b7847ed28d4'
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Story 5.3 shipped the offline FTS engine and the `searchArchive` IPC command (`SearchFilterVm` → `SearchHitVm[]`), but there is no UI: no way for a user to run a query, filter it, read grouped results, or jump to a matched message. The epic's promise — "finding beats scrolling, even offline" (FR-34, UX-DR13) — is unrealized.

**Approach:** Add a single React search surface driven off the existing `searchArchive` binding, opened two ways: **global** (`⌘⇧F`, all accounts) and **in-chat** (`⌘F`, scoped to the open Chat). It offers query text + filter chips (sender, Chat, Network, Account, date range), groups results by Chat with matched terms tinted in the `search-highlight` token, shows the honest header "Searching your local archive", and degrades to "No matches in your archive." with one-tap-removable chips. Selecting a result opens the containing Chat and deep-links to the matched message with a 2 s tint. Because the "Network" filter is a live per-room label and the engine only takes `room_ids`, the frontend resolves a Network/Chat selection to its room-id set from the merged room store before calling. Deep-linking reuses the existing `event_id → render key` resolution: a thin new command maps a hit's `eventId` (sanctioned input) to the timeline item's opaque `unique_id`, and the frontend drives the existing scroll+highlight machinery — no event id is ever added to a timeline view model.

## Boundaries & Constraints

**Always:**
- One surface for both entry points. `⌘⇧F` opens it **global** (no room lock); `⌘F` opens it **in-chat** scoped to the currently-selected Chat (`roomIds=[selected.roomId]`, `accountIds=[selected.accountId]`, Chat chip shown and locked). `⌘F` is a no-op when no Chat is open. Both preventDefault (⌘F is the webview's native find). Esc closes; the surface is fully keyboard-operable (open → type → arrow through results → Enter → Esc), no pointer required.
- Filters build a `SearchFilterVm` in a **pure, unit-tested** helper: `query` verbatim; a **Chat** selection → that room's id in `roomIds`; a **Network** selection → the set of `roomId` from the merged room store where `room.network === selected` (deduped, all accounts); an **Account** selection → its id in `accountIds`; **sender** → `SearchFilterVm.sender` (exact Matrix user id — the engine matches `events.sender = ?`); a **date range** → `startTs` (start-of-day ms) / `endTs` (end-of-day-inclusive ms). Empty account/room lists mean unrestricted. Enumerate Chats/Networks/Accounts from the existing `roomsStore.rooms` (`InboxRoomVm`), `networksStore.networks`, and `accountsStore.accounts`.
- Results group by Chat keyed `(accountId, roomId)`; each group's title + account attribution resolve from the matching `InboxRoomVm` (`displayName`, `hueIndex`); when the hit's room is outside the merged window, fall back to a neutral label (e.g. the room id) — never crash. Cross-account result identity is disambiguated by the account **hue dot** (`accountHueVar(hueIndex)`) + account `userId` in the result meta (FR-24). Matched query terms inside each hit `body` are wrapped in the `search-highlight` background tint (`bg-search-highlight` / `text-search-highlight-foreground`) — background only, never borders or text color.
- Query input is **debounced** before calling `searchArchive`; an in-flight query superseded by a newer keystroke must not overwrite fresher results (guard against out-of-order resolution). The header always reads "Searching your local archive" and an offline note stays visible in the loaded, empty, and no-result states (search works fully offline).
- Deep-link on result activation: open the Chat (switch to inbox view if needed, `selectRoom({accountId, roomId})`), record a pending focus `(accountId, roomId, eventId)`, and let the conversation pane resolve the `eventId` to the timeline `unique_id` via the new `resolveTimelineEventKey` command; on hit, scroll it into view and apply the `search-highlight` tint for **2 s** (2000 ms); when not yet loaded, best-effort bounded `paginateBackwards` and retry; if still unreachable (deep history not yet served — archive-first pagination is Story 5.6), leave the Chat open and surface an honest, non-blocking note — **never a wrong jump and never a silent no-op**. Clear the pending focus once handled.
- Rust: the resolver takes `event_id` as **input** and returns the opaque `unique_id` — event ids are never added to any streamed timeline VM (the `TimelineItemVm` no-event-id invariant, NFR-9/AD-1, holds). `open_timeline_for` + `items()` scan mirrors the existing reply/edit/reaction command pattern; no `.unwrap()`/bare `.expect()`; `?` + `thiserror` (`AccountError` → `CoreError` → `IpcError`); `tracing` with ids not content. TS: no `any`, `import type`, generated types re-exported from `gen/`.

**Block If:**
- Delivering an in-timeline deep-link with a 2 s highlight is impossible without either exposing `event_id` on a streamed `TimelineItemVm` (violating NFR-9/AD-1) **and** the input-only resolver + existing scroll machinery cannot locate a currently-loaded event by id. (Expected resolvable: `open_timeline_for(...).items()` carries each event item's `event_id()`, exactly as the reply index already uses.)

**Never:**
- No export (Story 5.5), no archive-first / seek-to-event pagination (Story 5.6 — deep history beyond a bounded live paginate degrades honestly here), no sign-out/delete-archive (5.7). No command palette (`⌘K`) or cheat-sheet (`⌘?`) — those are separate surfaces; only wire `⌘⇧F`/`⌘F`.
- No changes to the FTS engine, `SearchFilterVm`/`SearchHitVm`, or `searchArchive` (5.3 is frozen). No new backend member/contact enumeration (sender stays an exact Matrix-id field fed by result-derived suggestions — no member-list fetch).
- No `event_id`, mxc, key, or crypto material added to any timeline stream VM. No Matrix/search logic in TypeScript — the frontend only calls `searchArchive`, resolves filters from already-streamed view models, and renders.
- No holding search results in a store as a source of truth beyond the surface's own lifetime; close discards them.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Open global | `⌘⇧F` | Surface opens (no room lock), header "Searching your local archive", offline note, empty query → no call | — |
| Open in-chat | `⌘F` with a Chat open | Surface opens scoped: `roomIds`/`accountIds` locked to that Chat, Chat chip shown+locked | `⌘F` with no Chat open ⇒ no-op |
| Query ≥3 | user types `"hello"` | debounced `searchArchive`; hits grouped by Chat, terms tinted with `search-highlight` | stale (superseded) response discarded |
| Query <3 | user types `"hi"` | still calls (engine LIKE fallback); same UI | — |
| Network filter | user picks Network `"Telegram"` | resolves to that Network's `roomId` set from `roomsStore`; passed as `roomIds` | Network with 0 rooms in window ⇒ `roomIds` empties the result set (honest empty) |
| Account/sender/date filters | chips set | `accountIds`/`sender`/`startTs`/`endTs` built; empty lists ⇒ unrestricted | end date maps to end-of-day inclusive |
| No results | query with 0 hits | "No matches in your archive." + active chips each one-tap removable + offline note stays visible | — |
| Enter on result | result focused, its event loaded in timeline | Chat opens; row scrolled into view + `search-highlight` tint 2 s | — |
| Deep-link not loaded | event not in loaded window | bounded `paginateBackwards` + retry; on hit → jump+tint | unreachable ⇒ Chat open + honest note, no wrong jump / no silent no-op |
| Cross-account identity | same contact via 2 accounts | two distinct result groups, each with hue dot + account `userId` meta | — |
| Search error | `searchArchive` rejects (`IpcError`) | honest inline error; offline note intact; surface stays usable | retriable flag respected in copy |
| Resolver | `resolveTimelineEventKey(acct,room,evt)` | `unique_id` when the event is a loaded timeline item, else `null` | invalid event/room id ⇒ mapped `IpcError`, not a panic |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/account.rs` -- NEW `resolve_timeline_event_key(account_id, room_id, event_id) -> Result<Option<String>, AccountError>`: `open_timeline_for` (same accessor as `submit_reply`/`toggle_reaction`), parse the event id, scan `timeline.items()` for the event item whose `event_id()` matches → `Some(unique_id().0)`, else `None`. Input-only; no VM change.
- `src-tauri/crates/keeper/src/ipc.rs` (+ `keeper/src/lib.rs`) -- command `resolve_timeline_event_key(state, account_id, room_id, event_id) -> Result<Option<String>, IpcError>`; exhaustive error mapping; registered in `generate_handler!`.
- `src/lib/ipc/client.ts` -- binding `resolveTimelineEventKey(accountId, roomId, eventId): Promise<string | null>`.
- `src/lib/stores/search.ts` -- NEW vanilla-zustand store: `isOpen`, `scope: "global" | "chat"`, `open(scope)`, `close()` + `useSearchStore` hook (mirrors `primary-view.ts` shape).
- `src/lib/stores/rooms.ts` -- add a deep-link focus channel: `focusEvent: { accountId; roomId; eventId } | null`, `requestFocus(f)` (also selects the room), `clearFocus()`; consumed by the conversation pane.
- `src/lib/search-filter.ts` -- NEW pure helper: `buildSearchFilter(ui, rooms)` → `SearchFilterVm` (Chat→roomId, Network→roomId set, Account→accountId, sender exact, date-range→ms bounds, chat-scope lock); unit-tested.
- `src/components/search/search-overlay.tsx` -- NEW surface: `Dialog` container, query `InputGroup`, filter chips (`Badge`), debounced query→`searchArchive`, stale-response guard, header/offline note, no-results state, keyboard nav, Enter→`requestFocus` + `close`. Reads `roomsStore`/`networksStore`/`accountsStore`.
- `src/components/search/search-result-list.tsx` -- NEW: results grouped by Chat `(accountId, roomId)`, group header (Chat displayName + hue dot + account `userId`), per-hit row with term-highlight (`bg-search-highlight`), redacted marker; window-miss fallback label.
- `src/hooks/use-search-shortcuts.ts` -- NEW: `⌘⇧F` → `open("global")`, `⌘F` → `open("chat")` only when a Chat is selected; `preventDefault`; ad-hoc keydown listener (existing pattern).
- `src/components/layout/app-shell.tsx` -- mount `<SearchOverlay />` and call `useSearchShortcuts()`.
- `src/components/layout/conversation-pane.tsx` -- consume `roomsStore.focusEvent`: when set for the open room, `resolveTimelineEventKey` → jump+`search-highlight` tint 2 s (parameterize the existing `onJumpTo` to accept the search tint + 2000 ms), with bounded `paginateBackwards` retries and an honest non-blocking "further back in history" fallback; then `clearFocus()`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- `resolve_timeline_event_key` via `open_timeline_for` + `items()` event-id scan returning the opaque `unique_id` -- input-only deep-link resolution, no VM/invariant change.
- [x] `src-tauri/crates/keeper/src/ipc.rs` (+ `lib.rs`) -- `resolve_timeline_event_key` command, exhaustive `IpcError` mapping, registered -- IPC surface.
- [x] `src/lib/ipc/client.ts` -- `resolveTimelineEventKey` binding -- typed frontend access.
- [x] `src/lib/stores/search.ts` (new) + `src/lib/stores/rooms.ts` (focusEvent) -- overlay open/scope state + deep-link focus channel -- surface + jump plumbing.
- [x] `src/lib/search-filter.ts` (new) -- pure `buildSearchFilter` (Chat/Network/Account/sender/date-range → `SearchFilterVm`, chat-scope lock, empty ⇒ unrestricted) -- filter construction + Network→roomIds resolution.
- [x] `src/components/search/search-overlay.tsx` + `search-result-list.tsx` (new) -- the surface, debounced+stale-guarded search, grouped tinted results, header/offline/no-results/error/cross-account states, keyboard nav, Enter→deep-link -- the search UI.
- [x] `src/hooks/use-search-shortcuts.ts` (new) + `src/components/layout/app-shell.tsx` -- `⌘⇧F`/`⌘F` wiring (chat-only gating, preventDefault) + mount overlay -- entry points.
- [x] `src/components/layout/conversation-pane.tsx` -- consume focusEvent: resolve→scroll+`search-highlight` tint 2 s, bounded paginate retry, honest unreachable fallback -- deep-link landing.
- [x] Tests: `src/lib/search-filter.test.ts` (filter/Network-resolution matrix), `src/components/search/search-overlay.test.tsx` (grouping, tint, no-results+removable chips+offline note, header, cross-account meta, Enter→focus, IpcError), `src/hooks/use-search-shortcuts.test.ts` (global vs chat-only + preventDefault); Rust: `account.rs` inline test for event-id parse/error mapping (the live-timeline items-scan shares the no-mock-timeline-harness limitation already logged for reply/reaction error paths — note in Design Notes) -- covers the I/O matrix.

**Acceptance Criteria:**
- Given the global search surface (`⌘⇧F`), when the user types a query and adds sender/Chat/Network/Account/date-range chips, then results group by Chat with matches tinted in the `search-highlight` token, the header states "Searching your local archive", and it works fully offline (FR-34).
- Given a result, when the user presses Enter, then keeper opens the containing Chat and deep-links to the matched message highlighted for 2 s; when the message is not in loaded history it is best-effort paginated to, and if unreachable the Chat opens with an honest note rather than a wrong or silent jump (FR-34).
- Given an open Chat, when the user presses `⌘F`, then the same engine runs scoped to that Chat from the same surface, and a no-result state shows "No matches in your archive." with each active filter chip removable one-tap and the offline note kept visible (UX-DR13).
- Given the same contact reached via two accounts, when results render, then the two are disambiguated by account hue dot + account identity in the result meta (FR-24); and no `event_id` is added to any streamed timeline view model (NFR-9/AD-1 preserved).

## Design Notes

**Deep-link without breaking the no-ids invariant.** `TimelineItemVm::Message` explicitly forbids event ids ("never … an event id (AD-4, NFR-9)"), so the hit's `eventId` cannot be matched against streamed items on the frontend. Instead the resolver takes `event_id` as **input** (already sanctioned — `SearchHitVm` returns it for exactly this deep-link) and returns the opaque `unique_id`, reusing `open_timeline_for(...).items()` (each event item exposes `event_id()`, the same source the reply `event_id → unique_id` index is built from). The frontend then drives the existing `onJumpTo(key)` scroll + a tint — no new id ever flows outward on the timeline stream.

**Honest degrade for deep history.** Search's value is finding old messages, but archive-first / seek-to-event pagination is Story 5.6; here the timeline only serves its loaded window plus bounded live `paginateBackwards`. So the deep-link lands exactly when the event is reachable and otherwise opens the Chat with a plain "this message is further back in history" note — consistent with keeper's honest-state ethos and never a wrong jump. (Log a deferred-work item: full seek-to-event deep-link completes with 5.6.)

**Network filter lives above the engine.** A Network is a live per-room bridge label (`InboxRoomVm.network`), not an archive column; the tauri-free engine only filters `room_ids`. `buildSearchFilter` resolves a Network (and Chat) selection to its room-id set from the already-merged `roomsStore.rooms` before calling — keeping the engine pure and offline-capable (matches Story 5.3's Design Note).

**Sender is exact-match.** The engine does `events.sender = ?` (a full Matrix user id). With no frontend member list, the sender chip is a text field seeded by **result-derived suggestions** (the distinct `sender` values already present in the current hits) — usable without a member-list fetch and honest about exact-match semantics.

## Verification

**Commands:**
- `bun run check` -- expected: biome + tsc + vitest green, incl. `search-filter`, `search-overlay`, and `use-search-shortcuts` tests; new generated binding (if any) typechecks.
- `bun run check:rust` -- expected: `cargo fmt --check` + `clippy --all-targets -- -D warnings` clean (no `.unwrap()`, no warnings).
- `bun run test:rust` -- expected: cargo-nextest green incl. the resolver's event-id parse/error test; ts-rs regen leaves timeline VMs unchanged (no event id added).
- `bun run check:all` -- expected: full gate (frontend + rust + build) green; `bindings:check` passes.

## Spec Change Log

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 2, low 2)
- defer: 1
- reject: 11
- addressed_findings:
  - `[medium]` `[patch]` The search deep-link's once-guard (`handledFocusRef`) was set to the `account|room|event` key and never released, so re-activating the **same** search hit a second time returned early — no scroll, no tint, no note — a silent no-op that directly violates the spec's "never a silent no-op". Now released in the landing's `.finally` once the attempt completes (the in-flight window is still guarded), so an identical re-request re-lands; the same block's `clearFocus` was also tightened to compare the full `account|room|event` identity (not just `eventId`) so a newer focus for a different Chat that coincidentally shares an event id can't be dropped.
  - `[medium]` `[patch]` The debounced search effect listed the live merged `rooms` array in its deps, so every streamed inbox batch (which replaces the array) re-fired `searchArchive` after the debounce while the overlay was open on a busy multi-account inbox — redundant IPC and result churn. `rooms` is now read at call time via `roomsStore.getState().rooms` and dropped from the deps, so the search re-runs only on query/filter-selection changes.
  - `[low]` `[patch]` In the deep-link landing, a resolver **hit** (event loaded in the timeline) whose row was not yet painted fell through to `paginateBackwards` — paging older history that cannot help an already-loaded event — and could surface a premature "further back in history" note. The `key !== null` branch now retries the DOM jump a few times as React commits, and only degrades (never paginates) on a persistent paint-miss; pagination is reserved for the genuinely-not-loaded (`key === null`) case.
  - `[low]` `[patch]` `startOfDayMs` guarded only against `NaN`, but `new Date(y, m-1, d)` silently normalizes rollover inputs (month 13, day 45, Feb 30) into a valid-but-wrong day, so a malformed `YYYY-MM-DD` produced a wrong time bound instead of `null`. Added a component round-trip check so any rollover/malformed date maps to `null`; extended `search-filter.test.ts` to assert it.
  - Rejected as by-design / unreachable / negligible (11): malformed **event id** → `RoomNotFound` label (can't happen for a real hit — the engine only returns valid event ids); a transient `paginateBackwards` reject ending the bounded landing (best-effort bounded is spec-sanctioned; the note stays honest); the `EMPTY_MATCH_ROOM_ID` sentinel being "fragile" (it is a **bound** parameter in the frozen engine's `room_ids IN (…)` — SQLite never parses the value, so it simply cannot match); Chat + Network chips unioning `roomIds` (an unusual combination; union is a defensible reading and no correctness bug); `⌘F`/`⌘⇧F` intercepting native find even inside inputs (spec-mandated always-`preventDefault` app-level shortcut, standard for `⌘K`-class bindings); a pending debounce `setTimeout` firing just after close (the effect cleanup already `clearTimeout`s it and the seq guard discards any stray response); unmemoized `toLocaleString` / bad-timestamp (`SearchHitVm.timestamp` is a typed number from a real hit — cosmetic); an in-chat overlay silently widening to global if the selected Chat vanishes mid-search (needs the selection to clear while the overlay is open — niche, and `buildSearchFilter`'s chatLock precedence keeps it functionally scoped until then); switching scope while the overlay is already open not resetting stale query/chips (chatLock precedence overrides the chips in the built filter, so the residue is cosmetic and the transition is uncommon); `key={hit.eventId}` "colliding across groups" (false positive — hits render as per-group siblings and 5.3 dedups per `(account, root)`, so event ids are unique within a group's list); `startDate > endDate` not flagged (the frozen engine returns an honest empty result for an inverted range — the 5.3 I/O matrix's sanctioned behavior).

## Auto Run Result

Status: done

**Summary:** Delivered Story 5.4 — the Search UI over the frozen Story 5.3 offline FTS engine (FR-34, UX-DR13, DESIGN search-highlight). A single React surface opens two ways — global (`⌘⇧F`, all accounts) and in-chat (`⌘F`, scoped + locked to the open Chat) — offering a debounced query plus filter chips (sender, Chat, Network, Account, date range), results grouped by Chat with matched terms tinted in the `search-highlight` token, the honest header "Searching your local archive", an offline note kept visible across loaded/empty/no-result states, and a "No matches in your archive." empty state with one-tap-removable chips. Cross-account result identity is disambiguated by an account hue dot + `userId`. A pure, unit-tested `buildSearchFilter` translates the chips into a `SearchFilterVm` — resolving a Network (and Chat) selection to its `roomId` set from the merged room store, exact-match sender, local-day date bounds, and an honest empty-set sentinel for a zero-room Network. Selecting a result opens the containing Chat and deep-links to the matched message with a 2 s `search-highlight` background tint; deep-linking uses a NEW input-only resolver command (`resolve_timeline_event_key`: event id in → opaque `unique_id` out) that reuses the existing `open_timeline_for(...).items()` scan and the `onJumpTo` scroll machinery, so **no event id is ever added to a streamed timeline view model** (NFR-9/AD-1 invariant preserved). A target beyond the loaded window is best-effort paginated to (bounded); if still unreachable it degrades to an honest in-Chat note (full seek-to-event completes with Story 5.6) — never a wrong jump, never a silent no-op.

**Files changed:**
- `src-tauri/crates/keeper-core/src/account.rs` — new `resolve_timeline_event_key` (parse event id → scan open timeline items → opaque `unique_id`; input-only, no VM change).
- `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` — `resolve_timeline_event_key` command with exhaustive `IpcError` mapping, registered in `generate_handler!`.
- `src/lib/ipc/client.ts` — `resolveTimelineEventKey(accountId, roomId, eventId): Promise<string | null>` binding.
- `src/lib/stores/search.ts` (new) — overlay open/scope store; `src/lib/stores/rooms.ts` — `focusEvent` deep-link channel (`requestFocus`/`clearFocus`).
- `src/lib/search-filter.ts` (new) — pure `buildSearchFilter` (Chat/Network→roomIds, exact sender, local-day bounds, chat-scope lock, empty-Network sentinel).
- `src/components/search/search-overlay.tsx` + `search-result-list.tsx` (new) — the surface (debounced+stale-guarded search, header/offline/no-results/error/cross-account states, keyboard nav, Enter→deep-link) and grouped tinted result list.
- `src/hooks/use-search-shortcuts.ts` (new) + `src/components/layout/app-shell.tsx` — `⌘⇧F`/`⌘F` wiring (chat-only gating, preventDefault) + overlay mount.
- `src/components/layout/conversation-pane.tsx` — deep-link landing (resolve→scroll→2 s search tint, bounded paginate retry, honest fallback; parameterized jump helper).
- Tests: `src/lib/search-filter.test.ts`, `src/components/search/search-overlay.test.tsx`, `src/hooks/use-search-shortcuts.test.ts`, plus a Rust inline resolver parse/error test.

**Review findings:** 2 reviewers (adversarial Blind Hunter + Edge Case Hunter). Triage: 0 intent_gap, 0 bad_spec, 4 patch (medium 2, low 2), 1 defer, 11 reject. Patches: (1) the deep-link once-guard was never released, so re-activating the same hit was a silent no-op — now released after each attempt (+ full-identity `clearFocus`); (2) the debounced search re-fired on every streamed inbox batch — `rooms` now read at call time and dropped from effect deps; (3) a resolver-hit-but-not-yet-painted row wasted pagination rounds and could show a premature note — now retries the DOM jump instead of paginating; (4) `startOfDayMs` accepted rollover dates (month 13, Feb 30) as wrong bounds — now round-trip-validated to `null` (with a new test). Deferred: a deep-link beyond the loaded window + bounded paginate awaits Story 5.6's archive-first seek. Rejects were by-design/unreachable/negligible (incl. a false-positive React-key "collision" — hits are per-group siblings and 5.3 dedups per `(account, root)`); see the Review Triage Log.

**Verification:** `bun run check:rust` → `cargo fmt --check` + `clippy --all-targets -D warnings` clean. `bun run test:rust` → cargo-nextest 386/386 (incl. the resolver test). `bun run check` → biome + tsc + vitest 574/574 green (+23 new tests across the four suites) + core-tauri-free invariant holds. `bun run check:all` → full gate green incl. `bindings:check` (no `gen/` diff — no timeline VM changed, no new exported type; the resolver command uses plain string params/return). Post-patch re-run of `bun run check` re-confirmed 574/574 green; patches were TS-only so the Rust gates are unaffected.

**Residual risks:** (1) Deep-link seek to deep history is bounded by live pagination until Story 5.6's archive-first seek lands (spec-sanctioned honest degrade; deferred). (2) The sender filter is exact-match on a full Matrix user id, seeded by result-derived suggestions — no member-list picker (by design; no frontend member enumeration exists). (3) The empty-Network sentinel relies on the frozen engine treating an unmatchable bound `room_ids` value as "no rows" (safe against a parameterized `IN` clause; would need revisiting only if the 5.3 engine ever validated room-id syntax).
