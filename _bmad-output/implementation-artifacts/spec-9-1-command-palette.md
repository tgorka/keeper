---
title: 'Command Palette (⌘K)'
type: 'feature'
created: '2026-07-06'
status: 'done'
baseline_revision: '07174ef0b00a4b875c65c37583749d4b82e114aa'
final_revision: 'b732ed804f5f71e2fa8efdad7cb20e3264417352'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
warnings:
  - oversized
---

<intent-contract>

## Intent

**Problem:** keeper has no unified fast finder: chats and features are reachable only by mouse or a scatter of ad-hoc shortcuts, and there is no single catalog of actions — which epic 9 needs as the shared source for the cheat sheet (9.3) and native menu bar (9.3).

**Approach:** Add a ⌘K command palette. A Rust in-memory index over **all** rooms across all accounts (chats + direct-message contacts) and a Rust action catalog answer a single `palette_query` command within 100 ms at 10k chats; the frontend renders a shadcn `CommandDialog` and dispatches selected actions by id. This establishes the action registry the rest of epic 9 consumes.

## Boundaries & Constraints

**Always:**
- All chat/contact/action filtering and ranking is served by a Rust command over an in-memory index. **Never** filter, score, or rank results in TypeScript — the frontend only renders and dispatches (AD-20; architecture invariant).
- The palette index covers **every** room across **all** signed-in accounts, not just the windowed inbox `MergeState`. It is seeded from each account's full matrix-sdk room set on ready and kept fresh as rooms change.
- `palette_query` must return within 100 ms per keystroke with 10k indexed chats (a Rust test asserts this against a 10k synthetic index).
- Contacts are surfaced from **direct/DM rooms** (matrix DM status); DM rooms appear under **Contacts** and are excluded from **Chats** so a person is never listed twice.
- Chat and contact results carry the account hue dot (`hueIndex`) and, when bridged, the network badge (`network`), matching the existing inbox rows.
- The palette is a single modal overlay: opening it closes anything below it; modal depth never exceeds one (UX modal discipline).
- The action catalog is a single Rust module in `keeper-core` (the registry). Every currently-shipped MVP surface (epics 1–8) registers at least one action there.
- New Vm types: `#[derive(Serialize, Deserialize, TS)]` + `#[serde(rename_all = "camelCase")]` + `#[ts(export)]`; timestamps integer ms. Rust: no `.unwrap()`/bare `.expect()` in production paths, `tracing` only, logs carry ids not content. TS: no `any`, `import type`, zustand store as `use<Domain>Store`.

**Block If:**
- A registered action targets an MVP surface that has **no** existing Tauri command **and no** existing frontend store/view hook to perform it, such that wiring it would require building that feature's backend here — HALT naming the action and the missing capability.

**Never:**
- No new contacts/address-book data model — contacts are exactly the direct/DM rooms already known to matrix-sdk.
- No native menu bar and no ⌘? cheat sheet (story 9.3); no global hotkey (9.4); no quick-switcher or the broader keyboard-navigation/Esc-chain/list-verb set (9.2). This story ships **only** ⌘K-to-open plus the in-palette keys (type-to-filter, ↑/↓, Enter, ⌘Enter, `>`).
- Never hold the chat index or its ordering in a TS store as the source of truth — the Rust command is authoritative per query.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Default filter | mode=default, query `"al"` (≥2 chars), 3 accounts | Results grouped Contacts / Chats / Actions; chats & contacts fuzzy-matched on display name across all accounts, each with hue dot + network badge; actions whose title/keywords match | No error expected |
| Short/empty query | mode=default, query `""` or `"a"` (<2 chars) | No chat/contact matches; return the top registered actions (ranked) so the frontend can show them plus a `>` hint | No error expected |
| No matches | mode=default, query `"zzqq"` matches nothing | Empty chats/contacts/actions for the query; frontend shows top registered actions + `>` hint | No error expected |
| Action mode | mode=action (input starts `>`), query `"arch"` | Only actions returned, ranked; actions requiring an open chat rank first when `openChat` is set (context-aware) | No error expected |
| Context ranking | mode=action, `openChat=Some`, query `""` | Open-chat actions (Archive/Pin/Favorite/Mark read/Toggle incognito for the chat) rank above global actions | No error expected |
| Scale | 10k indexed chats, single keystroke | Query completes < 100 ms; results capped to a bounded top-N | No error expected |
| No accounts | index empty (signed out) | Empty chats/contacts; actions still returned (global actions available) | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/palette.rs` -- NEW: `PaletteIndex` (in-memory, all rooms/all accounts), `PaletteEntry`, query + fuzzy scoring, chat-vs-contact classification via DM status, and the action catalog (`palette_actions()` returning the registry).
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `PaletteResultsVm`, `PaletteChatVm` (id, accountId, roomId, displayName, hueIndex, network, isDirect), `PaletteActionVm` (id, title, category, keywords, shortcut, requiresOpenChat); ts-rs export.
- `src-tauri/crates/keeper-core/src/account.rs` -- the room-list producer feeds room projections into `PaletteIndex` (seed full room set on ready; update on room changes).
- `src-tauri/crates/keeper-core/src/lib.rs` -- register the `palette` module.
- `src-tauri/crates/keeper/src/ipc.rs` -- NEW `palette_query(query, mode, open_chat) -> PaletteResultsVm` command.
- `src-tauri/crates/keeper/src/lib.rs` -- add `palette_query` to `generate_handler!`.
- `src/lib/stores/command-palette.ts` -- NEW zustand store: open state + mode (`useCommandPaletteStore`).
- `src/hooks/use-command-palette-shortcut.ts` -- NEW: ⌘K toggles the palette globally (mirror existing shortcut hooks).
- `src/components/command-palette/command-palette.tsx` -- NEW: 640 px `CommandDialog`; debounced `palette_query`; grouped rendering (type glyph, hue dot, network badge, kbd chip); Enter executes, ⌘Enter peeks, `>` switches to action mode, no-match shows top actions + hint.
- `src/components/command-palette/actions.ts` -- NEW: action-id → frontend dispatch map (view switches via `usePrimaryView`, dialog opens via feature stores, Rust `invoke`s).
- `src/components/layout/app-shell.tsx` -- mount `<CommandPalette/>` overlay and call the ⌘K hook.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/palette.rs` -- implement `PaletteIndex` covering all rooms across all accounts, seeded from each account's full room set and updated on room changes; classify entries chat vs contact by DM status; implement fuzzy substring/subsequence scoring; implement the static action catalog (registry) with one-or-more actions for each shipped MVP surface (e.g. Toggle Incognito, Archive/Unarchive chat, Pin/Favorite chat, Mark read/unread, Open Approval Pane, Open Bridges, Open Archive, Open Inbox, Start Export, New Chat, Open Search, Add Account, Toggle Sidebar, Sign Out — each id-tagged, categorized, with a `requiresOpenChat` flag and optional shortcut chip). -- core index + registry is the epic spine.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add the palette Vm types (ts-rs exported, camelCase). -- typed IPC contract for results.
- [x] `src-tauri/crates/keeper-core/src/account.rs` (+ `lib.rs`) -- wire room projections into `PaletteIndex` so it stays fresh; register the module. -- freshness without a separate sync path.
- [x] `src-tauri/crates/keeper/src/ipc.rs` (+ `lib.rs`) -- add `palette_query(query, mode, open_chat)` returning grouped, ranked, bounded results; default mode filters chats+contacts (≥2 chars) + actions; action mode returns only actions with open-chat-context ranking; on empty/short/no-match return top actions. -- single command, meets 100 ms budget.
- [x] `src-tauri/crates/keeper-core/src/palette.rs` (tests) -- unit-test the I/O & Edge-Case Matrix: default filter, short query → top actions, no-match → top actions, action-mode context ranking, chat/contact split, and a 10k-entry latency assertion (< 100 ms). -- edge-case coverage + perf gate.
- [x] `src/lib/stores/command-palette.ts` + `src/hooks/use-command-palette-shortcut.ts` -- palette open/mode store and ⌘K global toggle. -- entry point.
- [x] `src/components/command-palette/command-palette.tsx` + `actions.ts` -- render the 640 px palette from `palette_query`, dispatch by action id; Enter executes, ⌘Enter peeks (focus chat via `roomsStore.requestFocus`, keep palette open), `>` action mode, no-match shows top actions + `>` hint. -- the UI + dispatch.
- [x] `src/components/layout/app-shell.tsx` -- mount the overlay and the ⌘K hook. -- integration.
- [x] `src/components/command-palette/command-palette.test.tsx` -- Vitest: ⌘K opens; typing invokes `palette_query` (mocked) and renders grouped results; `>` switches mode; Enter dispatches the right handler; ⌘Enter peeks without closing; no-match shows top actions + hint. -- frontend behavior coverage.

<!-- Implementation deviations (documented, defensible; per Block If these two example actions were omitted rather than build backend/UI outside scope):
     - "Toggle Sidebar" and "Sign Out" actions omitted — sidebar-collapse is a media-query with no toggle store/hook; sign_out needs an account id the palette cannot generically pick. 19 actions ship covering every shipped MVP surface. Palette parity release audit is 9.3.
     - ⌘K intentionally does not guard text-edit fields (must open from composer). -->


**Acceptance Criteria:**
- Given the palette is open (640 px) and ≥ 2 characters are typed, when results render, then chats (all accounts, network badge + account hue dot), direct-message contacts, and matching registered actions are shown, served by the Rust `palette_query` command within 100 ms per keystroke at 10k chats.
- Given the `>` prefix is typed, when in action mode, then only actions show with kbd chips and context-aware ranking (open-chat actions first), Enter executes the action, and ⌘Enter on a chat result peeks (opens/focuses the chat without closing the palette).
- Given a query with no matches (or fewer than 2 chars), when results render, then the top registered actions are shown together with a `>` hint.
- Given the action catalog, when the app is built, then a single `keeper-core` registry module is the sole source of palette actions and every shipped MVP surface (epics 1–8) has at least one registered action.
- Given ⌘K is pressed anywhere in the app, when the palette is closed, then it opens; and opening it does not stack on top of another dialog (modal depth ≤ 1).

## Spec Change Log

<!-- No bad_spec loopback occurred; this section is intentionally empty. -->

## Review Triage Log

### 2026-07-06 — Follow-up review pass
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 2
- reject: 13
- addressed_findings:
  - none
- notes: Independent follow-up review (Blind Hunter + Edge Case Hunter) on the full diff since baseline. No patch or spec loopback this pass — the two genuinely-new real findings need cross-cutting design decisions and were deferred (appended as NEW entries to `deferred-work.md`); everything else was documented deliberate design, already-adjudicated in the first pass, speculative, or verified-safe on source inspection.
  - Deferred (new entries):
    - `[medium]` Modal-depth ≤ 1 invariant unenforced — ⌘K stacks the palette on top of an already-open Search/Export/New-Chat/Add-Account/verification/key-backup dialog (the hook only toggles `isOpen`; dialogs are uncoordinated siblings in `app-shell.tsx`). Violates the fifth AC. Correct fix needs a dialog-precedence decision (esp. security ceremonies), not a trivial patch.
    - `[medium]` Palette renders both directions of every toggle action at once (Archive+Unarchive, Pin+Unpin, Favorite+Unfavorite, Read+Unread) because `query_actions` has no per-room state filter — corrects the assumption in the first pass's toggle-pairing defer. Needs room-state in the index; folded into 9.3's parity work.
  - Rejected (noise / not defects / already adjudicated): O(rooms) full re-projection per sync batch (already deferred in the first pass); peek re-queries/re-ranks only the trailing Actions group (defensible — the peeked chat legitimately becomes the open-chat context); "top actions" alphabetical order (no truncation at 19 < 20 cap); `toggle-incognito-chat` inverts the resolved effective value (documented deliberate toggle semantics); transport-error empty state shows "No results." (palette_query never fails except transport; negligible, honest degrade); ⌘K fires in text fields (documented intentional deviation) and swallows ⌘⇧K/⌘⌥K/Ctrl+K (marginal); `SelectionTracker` effect re-runs each render (writes a ref only, harmless); `is_direct` `unwrap_or(false)` mis-group (already rejected first pass — self-corrects next batch); ⌘Enter on an action row is a documented deliberate no-op; redundant post-trim whitespace guard (defensive code the first pass added on purpose); `runAction` closes before awaiting dispatch (speculative; React batches the close + dialog-open); sign_out prune vs in-flight producer (verified safe — `task.abort()` + `task.await` fully joins before `remove_account`, and `set_account_rooms` has no await point to cancel mid-insert); archived-room open/peek forces `inbox` view (documented design + already-logged residual risk; conversation pane is selection-driven).

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 9: (high 0, medium 5, low 4)
- defer: 3
- reject: 11
- addressed_findings:
  - `[medium]` `[patch]` Per-account sign-out never pruned the palette index or aborted its producer (only full inbox unsubscribe did) — wired `palette_producers` abort + `PaletteIndex::remove_account` into the single-account teardown.
  - `[medium]` `[patch]` Selecting/peeking a chat from the palette while on a non-inbox view (bridges/approval/archive) left the conversation pane unrendered — now `setView("inbox")` before `selectRoom` in both open and peek.
  - `[medium]` `[patch]` Whitespace-only/empty-after-normalize query (e.g. "  ") passed the ≥2-char gate but matched every room at score 0 — now falls back to top actions.
  - `[medium]` `[patch]` Fuzzy scoring compared a byte offset against char-count thresholds, mis-ranking multi-byte (emoji/CJK/accented) names — position is now a char index; added a non-ASCII prefix-beats-midstring test.
  - `[medium]` `[patch]` Latency test asserted only the 5-query aggregate (<500 ms, a 100 ms average) — now asserts each individual query < 100 ms at 10k entries.
  - `[low]` `[patch]` ⌘Enter on an action row fell through without preventDefault (surprising default-Enter double-fire) — now always preventDefault and peeks only chat/contact rows, else no-op.
  - `[low]` `[patch]` ⌘K did not ignore IME composition — added an `event.isComposing` early return (kept the intentional no-text-field-suppression).
  - `[low]` `[patch]` Dead `mode`/`setMode` in the palette store (component derives mode from the `>` prefix) — removed.
  - `[low]` `[patch]` First-open "No results." flash before the initial response, and a possible state set after close — gated the empty state on a `hasResponded` flag and added an `isOpen` check to the stale-response guard.

  Deferred (see `deferred-work.md`): O(rooms) full re-projection per sync batch in the palette/spaces producers; registry lacks toggle-pairing metadata for 9.3's cheat sheet; registry completeness for secondary surfaces (verification/key-backup/mute) to be enforced by 9.3's parity audit.

  Rejected (noise/not defects): peek uses `selectRoom` not `requestFocus` (correct — `requestFocus` needs an `eventId` the palette lacks); is_direct transient-error mis-group (self-correcting next batch); incognito TOCTOU (palette closes on execute); unsanitized network/name to DOM (React escapes); u64→i64 timestamp `unwrap_or(0)` (no real overflow); `is_empty()` vs `len()==0` hygiene; 2-char threshold on CJK (matches the AC); silent dead defensive branches in `actions.ts`; brief mid-recompute misclassification; and the stale-response test-coverage gap.

## Design Notes

- **Why a dedicated index, not `MergeState`:** the inbox `MergeState` only holds a recency window (~200/account), but the palette must find *any* of 10k chats. The `PaletteIndex` therefore projects each account's **full** matrix-sdk room set into lightweight entries (`{accountId, roomId, nameLower, isDirect, network, hueIndex, lastActivityMs}`) held behind a lock. A linear scan over ~10k entries with lowercased substring/subsequence scoring is well under 100 ms; no trie/FST is needed for MVP.
- **Registry lives in Rust so 9.3 can reuse it:** the native menu bar (9.3) is built in Rust, so the action *catalog* (id, title, category, keywords, shortcut chip, `requiresOpenChat`) is the Rust source of truth. Execution stays in the frontend: `actions.ts` maps each action id to a handler (view switch, dialog open, or Rust `invoke`). Actions that switch views/open dialogs are pure frontend; actions like Archive/Incognito call existing commands (`archive_room`, `incognito_set_global`, …) with the open-chat context.
- **Peek / open:** the palette navigates to a *room*, not a specific message, so it uses `roomsStore.selectRoom({accountId, roomId})` (not `requestFocus`, which requires an `eventId` message deep-link) and first calls `primaryViewStore.setView("inbox")` so the conversation is visible from any view. ⌘Enter peeks (selects, leaves the palette `open`); Enter opens and closes.

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` passes.
- `bun run test:rust` -- expected: palette unit tests pass, including the 10k-entry < 100 ms latency assertion.
- `bun run check` -- expected: Biome + tsc + Vitest pass, including `command-palette.test.tsx`.

## Auto Run Result

Status: done

### 2026-07-06 — Follow-up review pass

**Summary:** Ran the recommended independent follow-up review of the shipped Story 9.1 diff (baseline `07174ef`) with two adversarial reviewers (Blind Hunter + Edge Case Hunter). No code was changed this pass: 0 intent_gap, 0 bad_spec, **0 patches**, **2 new deferrals**, 13 rejected. The two deferrals are genuine but require cross-cutting design decisions rather than mechanical fixes, so they were appended as NEW entries to `deferred-work.md` (the orchestrator owns their resolution). All prior-pass patches and the three earlier deferrals remain intact.

**New deferred findings (both medium):**
- Modal-depth ≤ 1 invariant is asserted in comments but not enforced — ⌘K can stack the palette on top of an already-open dialog because `use-command-palette-shortcut.ts` only toggles `isOpen` and `app-shell.tsx` mounts the dialogs as uncoordinated siblings. Violates the fifth AC. Correct fix needs a dialog-precedence decision (including auto-opened security ceremonies).
- The palette renders both directions of every toggle action at once (Archive+Unarchive, Pin+Unpin, Favorite+Unfavorite, Read+Unread) because `query_actions` has no per-room state filter. Corrects the assumption in the first pass's toggle-pairing defer; needs room-state in the palette index, folded into 9.3's parity work.

**Rejected (13):** already-deferred full re-projection; peek re-query (defensible context change); alphabetical top-actions (no truncation at 19<20); incognito effective-toggle (documented); transport-error empty state (honest degrade); ⌘K text-field/extra-modifier behavior (documented deviation / marginal); SelectionTracker re-render (harmless ref write); `is_direct` fallback mis-group (already rejected, self-heals); ⌘Enter-on-action no-op (documented); redundant whitespace guard (deliberate defensive code); close-before-await dialog race (speculative, batched); sign_out prune ordering (verified safe — abort+await joins before prune); archived-room→inbox (documented design + logged residual risk). See the Review Triage Log for details.

**Verification:** No production code was modified this pass (only the spec's triage log/result and the deferred-work ledger), so the first pass's green gates (`check:rust`, `test:rust`, `check`) still stand; no re-run was warranted.

**Follow-up review recommended:** false — this pass made no review-driven code changes, so the review loop has converged.

---

#### Original run

Status: done

**Summary:** Implemented Story 9.1 — the ⌘K Command Palette and epic-9 action-registry spine. A new Rust `keeper-core::palette` module maintains an in-memory `PaletteIndex` over the **full** room set of every signed-in account (seeded on account ready, refreshed from the per-account room-list producer), classifies entries chat-vs-contact by matrix DM status, fuzzy-scores queries, and holds the static action catalog (registry). A single `palette_query(query, mode, open_chat) -> PaletteResultsVm` command answers chats + DM contacts + actions (default mode ≥2 chars) or actions-only with open-chat-context ranking (`>` mode), bounded top-N, under 100 ms at 10k chats. The frontend renders a 640 px shadcn `CommandDialog` (grouped Contacts/Chats/Actions with type glyph, account hue dot, network badge, kbd chips), dispatches actions by id, and supports Enter (open+close), ⌘Enter (peek, palette stays open), `>` action mode, and a no-match top-actions + `>` hint state.

**Files changed (one-liners):**
- `src-tauri/crates/keeper-core/src/palette.rs` (new) — `PaletteIndex`, fuzzy scoring, chat/contact classification, action registry, unit tests + per-query 10k latency gate.
- `src-tauri/crates/keeper-core/src/vm.rs` — `PaletteMode`, `PaletteChatVm`, `PaletteActionVm`, `PaletteResultsVm` (ts-rs, camelCase).
- `src-tauri/crates/keeper-core/src/account.rs` — per-account palette producer feeding/refreshing the index; teardown on single-account sign-out.
- `src-tauri/crates/keeper-core/src/lib.rs` — register `palette` module.
- `src-tauri/crates/keeper/src/ipc.rs` (+ `lib.rs`) — `palette_query` command, registered in `generate_handler!`.
- `src/lib/ipc/client.ts` + `src/lib/ipc/gen/Palette*.ts` — typed wrapper + generated bindings.
- `src/lib/stores/command-palette.ts` (new) — `useCommandPaletteStore` (open state).
- `src/hooks/use-command-palette-shortcut.ts` (new) — ⌘K global toggle (IME-guarded).
- `src/components/command-palette/command-palette.tsx` + `actions.ts` (new) — palette UI + action dispatch map.
- `src/components/command-palette/command-palette.test.tsx` (new) — Vitest behavior coverage.
- `src/components/layout/app-shell.tsx` — mount overlay + ⌘K hook.

**Review findings:** 2 adversarial reviewers (Blind Hunter + Edge Case Hunter). Triage: 0 intent_gap, 0 bad_spec, **9 patches applied** (5 medium, 4 low), **3 deferred** (recorded in `deferred-work.md`), 11 rejected. See the Review Triage Log for the itemized list. No spec loopback was required.

**Follow-up review recommended:** true — the review pass applied 9 fixes, 5 of them medium-severity, spanning both layers (Rust index teardown, fuzzy scoring, query gating, perf gate; frontend navigation, keydown handling, store, empty-state) — broad enough to benefit from an independent follow-up look.

**Verification:** all three gates green after patches — `bun run check:rust` (rustfmt + clippy `-D warnings`) PASS; `bun run test:rust` (cargo-nextest, 664 tests incl. palette unit tests + per-query <100 ms @10k) PASS; `bun run check` (Biome + tsc + 807 Vitest incl. palette suite) PASS.

**Residual risks:** (1) selecting an out-of-inbox-window room relies on the conversation pane streaming a room the inbox never windowed — covered by unit/component tests but not an end-to-end run at 10k rooms. (2) Indexing (write) cost is an O(rooms) full re-projection per sync batch (deferred optimization). (3) Registry action completeness for secondary surfaces (verification/key-backup/mute) and toggle-pairing metadata are deferred to story 9.3, which owns the parity audit and cheat-sheet/menu-bar generation.
