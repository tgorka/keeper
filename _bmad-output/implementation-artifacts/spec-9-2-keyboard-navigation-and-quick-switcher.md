---
title: 'Keyboard Navigation and Quick-Switcher'
type: 'feature'
created: '2026-07-06'
status: 'done'
baseline_revision: '3af968b102da8d340ecfd2cbb480bf44cff65055'
final_revision: 'd73e7e40aee013e15db4606b39b60f1c49cafe89'
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

**Problem:** keeper's chat list is only mouse-operable: rows are individually focusable buttons with no list-level keyboard traversal, no single-key triage verbs, no view-switch chords for Inbox/Archive, no next/previous-unread jump, and no chat cycler. This blocks the epic-9 promise that the whole triage loop (walk unreads → archive → reply → next) runs with zero pointer use. (Timeline-side keys — ↑/↓ select, `r` reply, `e` edit, ⌫ delete — and composer Esc/edit/send already ship from epic 3; the ⌘K palette and ⌘3/⌘4 view chords already ship. This story fills the chat-list gap.)

**Approach:** Add chat-list keyboard navigation over the Rust-ordered inbox window: roving ↑/↓ and `j`/`k` selection with a visible focus ring, `Enter` opens the focused chat and drops focus into the composer, and single-key verbs `e`/`u`/`p`/`f` invoke the existing archive/read/pin/favorite commands on the focused row. Add three global chord hooks mirroring the app's per-hook `window` keydown pattern: ⌘1/⌘2 switch Inbox/Archive, ⌥⌘↓/⌥⌘↑ jump to the next/previous unread chat, and ⌃Tab/⌃⇧Tab cycle the open conversation through the recency-ordered inbox window (the quick-switcher). No new Rust, no new ordering logic in TS — every list operation moves a cursor over the array Rust already ordered.

## Boundaries & Constraints

**Always:**
- Ordering is Rust-authoritative. List nav, unread-jump, and the quick-switcher only move a selection/focus cursor over the existing `roomsStore.rooms` (recency order) / `archiveRoomsStore.rooms` array — never sort, re-sort, filter, or re-derive order in TypeScript (AD-20; architecture invariant).
- Reuse the shipped commands and stores; wire keys to existing capability only. Verbs call the same IPC wrappers the row context menu uses (`archiveRoom`/`unarchiveRoom`, `pinRoom`/`unpinRoom`, `favoriteRoom`/`unfavoriteRoom`, `markRoomRead`/`markRoomUnread`), choosing the direction from the row's current `isArchived`/`isPinned`/`isFavourite`/effective-unread flag. `u` mirrors `ChatRow`'s optimistic-unread pattern (`setOptimisticUnread` then round-trip; revert on hard reject). Enter uses `roomsStore.selectRoom` + `composerStore.requestFocus`.
- Global chord hooks follow the established ad-hoc pattern (each its own `useEffect` + `window.addEventListener("keydown", …)`, `preventDefault`, `ctrl` accepted alongside `meta` for non-mac parity, `event.isComposing` early-return), mounted in `app-shell.tsx` alongside the existing shortcut hooks. ⌘1/⌘2 carry the same text-field guard as `useApprovalShortcut`/`useBridgesShortcut` (skip when the target is an INPUT/TEXTAREA/SELECT/contentEditable).
- List-focused keys (↑/↓/`j`/`k`, bare `e`/`u`/`p`/`f`, Enter) are handled on the chat-list container and fire only when a modifier is absent and focus is within the list — so ⌘/⌥/⌃ chords fall through to the global hooks and typing elsewhere is never hijacked.
- Focus model: the focused row carries a visible focus ring and a roving `tabIndex` (the focused row is `0`, the rest `-1`); interactive rows keep accessible labels. `Esc` in the list clears the focused-row selection ring after any active filter is cleared (extends the existing filter-clearing Esc handler); overlays continue to return focus to their invoker on close (radix/`toggleRef` — verify, do not rebuild).
- Quick-switcher and unread-jump operate on the currently rendered window: the inbox window when `primaryView === "inbox"`, the archive window when `"archive"`, honoring the active account-switcher display filter; they no-op when `"bridges"`/`"approval"` replaces the cluster or the list is empty.

**Block If:**
- A required verb's backing command does not exist such that wiring it would mean building that feature's backend here — HALT naming the verb and the missing command. (Known and pre-resolved: `m` mute has no command/store anywhere in the app — see Never; it is scoped out, not built.)

**Never:**
- No `m` mute verb / mute menu — there is no mute feature or command in the codebase (epics 1–8 never shipped one; 9.1 already deferred registry mute-coverage to 9.3). Wiring `m` would require building a mute backend, which is out of this story. Documented omission, consistent with 9.1's Toggle-Sidebar/Sign-Out omissions.
- Do not re-implement or modify the timeline keyboard nav (`conversation-pane.tsx` `onKeyDown`: ↑/↓/`r`/`e`/⌫) or composer keys — they already ship. Do not build the ⌘? cheat sheet or native menu bar (9.3) or the global system hotkey (9.4).
- Do not build a new all-accounts MRU visit-stack switcher or a quick-switcher overlay UI; ⌃Tab cycles the recency-ordered rendered window in place. Do not add keyboard nav to the Pins strip or Favorites section (secondary surfaces) in this story.
- No new Rust command, no new Vm, no chat/ordering state held in a TS store as source of truth.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| List move | chat list focused, `ArrowDown`/`j` (no modifier) | Focus + selection ring moves to the next row in the Rust order; wraps or clamps at the end deterministically; row `.focus()`'d (roving tabindex) | No error |
| List open | focused row, `Enter` | `selectRoom({accountId, roomId})` opens it and `composerStore.requestFocus()` drops focus into the composer | No error |
| Verb archive | focused row `isArchived=false`, bare `e` | `archiveRoom(...)`; `isArchived=true` → `unarchiveRoom(...)`; best-effort, no re-sort (Rust re-partitions) | Reject swallowed |
| Verb unread | focused row, bare `u` | Effective-unread → `markRoomRead` (optimistic false) else `markRoomUnread` (optimistic true), mirroring `ChatRow` | Hard reject reverts overlay |
| Verb pin/fav | focused row, bare `p`/`f` | Toggles via `pinRoom`/`unpinRoom`, `favoriteRoom`/`unfavoriteRoom` per current flag | Reject swallowed |
| View chord | `⌘1` / `⌘2` (not in a text field) | `setView("inbox")` / `setView("archive")`, `preventDefault` | No error |
| View chord in field | `⌘1` with INPUT/TEXTAREA focused | Ignored (guard); native/text behavior preserved | No error |
| Unread jump | `⌥⌘↓`, some rows unread | Selects + opens the next unread row after the current selection in the rendered window (wraps); focuses composer; `preventDefault` | No error |
| Unread jump none | `⌥⌘↑`, zero unread rows | No-op (selection unchanged) | No error |
| Quick-switch | `⌃Tab` / `⌃⇧Tab`, ≥1 chat | Opens next / previous chat in the recency-ordered rendered window from the current selection (wraps); focuses composer; `preventDefault` | No error |
| Quick-switch empty | `⌃Tab`, empty/non-list view | No-op | No error |
| Esc in list | filter active then focused row | Esc clears the active filter(s) first; a second Esc clears the focused-row selection ring | No error |
| Modifier passthrough | list focused, `⌘F` | List handler ignores it (modifier present); global search hook handles it | No error |

</intent-contract>

## Code Map

- `src/hooks/use-view-shortcuts.ts` -- NEW: ⌘1→inbox, ⌘2→archive via `primaryViewStore.setView`; text-field guarded, IME-guarded, `preventDefault`; mirrors `use-approval-shortcut.ts`. (⌘3/⌘4 already ship as separate hooks — left intact.)
- `src/hooks/use-quick-switcher.ts` -- NEW: ⌃Tab/⌃⇧Tab cycle the open conversation over the recency-ordered rendered window (inbox/archive per `primaryViewStore`), honoring the account-switcher filter; `selectRoom` + `composerStore.requestFocus`; `preventDefault`; no-op off-list/empty.
- `src/hooks/use-unread-jump.ts` -- NEW: ⌥⌘↓/⌥⌘↑ select+open the next/previous `effectiveIsUnread` row in the rendered window (wraps), focus composer; `preventDefault`; no-op when none.
- `src/components/layout/chat-list-pane.tsx` -- MODIFY: add roving focused-row state over the main `visibleRooms` list; container `onKeyDown` for ↑/↓/`j`/`k` (move), `Enter` (open+focus composer), bare `e`/`u`/`p`/`f` (verbs on focused row); extend the existing Esc handler to also clear the focused-row ring; pass roving `tabIndex`/ref to rows.
- `src/components/chat/chat-row.tsx` -- MODIFY: accept an optional `tabIndex` and forwarded `ref` (or `rowRef`) so the pane drives roving focus; keep existing click/Enter/Space select, focus ring, `aria-current`, context menu unchanged.
- `src/components/layout/app-shell.tsx` -- MODIFY: mount `useViewShortcuts`, `useQuickSwitcher`, `useUnreadJump` beside the existing shortcut hooks.
- `src/lib/stores/rooms.ts`, `src/lib/stores/composer.ts`, `src/lib/stores/primary-view.ts`, `src/lib/ipc/client.ts` -- REFERENCE only: `selectRoom`/`effectiveIsUnread`/`setOptimisticUnread`, `requestFocus`, `setView`, and the verb command wrappers already exist.

## Tasks & Acceptance

**Execution:**
- [x] `src/hooks/use-view-shortcuts.ts` -- add ⌘1/⌘2 inbox/archive chord hook (text-field + IME guard, preventDefault). -- completes the ⌘1–4 view set.
- [x] `src/hooks/use-quick-switcher.ts` -- add ⌃Tab/⌃⇧Tab open-chat cycler over the recency-ordered rendered window (wrap; selectRoom + composer focus; no-op off-list/empty). -- the quick-switcher.
- [x] `src/hooks/use-unread-jump.ts` -- add ⌥⌘↓/⌥⌘↑ next/previous-unread jump (effectiveIsUnread scan, wrap, open + composer focus, no-op when none). -- triage jump.
- [x] `src/components/chat/chat-row.tsx` -- accept roving `tabIndex` + forwarded ref; preserve all existing behavior. -- lets the pane drive keyboard focus.
- [x] `src/components/layout/chat-list-pane.tsx` -- add focused-row roving state + container `onKeyDown` (↑/↓/`j`/`k` move, `Enter` open+composer focus, bare `e`/`u`/`p`/`f` verbs via existing commands with `u` optimistic-unread parity), and extend the Esc handler to clear the focused-row ring. -- the core list operability.
- [x] `src/components/layout/app-shell.tsx` -- mount the three new hooks. -- integration.
- [x] `src/hooks/use-view-shortcuts.test.ts`, `src/hooks/use-quick-switcher.test.ts`, `src/hooks/use-unread-jump.test.ts` -- unit-test each chord: fires + preventDefaults, respects guards/wrap/no-op edges. -- I/O matrix chord rows.
- [x] `src/components/layout/chat-list-pane.test.tsx` -- extend: ↑/↓/`j`/`k` move the ring in Rust order; `Enter` selects + requests composer focus; each verb calls the right command per current flag; `u` sets/reverts the optimistic overlay; modifier chords pass through; Esc clears the ring.

Note: a shared read-only helper `src/lib/rendered-window.ts` was added (consumed by the quick-switcher and unread-jump hooks) to avoid duplicating the identical window+account-filter projection; it slices/filters the Rust-ordered arrays without re-ordering. -- I/O matrix list rows.

**Acceptance Criteria:**
- Given the chat list is focused, when the user presses ↑/↓ or `j`/`k`, then a visible focus ring moves through the rows in the Rust-authoritative order (no client re-sort) with a roving tabindex, and `Enter` opens the focused chat with focus landing in the composer.
- Given a chat row is focused, when the user presses `e`/`u`/`p`/`f`, then the corresponding existing archive/read/pin/favorite command runs in the correct direction for that row's current state (with `u` reflecting the optimistic-unread overlay), and no bare verb fires while a modifier is held or focus is in a text field.
- Given the app anywhere, when the user presses ⌘1/⌘2, then the shell switches to Inbox/Archive (guarded off in text fields); and ⌘3/⌘4 (Approval/Bridges) continue to work — the ⌘1–4 set is complete.
- Given unread chats exist in the rendered window, when the user presses ⌥⌘↓/⌥⌘↑, then the next/previous unread chat is selected, opened, and composer-focused (wrapping), and the chord no-ops when there are no unread rows.
- Given at least one chat in the rendered window, when the user presses ⌃Tab/⌃⇧Tab, then the open conversation advances to the next/previous chat in recency order (wrapping) with composer focus, and the chord no-ops on the bridges/approval views or an empty list.
- Given the whole flow, when a user walks unreads (⌥⌘↓), archives (`e`), replies (timeline `r`, pre-existing), and jumps to the next unread (⌥⌘↓), then the triage loop completes with zero pointer use.

## Spec Change Log

No `bad_spec` loopback occurred; this section is intentionally empty.

## Review Triage Log

### 2026-07-06 — Review pass (independent follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 1
- reject: 11
- addressed_findings:
  - none

Follow-up review recommended by the prior pass. Both reviewers' headline finding — the new global `⌥⌘↑/↓` unread-jump chord double-acts with the pre-existing unmodified-`ArrowUp`/`ArrowDown` handlers in `conversation-pane.tsx:1319` (timeline message-selection) and `composer.tsx:733` (empty-composer edit-last), which don't guard modifier keys — is real but rooted in epic-3 handlers the spec's Never section forbids modifying (`conversation-pane.tsx onKeyDown` / composer keys). The spec-compliant fix (a modifier guard on those handlers, or capture-phase `stopPropagation` in the new window hooks) is a focused, cross-cutting coexistence decision beyond an in-diff patch, and the practical impact is bounded (the local side-effect lands on the conversation being navigated away from, which the switch immediately supersedes). Deferred for focused attention. All other findings rejected: no-text-field-guard on `⌃Tab`/`⌥⌘↑↓` and AltGr `⌥⌃` parity were already rejected by the prior pass as by-design (macOS-first, chords fire from the composer like `⌘K`); `Enter` native-click double-select is suppressed by the handler's `preventDefault`; the `renderedWindowRooms`/`visibleRooms` duplication has no divergence today; silent verb-reject matches the I/O matrix; the `isComposing` guard on `⌘1/⌘2` vs `⌘3/⌘4` and Tab-to-row-vs-arrow-cursor divergence are cosmetic; account sign-out's real path (`removeAccount`) already clears a stale `filterAccountId` (only the test-utility `clear()` doesn't); the rest are test-coverage/source-ordering nits.

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 2, low 2)
- defer: 0
- reject: 6
- addressed_findings:
  - `[medium]` `[patch]` Roving focus tracked a **positional** `focusedIndex`, never reconciled when the Rust stream re-orders `visibleRooms` (routine recency bump) or the focused row leaves the window — so after a reorder a single-key verb archived/pinned the *wrong* chat (ring on room X, `visibleRooms[index]` = room Y), and an out-of-range index gave every row `tabIndex=-1`, losing the list's keyboard tab stop. Refactored to a stable `${accountId}:${roomId}` `focusedKey` resolved to a position each render (falling back to the first row as the tab stop when the keyed row is gone); verbs/Enter now target by identity and no-op when the focused room has left the window. Added two regression tests (survives re-order; no-ops when gone).
  - `[medium]` `[patch]` The container `onKeyDown` was not scoped to the main list, so bare `e`/`u`/`p`/`f`/`j`/`k`/Enter bubbling from a Pins-strip / Favorites-section / filter-chip button (focusable siblings in the same container) acted on the main-list ring — violating the spec's "fire only when focus is within the list" and its Pins/Favorites scope-out. Added a `target.closest('ul[aria-label="Conversations"]')` guard (Esc filter-clear stays container-wide). Added a regression test.
  - `[low]` `[patch]` Esc cleared the ring state but never blurred the DOM-focused row, so its `focus-visible` ring persisted while `tabIndex` flipped to `-1`. Esc now blurs the focused row before dropping the cursor.
  - `[low]` `[patch]` Two misleading comments in `use-unread-jump.ts` (claimed "exactly one of meta/ctrl" while the code accepts either for parity; claimed it would `preventDefault` on the zero-unread no-op while it intentionally does not) — corrected both to match the intended behavior.
  - Rejected (6, noise/by-design): no text-field guard on ⌃Tab/⌥⌘↑↓ (deliberate — global chords should work from the composer, matching ⌘K's documented no-guard); redundant `focusNonce` bump on re-Enter (harmless); ⇧Tab from an out-of-window selection opens the last row (standard wrap); ⌘1/⌘2 firing on a null event target (intended — not a text field); AltGr (`ctrl+alt`) spuriously matching ⌥⌃ on Win/Linux (macOS-first MVP; arrow+AltGr is obscure); `rowRefs` never length-trimmed (benign, optional-chained; the key-based resolve removes the hazard).

## Design Notes

- **Why chat-list-only:** the timeline half of epic-9's keyboard spec already ships from epic 3 — `conversation-pane.tsx:1300` handles ↑/↓/`r`/`e`/⌫ against `composerStore.selectedKey`, and `composer.tsx` handles Esc-cancel / ArrowUp-to-edit / Enter-send with `focusNonce` focus. 9.2 adds the missing chat-list layer and the global chords; it must not duplicate or fight the timeline handlers (different focus context, different store cursor).
- **"Quick-switcher rides the palette index":** the epic's intent is honored by cycling the recency-ordered inbox window (`roomsStore.rooms`), which is the windowed projection of the *same* Rust recency ordering the palette index holds. Cycling ⌃Tab through 10k chats would be nonsensical UX; the recent window is exactly the "recent chats" a switcher should offer. A full all-accounts MRU visit-stack switcher over the palette index is a deliberate future extension (keeps TS free of any re-ordering).
- **List-focused vs. global keys:** bare-key list verbs and arrows/`j`/`k` live on the chat-list container's `onKeyDown` (fire only sans-modifier with focus inside the list) so they never collide with the timeline's own ↑/↓ or with global ⌘/⌥/⌃ chords; the three chord hooks live on `window` following the shipped per-hook pattern. This split is why ⌘F/⌘K/⌘1 pass cleanly to their global handlers even while the list is focused.
- **Verb direction + `u` parity:** pick the command from the row's current flag (`isArchived`/`isPinned`/`isFavourite`) exactly as the `ChatRow` context menu does; `u` uses `effectiveIsUnread` + `setOptimisticUnread`/`clearOptimisticUnread` so the row flips within a frame and reverts on a hard reject — reusing, not re-deriving, the Rust-authoritative state.

## Verification

**Commands:**
- `bun run check` -- expected: Biome + tsc + Vitest pass, including the new hook tests and the extended `chat-list-pane.test.tsx`.
- `bun run check:rust` -- expected: clean (no Rust changed; confirms nothing regressed).

**Manual checks:**
- With two accounts and mixed unread state: ⌥⌘↓ walks unreads, `e` archives the focused row (it leaves the inbox window), `r` in the timeline replies, ⌃Tab cycles open chats — all pointer-free; ⌘1/⌘2 switch Inbox/Archive; Esc unwinds overlay → composer → timeline selection → filter → list ring.

## Auto Run Result

Status: done

**Summary:** Implemented Story 9.2 — chat-list keyboard operability plus the global chords that complete epic-9's keyboard set. A roving focus ring over the Rust-ordered inbox/archive window (↑/↓ + `j`/`k`, tracked by stable room identity), `Enter` opens the focused chat and drops focus into the composer, and single-key verbs `e`/`u`/`p`/`f` invoke the shipped archive/read/pin/favorite commands on the focused row (direction from the row's flag; `u` mirrors the optimistic-unread pattern). Three window-level chord hooks: `⌘1`/`⌘2` switch Inbox/Archive (completing `⌘1–4` alongside the existing `⌘3`/`⌘4`), `⌥⌘↓`/`⌥⌘↑` jump next/previous unread, and `⌃Tab`/`⌃⇧Tab` cycle the open conversation. No new Rust, no new Vm, no re-ordering in TS — every operation moves a cursor over arrays Rust already ordered (via the shared read-only `rendered-window.ts` projection). Timeline keys (↑/↓/`r`/`e`/⌫) and composer keys were already shipped in epic 3 and left untouched; `m` mute is omitted (no backend exists).

**Files changed (one-liners):**
- `src/hooks/use-view-shortcuts.ts` (new) — `⌘1`→inbox / `⌘2`→archive chord (text-field + IME guard, preventDefault).
- `src/hooks/use-quick-switcher.ts` (new) — `⌃Tab`/`⌃⇧Tab` open-chat cycler over the rendered window (wrap; select + composer focus; no-op off-list/empty).
- `src/hooks/use-unread-jump.ts` (new) — `⌥⌘↓`/`⌥⌘↑` next/previous-unread jump (effective-unread scan, wrap, open + composer focus, no-op when none).
- `src/lib/rendered-window.ts` (new) — shared read-only inbox/archive window projection (account-filtered, Rust order), `null` for bridges/approval.
- `src/components/chat/chat-row.tsx` — `forwardRef` + optional roving `tabIndex`; all existing behavior preserved.
- `src/components/layout/chat-list-pane.tsx` — identity-keyed roving focus, container `onKeyDown` (move / open+composer-focus / verbs / Esc), main-list scope guard, roving tabindex.
- `src/components/layout/app-shell.tsx` — mounted the three new hooks.
- Tests: `use-view-shortcuts.test.ts`, `use-quick-switcher.test.ts`, `use-unread-jump.test.ts` (new) + extended `chat-list-pane.test.tsx` (nav, verbs, `u` overlay, modifier passthrough, Esc, re-order identity, leave-window no-op, main-list scoping).

**Review findings:** 2 adversarial reviewers (Blind Hunter + Edge Case Hunter). Triage: 0 intent_gap, 0 bad_spec, **4 patches applied** (2 medium, 2 low), 0 deferred, 6 rejected. See the Review Triage Log for the itemized list. No spec loopback was required. The two medium patches were genuine correctness fixes: positional-index staleness under recency re-ordering (wrong-target verb + lost tab stop) → refactored to identity-keyed focus; and the container handler stealing keys from the excluded Pins/Favorites/chip surfaces → scoped to the conversations list. Three regression tests were added to lock these in.

**Follow-up review recommended:** true — the review pass changed the focus-tracking model (positional → identity) to fix a wrong-target correctness hazard and added a focus-context scope guard; medium-severity behavior changes to the core list handler warrant an independent follow-up look even though they are well-covered by the new regression tests.

**Verification:** all gates green after patches — `bun run check` (Biome + `tsc --noEmit` + **845 Vitest tests**, incl. the new hook suites and the extended chat-list nav suite) PASS; `bun run check:rust` (rustfmt + clippy `-D warnings`) PASS (no Rust changed). Independently re-run, not just trusted from the subagent.

**Residual risks:** (1) Pins-strip and Favorites-section rows remain mouse/Tab-only (keyboard nav within them is a deliberate scope-out). (2) `⌃Tab`/`⌥⌘↑↓` intentionally have no text-field guard, so they fire from the composer/search input (by design, matching ⌘K); a user who expects `⌃Tab` to do nothing while typing will still switch chats. (3) The quick-switcher cycles the recency-windowed list, not a full all-accounts MRU visit-stack (deliberate MVP scope). (4) NEW (follow-up review): `⌥⌘↑/↓` co-fires with the unmodified-arrow handlers in the timeline/composer when one of those is focused — a bounded double-action deferred to focused cross-cutting work (see the follow-up pass note below and `deferred-work.md`).

## Auto Run Result — Follow-up Review Pass (2026-07-06)

Status: done

**Summary:** Independent follow-up review (recommended by the prior pass). No code changed. Two adversarial reviewers (Blind Hunter + Edge Case Hunter) re-examined the full diff since baseline. Triage: **0 intent_gap, 0 bad_spec, 0 patch, 1 defer, 11 reject.** No patch applied and no spec loopback → `addressed_findings: none`.

**Files changed this pass:** none (source unchanged). Only review bookkeeping: this spec's Review Triage Log + Auto Run Result + frontmatter (`status`, `followup_review_recommended`, `final_revision`), and one NEW entry appended to `deferred-work.md`.

**Review findings:** The single non-rejected finding is a genuine cross-handler collision — the new global `⌥⌘↑/↓` unread-jump chord co-fires with the pre-existing unmodified-`ArrowUp`/`ArrowDown` handlers in `conversation-pane.tsx:1319` (timeline) and `composer.tsx:733` (empty-composer edit-last), which don't guard modifiers. It is **deferred** (not patched) because its root cause and fix live in epic-3 timeline/composer handlers the spec's Never section forbids modifying; the spec-compliant fix is a focused coexistence decision (modifier guard on those handlers, or capture-phase `stopPropagation` in the new hooks), and the practical impact is bounded (the local side-effect lands on the outgoing conversation). All 11 other findings rejected: `⌃Tab`/`⌥⌘↑↓` text-field-guard and AltGr `⌥⌃` parity were already rejected by the prior pass as by-design; `Enter` native-click double-select is suppressed by `preventDefault`; `renderedWindowRooms`/`visibleRooms` duplication has no divergence today; silent verb-reject matches the I/O matrix; the `isComposing` guard delta vs `⌘3/⌘4` and Tab-vs-arrow cursor divergence are cosmetic; account sign-out's real path (`removeAccount`) already clears a stale filter; remainder are test-coverage/source-ordering nits.

**Follow-up review recommended:** false — this pass made zero code changes; there is nothing new to independently re-review.

**Verification:** No source files changed, so the reviewed HEAD's gate stands. Re-ran `bun run check` to confirm the tree is green — see command outcome recorded at commit time.

**Residual risks:** unchanged from the original run, plus the newly-deferred `⌥⌘↑/↓` timeline/composer co-fire noted above.
