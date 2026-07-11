---
title: 'Merged Full-Screen Search Surface'
type: 'feature'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: 'e612f41292935247b01704557b806718f7585127'
final_revision: '6ac444250a51b5cd3acf9e4e7c7b2ee952df2d53'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-13-context.md'
warnings:
  - oversized
---

<intent-contract>

## Intent

**Problem:** On the phone tier (< 768 px) there is no reachable Search. Story 13.3 shipped the Inbox-header magnifier as a stopgap that opens the desktop command palette (`commandPaletteStore.open()`), the Room ⋯ overflow has no "Search in chat", and the desktop `SearchOverlay` (⌘⇧F, message FTS) and `CommandPalette` (⌘K, chats/contacts/actions) are keyboard-summoned centered dialogs with no phone entry. ⌘K / global-search / in-chat-search must survive the trip to a keyboard-less device (FR-48, FR-34, FR-58, FR-60, UX-DR24).

**Approach:** Add one full-screen phone Search surface with segmented **Chats / Messages / Actions** scopes that reuses the exact desktop *engines* — `paletteQuery` for Chats+Actions, `searchArchive`/`buildSearchFilter`/`SearchResultList` for Messages — never forking their scoring, filtering, or behavior. Extract the desktop message-search body into a shared `SearchPanel` and the palette chat/action rows into shared row components so desktop and phone render byte-identical markup. Mount the surface always in `PhoneShell` (mirroring `LeadingDrawer`), driven by a tiny `searchSurfaceStore`. Repoint the header magnifier and wire a level-0 pull-down and a Room ⋯ "Search in chat" as its entry points.

## Boundaries & Constraints

**Always:**
- Reuse the desktop engines unchanged: Chats + Actions call `paletteQuery(needle, "default"|"action", openChat)`; Messages calls `searchArchive(buildSearchFilter(uiFilter, rooms))`. No scoring/filtering/ranking is re-implemented on the phone — Rust stays authoritative (AD-20).
- Message search reuses a single shared body: extract the `SearchOverlay` `DialogContent` body into `SearchPanel` (query field, filter chips, 200 ms debounce, out-of-order guard, honest "Searching your local archive" + offline header, `SearchResultList`, deep-link via `roomsStore.requestFocus`, export/approval shortcuts). Desktop `SearchOverlay` becomes a thin `Dialog` wrapper over `SearchPanel` with **byte-for-byte identical desktop behavior**.
- Chats + Actions reuse a single shared row source: extract `PaletteChatRow` (hue dot + display name + network badge) and the action row from `command-palette.tsx` into a shared module imported by both the desktop palette and the phone surface. `CommandPalette` keeps identical behavior.
- Typing `>` as the first character jumps to Actions scope and strips the `>` from the needle (reuse the `parseInput` `>`-prefix rule).
- In-chat search routes through the Room (level 1) ⋯ overflow → "Search in chat", opening the surface in Messages scope pre-filtered/locked to the open Chat (same lock semantics as desktop `scope: "chat"`).
- The surface is full-screen, focus-trapping, closes on Escape and an explicit back/close affordance, and returns focus to the element that opened it. Every tappable target ≥ 44 pt with an accessible name.
- Parity gate: the Actions scope surfaces exactly what the shared `paletteQuery(_, "action", _)` engine returns and dispatches each by id via `dispatchPaletteAction` — the phone registers **no** actions of its own, so every registered action is reachable and no dead entries appear (FR-48, FR-57).
- Chats select → `primaryViewStore.setView("inbox")` + `roomsStore.selectRoom(...)` + close (the phone stack pushes to Room). Messages activate → `roomsStore.requestFocus(...)` + close (pushes to Room; the conversation pane consumes `focusEvent` to scroll-to-match).
- Tier/gating flows only from `useShellLayout().phone`; never sniff platform or user-agent. Reduced motion (`prefers-reduced-motion: reduce`) renders the surface open/close and the pull-down as an instant cut.

**Block If:**
- `paletteQuery`, `searchArchive`, `buildSearchFilter`, `SearchResultList`, `roomsStore.requestFocus`/`selectRoom`, or `dispatchPaletteAction` are absent or renamed — the reuse contract is broken (HALT: missing search/palette engine).
- The palette action registry is discovered to contain a genuinely desktop-only action that the shared `paletteQuery` engine does **not** already gate out on iOS — hiding it would need registry/capability work outside this frontend story (HALT: action-registry capability gap). (Investigation found none today.)

**Never:**
- No forked/second search or palette component; no re-implemented debounce, scoring, filter-building, or deep-link logic. Do not restyle desktop search into a "mobile" second visual language beyond the full-screen container and touch sizing.
- No new IPC command, no Rust change, no new palette action, and no change to `searchStore` / `commandPaletteStore` desktop wiring or the desktop ⌘K / ⌘⇧F / ⌘F shortcuts.
- No bottom tab bar. No safe-area / keyboard-inset (`env(safe-area-inset-*)`, `--kb-inset`) handling — that is Story 13.5.
- Do not build pull-to-refresh: Story 13.4 owns only the pull-down-past-reveal-threshold-**opens-Search** leg of the shared gesture axis; the beyond-threshold pull-to-refresh continuation is Story 13.6.
- Do not change desktop/tablet (≥ 768 px) behavior; the extraction refactors must leave `app-shell` / `search-overlay` / `command-palette` desktop suites green.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Open via magnifier | phone, level 0, header magnifier tapped | full-screen surface opens in default **Chats** scope; query field focused | — |
| Open via pull-down | phone, level 0, Inbox list at `scrollTop === 0`, downward pull past reveal threshold on release | surface opens (Chats); below threshold → snap back, no open | pull when not at top → native scroll, no open |
| Open in-chat search | phone, level 1 Room, ⋯ overflow → "Search in chat" | surface opens in **Messages** scope locked to the open Chat (locked, non-removable Chat chip) | — |
| `>` prefix | any scope, query starts with `>` | scope switches to **Actions**, needle = text after `>` (trimmed) | — |
| Chats results | Chats scope, needle typed | `paletteQuery(needle,"default",openChat)` → contacts + chats rows (hue dot + name + network badge); select → open Chat (push to Room) + close | query error → empty list (no crash) |
| Messages results | Messages scope, needle typed | `searchArchive` results grouped by Chat with tinted matches, ≤ 100 ms/offline honesty header, filter chips; activate → deep-link into timeline at match + close | IpcError → inline honest "Search failed" (reused `SearchPanel`) |
| Actions results | Actions scope (or `>`), needle typed | `paletteQuery(needle,"action",openChat)` → action rows; select → `dispatchPaletteAction(id, selected)` + close | empty/short query → top registered actions |
| Empty query | any scope, blank needle | Chats/Actions show top actions/none per engine; Messages makes no call and shows the offline header only | — |
| Close | Escape, back/close affordance, or scrim | surface closes; focus returns to the opener (magnifier / overflow item) | — |
| Reduced motion | `prefers-reduced-motion: reduce` | open/close and pull-down render as cuts; all controls still function | — |
| Desktop tier | ≥ 768 px | surface never mounts; ⌘K/⌘⇧F/⌘F and the desktop dialogs behave byte-for-byte as before | — |

</intent-contract>

## Code Map

- `src/lib/stores/search-surface.ts` -- NEW: tiny always-mounted-overlay store (`isOpen`, `scope: "chats"|"messages"|"actions"`, `chatLock: RoomSelection | null`, `open({scope?, chatLock?})`, `close`), modeled on `src/lib/stores/leading-drawer.ts`.
- `src/components/layout/phone-search-surface.tsx` -- NEW: the full-screen merged surface (segmented scopes, shared query input, `>`→Actions, Chats/Actions via bare `Command` + shared rows, Messages via `SearchPanel`); mounted always in `PhoneShell`, focus-trapping, Escape/close, reduced-motion cut.
- `src/components/search/search-panel.tsx` -- NEW: message-search body extracted from `search-overlay.tsx` as `SearchPanel({ active, scope, chatLock, onClose })` — the single source of message-search behavior.
- `src/components/search/search-overlay.tsx` -- MODIFY: reduce to a thin `Dialog` wrapper reading `searchStore` and rendering `<SearchPanel active={isOpen} scope={scope} chatLock={…} onClose={close} />`; desktop behavior unchanged.
- `src/components/command-palette/palette-rows.tsx` -- NEW: shared `PaletteChatRow` + `PaletteActionRow` (`CommandItem`-based) extracted from `command-palette.tsx`.
- `src/components/command-palette/command-palette.tsx` -- MODIFY: import the extracted rows; no behavior change.
- `src/components/layout/phone-inbox-header.tsx` -- MODIFY: repoint the magnifier from `commandPaletteStore.getState().open()` to `searchSurfaceStore.getState().open()`.
- `src/components/layout/phone-header.tsx` -- MODIFY: add "Search in chat" to the level-1 ⋯ overflow → `searchSurfaceStore.getState().open({ scope: "messages", chatLock: selected })`.
- `src/components/layout/phone-shell.tsx` -- MODIFY: mount `<PhoneSearchSurface />` (like `<LeadingDrawer />`); add the level-0 pull-down-to-open-search gesture on the Inbox list, active only when the list is scrolled to top (mirror the 13.2/13.3 pointer-threshold pattern on the vertical axis); restore focus to the opener on close.
- `src/lib/ipc/client.ts` -- REUSE (read-only): `paletteQuery`, `searchArchive`, `resolveTimelineEventKey`, and the `PaletteChatVm`/`PaletteResultsVm`/`PaletteMode`/`SearchHitVm`/`SearchFilterVm` types.
- `src/lib/search-filter.ts` -- REUSE (read-only): `buildSearchFilter` + `SearchUiFilter`.
- `src/components/search/search-result-list.tsx` -- REUSE (read-only): `SearchResultList` (+ `groupHits`/`highlightSegments`).
- `src/components/command-palette/actions.ts` -- REUSE (read-only): `dispatchPaletteAction`.
- `src/lib/stores/rooms.ts` -- REUSE: `selectRoom`, `requestFocus`, `RoomSelection`, `FocusEvent`.
- `src/lib/stores/command-palette.ts` / `src/lib/stores/search.ts` -- REUSE (read-only): unchanged desktop stores.
- `src/hooks/use-shell-layout.ts` -- REUSE: `useShellLayout().phone`. `src/hooks/use-reduced-motion.ts` -- REUSE for the cut.
- `src/components/layout/phone-shell.test.tsx`, `phone-inbox-header.test.tsx` -- REUSE test patterns (`mockViewportWidth`, pointer `fireEvent`).

## Tasks & Acceptance

**Execution:**
- [x] `src/lib/stores/search-surface.ts` + `src/lib/stores/search-surface.test.ts` -- NEW zustand overlay store (`isOpen:false`, `scope:"chats"`, `chatLock:null`, `open`/`close`) following the `leading-drawer.ts` shape; `open({scope?, chatLock?})` sets both, `close` resets `chatLock`. Tests cover default state, open-with-scope, open-with-chatLock, and close-clears-lock -- one source of surface open/scope/lock state usable from the header, pull-down, and overflow.
- [x] `src/components/search/search-panel.tsx` + `src/components/search/search-panel.test.tsx` -- NEW: extract the message-search body from `search-overlay.tsx` verbatim into `SearchPanel({ active, scope, chatLock, onClose })` (query, filter chips, debounce → `searchArchive`, out-of-order guard, offline header, `SearchResultList`, `requestFocus` deep-link, export/approval buttons). Reset on `active` rising edge. Tests: debounced search fires the engine, results render + activate deep-links, chat-lock shows a locked chip, IpcError renders honestly -- the single reused message-search body.
- [x] `src/components/search/search-overlay.tsx` -- MODIFY: thin `Dialog` wrapper reading `searchStore`, rendering `<SearchPanel active={isOpen} scope={scope} chatLock={scope==="chat"&&selected?selected:null} onClose={close}/>`; keep the existing `search-overlay.test.tsx` green (adjust only if the extraction moves a query selector) -- desktop message search unchanged.
- [x] `src/components/command-palette/palette-rows.tsx` + `src/components/command-palette/palette-rows.test.tsx` -- NEW: export `PaletteChatRow` (glyph, `account-hue-dot`, display name, network `Badge`) and `PaletteActionRow` (⚡ glyph, title, `Kbd` shortcut) as `CommandItem`s. Tests: hue dot color, network badge only when non-null, shortcut only when set -- shared rows.
- [x] `src/components/command-palette/command-palette.tsx` -- MODIFY: import `PaletteChatRow`/`PaletteActionRow`; remove the local copies; behavior identical (keep `command-palette.test.tsx` green).
- [x] `src/components/layout/phone-search-surface.tsx` + `src/components/layout/phone-search-surface.test.tsx` -- NEW: full-screen overlay driven by `searchSurfaceStore`; segmented Chats/Messages/Actions control (≥ 44 pt each); one query input with `>`→Actions parsing; Chats/Actions render a bare `Command`/`CommandList` of shared rows fed by a debounced `paletteQuery` (out-of-order-guarded, `openChat` from `roomsStore.selected`); Messages renders `<SearchPanel active scope="chat"|"global" chatLock={store.chatLock} onClose={close}/>`; focus-trap + Escape/close + focus-return; `motion-reduce:*` cut. Tests: opens per store; scope switch; `>` jumps to Actions; Chats select opens a chat + closes; Actions select dispatches + closes; Messages activate deep-links + closes; opening with `chatLock` starts in Messages locked; reduced-motion cut; no tab bar.
- [x] `src/components/layout/phone-inbox-header.tsx` + update `phone-inbox-header.test.tsx` -- MODIFY: magnifier `onClick` → `searchSurfaceStore.getState().open()` (drop the `commandPaletteStore` import if now unused). Test asserts the magnifier opens the search surface (not the palette).
- [x] `src/components/layout/phone-header.tsx` -- MODIFY: add a "Search in chat" `DropdownMenuItem` (level 1, when `accountId`/`roomId` set) → `searchSurfaceStore.getState().open({ scope: "messages", chatLock: { accountId, roomId } })`, above/beside Export. Extend the header test for the new item.
- [x] `src/components/layout/phone-shell.tsx` + extend `phone-shell.test.tsx` -- MODIFY: mount `<PhoneSearchSurface />` after `<LeadingDrawer />`; add a level-0 pull-down-to-open-search gesture on the Inbox list container, active only at `scrollTop === 0`, opening the surface past a reveal threshold (reuse the pointer-threshold math on the vertical axis; below threshold snaps back); restore focus to the magnifier on surface open→closed. Tests: pull-down past threshold opens the surface; below-threshold/not-at-top no-ops; mounting the surface leaves the 13.2 back-swipe and 13.3 drawer gestures unregressed.

**Acceptance Criteria:**
- Given a phone Inbox (level 0), when the magnifier is tapped or the list is pulled down past the reveal threshold, then one full-screen Search surface opens with segmented Chats/Messages/Actions scopes on the reused engines and the same ≤ 100 ms/offline honesty as desktop, and no bottom tab bar exists.
- Given the Actions scope (or a `>`-prefixed query), then it surfaces exactly the shared `paletteQuery` action results and dispatches each via `dispatchPaletteAction` — every registered action is reachable and no dead/desktop-only entries appear.
- Given the level-1 Room ⋯ overflow → "Search in chat", then the surface opens in Messages scope locked to the open Chat, and message activation deep-links into that timeline at the matched event.
- Given a desktop/tablet viewport (≥ 768 px), then the surface never mounts and ⌘K / ⌘⇧F / ⌘F plus the desktop palette and search dialogs behave byte-for-byte as before (the extraction refactors are behavior-preserving).
- Given `bun run check`, then Biome + `tsc --noEmit` + vitest pass, including the new `search-surface` store, `search-panel`, `palette-rows`, `phone-search-surface`, and extended `phone-shell` / `phone-inbox-header` / `phone-header` suites, and the desktop `search-overlay` / `command-palette` suites stay green.

## Design Notes

**Merge two engines, fork neither.** The desktop already splits search into `paletteQuery` (chats/contacts/actions, `>`=action mode) and `searchArchive` (message FTS with filter chips + deep-link). The phone surface is a new *arrangement container* (like the 13.1 stack and the 13.3 drawer) that hosts both engines under one segmented input — the same "new container, reused internals" move 13.3 made mounting `SidebarPane` verbatim. To keep behavior single-sourced, the message-search body is extracted into `SearchPanel` (desktop `SearchOverlay` becomes a thin `Dialog` over it) and the palette rows into `palette-rows.tsx`; both extractions are behavior-preserving refactors guarded by the existing desktop suites.

**Scope routing.** `parseInput`'s `>`-rule is honored: a leading `>` forces Actions and strips itself from the needle. Chats/Actions feed a bare `Command`/`CommandList` (cmdk without the Dialog wrapper) so the shared `CommandItem` rows and ↑/↓/Enter nav come for free; Messages renders `SearchPanel` full-width. `openChat` for the palette query comes from `roomsStore.selected`, exactly as the desktop palette derives it.

**Entry points and the shared gesture axis.** The magnifier and the level-0 pull-down open Chats; the Room ⋯ "Search in chat" opens Messages locked to the Chat. The pull-down is only the *open-Search* leg — Story 13.6 extends the same continuous vertical axis into pull-to-refresh beyond the reveal threshold, so this story keeps the gesture minimal (open on release past threshold, snap back below) to leave that seam clean.

**Selection is the existing projection.** Chats select via `selectRoom` and Messages activate via `requestFocus` — both mutate the same selection state the phone stack already projects, so opening a result naturally pushes to the Room level with focus handled by the 13.2 push effect; the surface just closes.

## Verification

**Commands:**
- `bun run check` -- expected: Biome + `tsc --noEmit` + vitest all green, including the new `search-surface` store, `search-panel`, `palette-rows`, `phone-search-surface`, and extended `phone-shell`/`phone-inbox-header`/`phone-header` suites; desktop `search-overlay`/`command-palette`/`app-shell` suites unchanged and green.
- `bun run test -- phone-search-surface phone-shell phone-inbox-header phone-header search-panel search-overlay palette-rows command-palette search-surface` -- expected: the touched suites pass in isolation (jsdom matchMedia + pointer polyfills exercise the phone tier, the scopes, the `>` jump, and the pull-down).

## Spec Change Log

_No bad_spec loopback occurred; the spec was implemented as written._

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 2, low 1)
- defer: 2
- reject: 15
- addressed_findings:
  - `[medium]` `[patch]` Segmented scope tab appeared inert while a leading `>` was typed — the `>`-prefix rule (`parseNeedle`) forced Actions and overrode the tapped scope, so tapping Chats/Messages looked dead. Fixed: a scope tap now strips a stray leading `>` from the query so the explicit choice takes effect; added a `phone-search-surface` regression test.
  - `[medium]` `[patch]` The reused desktop `SearchPanel` hard-capped its results at `max-h-[50vh]`, wasting half the full-screen phone Messages view (nested scroll, empty lower half). Fixed additively: `SearchPanel` gained optional `className`/`resultsClassName` overrides (desktop defaults unchanged — byte-for-byte), and `PhoneSearchSurface` passes fill classes (`min-h-0 flex-1` / `max-h-none flex-1 min-h-0`) so results use the whole screen.
  - `[low]` `[patch]` Switching Chats↔Actions left the prior scope's rows visible during the 120 ms debounce window. Fixed: a scope-change effect clears the palette results/`hasResponded` immediately on scope change.

Rejected (15): desktop keydown host moved from `DialogContent` to the new `<search>` landmark (the wrapper covers the input+results; the only delta is focus-on-injected-X + arrow keys — negligible, and the `<search>` landmark is a minor a11y improvement); "phone Chats/Actions keyboard nav is broken" (false positive — the `InputGroupInput` sits inside `<Command>`, so ↑/↓/Enter bubble to cmdk's root handler and work); two self-withdrawn Blind Hunter findings (scroll container preserved; seq-guard symmetric); `runAction` closing before `await dispatchPaletteAction` (identical to the shipped desktop palette — pre-existing, not a regression); pull-down lacks axis-lock/live-preview and samples `atTop` once (by-pattern — a bounded 24 px `touch-none` band mirroring the discrete-threshold 13.2/13.3 gestures; on-device feel folds into Epic 14/15); focus-return on the level-1 "Search in chat" close path (radix restores focus to the ⋯ trigger); overlay scrim `bg-black/10` token (cosmetic — the content is opaque `inset-0`, scrim invisible); empty `paletteQuery` on open (by-spec parity with the desktop palette's top-actions); dual `autoFocus` (only one branch mounts at a time — never simultaneous); reopen seq-guard race (handled — reopen bumps the seq); pull-down firing while the drawer is mid-open (per-pointer mutually exclusive; rare); whitespace-only query (already guarded — no call, no false "no matches"); half-implemented `role="tablist"` ARIA (functional; the tests deliberately assert the tablist and a full APG tablist is more than a trivial patch — low-value churn for low consequence). The two defers (`open-search` action opening the desktop dialog on phone; the "on this Mac" copy on the phone panel) are recorded as DW-111 / DW-112 — both cross-cut Story 13.7 (capability gating / "On this iPhone" disclosure).

## Auto Run Result

Status: done

### Summary
Delivered the phone tier's single reachable Search: a full-screen, focus-trapping `PhoneSearchSurface` (a portalled radix `Dialog`, always mounted in `PhoneShell`, driven by a tiny `searchSurfaceStore`) with segmented **Chats / Messages / Actions** scopes that reuse the desktop engines and fork nothing. Chats/Actions feed a bare cmdk `Command` from `paletteQuery` (`default`/`action` mode, `openChat` from `roomsStore.selected`) rendered with shared `PaletteChatRow`/`PaletteActionRow`; a leading `>` jumps to Actions and strips itself. Messages renders the shared `SearchPanel` — the desktop `SearchOverlay`'s message-search body, extracted verbatim (desktop `SearchOverlay` is now a thin `Dialog` wrapper) — over `searchArchive`/`buildSearchFilter`/`SearchResultList` with the same filter chips, offline-honesty header, out-of-order guard, and `requestFocus` deep-link. Entry points: the Inbox header magnifier (repointed off the interim command palette), a level-0 pull-down-to-open gesture (armed only at the list's scroll-top, opens past a 64 px reveal threshold or flick; pull-to-refresh beyond stays Story 13.6's), and the Room ⋯ "Search in chat" overflow (opens Messages locked to the open Chat). Selecting a Chat pushes to the Room via `selectRoom`; activating a message deep-links via `requestFocus`; running an action dispatches via `dispatchPaletteAction`. Desktop/tablet ≥ 768 px is untouched — the two extractions are behavior-preserving and the desktop `search-overlay`/`command-palette` suites stay green. No bottom tab bar; no safe-area/keyboard (13.5) or pull-to-refresh (13.6) work.

### Files changed
- `src/lib/stores/search-surface.ts` — NEW always-mounted-overlay store (`isOpen`/`scope`/`chatLock`, `open({scope?, chatLock?})`/`close`), modeled on `leading-drawer.ts`.
- `src/lib/stores/search-surface.test.ts` — NEW default/open-scope/open-chatLock/close-clears-lock coverage.
- `src/components/search/search-panel.tsx` — NEW: message-search body extracted verbatim from `SearchOverlay` (query, chips, debounce, offline header, `SearchResultList`, deep-link); gained optional `className`/`resultsClassName` (desktop defaults unchanged) for the full-screen phone fill.
- `src/components/search/search-panel.test.tsx` — NEW: debounce/engine, deep-link, chat-lock chip, IpcError coverage.
- `src/components/search/search-overlay.tsx` — MODIFY: reduced to a thin `Dialog` wrapper over `SearchPanel`; desktop behavior byte-for-byte.
- `src/components/command-palette/palette-rows.tsx` — NEW: shared `PaletteChatRow` + `PaletteActionRow` extracted from `command-palette.tsx`.
- `src/components/command-palette/palette-rows.test.tsx` — NEW: hue dot / network badge / shortcut coverage.
- `src/components/command-palette/command-palette.tsx` — MODIFY: imports the shared rows; behavior identical.
- `src/components/layout/phone-search-surface.tsx` — NEW: the full-screen merged surface (segmented scopes, `>`→Actions with a stray-`>`-stripping tab tap, scope-change result reset, reduced-motion cut, focus-return).
- `src/components/layout/phone-search-surface.test.tsx` — NEW: scopes, `>` jump, select/dispatch/deep-link + close, chat-lock, and the `>`-tab-strip regression test.
- `src/components/layout/phone-inbox-header.tsx` (+ test) — MODIFY: magnifier repointed to `searchSurfaceStore.open()`.
- `src/components/layout/phone-header.tsx` (+ test) — MODIFY: added the Room ⋯ "Search in chat" item → Messages locked to the Chat.
- `src/components/layout/phone-shell.tsx` (+ test) — MODIFY: mount `<PhoneSearchSurface/>`; add the level-0 pull-down-to-open gesture and magnifier focus-return.

### Review findings breakdown
- Patches applied: 3 — the `>`-vs-tab inert control (medium), the `max-h-[50vh]` half-screen waste on the phone Messages scope (medium), and the cross-scope stale-result flash (low). See the Review Triage Log.
- Deferred: 2 — DW-111 (`open-search` action opens the desktop dialog on phone) and DW-112 ("on this Mac" copy on the phone panel); both are Story 13.7 (capability gating / "On this iPhone") territory.
- Rejected: 15 (false positives, by-pattern/by-spec parity, pre-existing, handled, or low-value churn). See the Review Triage Log.
- intent_gap: 0, bad_spec: 0.

### Follow-up review
`followup_review_recommended: false` — the three fixes are localized, well-understood UI changes (a query-string strip, an additive desktop-preserving layout prop, and an immediate result reset) with no API/security/data-model/behavior-re-derivation impact; the `SearchPanel` prop addition keeps desktop byte-for-byte. All verified green and the most user-visible fix is guarded by a new regression test.

### Verification
`bun run check` — green: Biome clean (292 files), `tsc --noEmit` clean, vitest **106 files / 1055 tests passed**, core-tauri-free convention check passed. Run independently by the main loop after implementation and again after the review patches. Desktop `search-overlay` / `command-palette` suites stayed green (behavior-preserving extractions).

### Residual risks
- Push/pop, the full-screen surface open/close animation, and the pull-down gesture feel are only fully verifiable on a real WKWebView/device; per the epic, on-device confirmation folds into the Epic 14/15 hardening + SM-8 dogfooding gate, not this story's acceptance.
- DW-111/DW-112: on the phone Actions scope, "Open Search" opens the desktop-style centered search dialog, and the Messages offline header reads "on this Mac" — both deferred to Story 13.7's capability-honest surface work.
- Safe-area / keyboard-inset padding (13.5) and pull-to-refresh beyond the reveal threshold (13.6) are intentionally out of scope; the pull-down deliberately implements only the open-Search leg to leave 13.6's seam clean.
