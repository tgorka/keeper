---
title: 'Phone Layout Tier and Navigation Stack'
type: 'feature'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 0
baseline_revision: '898786af5889a5d3436b9f5b500929b8ca598257'
final_revision: 'f54f0d8f7bc20142c35951873e64f77c932373f5'
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-13-context.md'
warnings:
  - oversized
---

<intent-contract>

## Intent

**Problem:** keeper's shell renders three panes side by side, which does not fit an iPhone (< 768 px) viewport — there is no single-pane navigation and no phone layout tier, so the desktop product cannot project onto a phone without becoming a second app.

**Approach:** Add a `phone` tier to `useShellLayout` (< 768 px) and a `PhoneShell` stack container that projects the existing zustand selection state into exactly one visible level at a time — level 0 Inbox, level 1 Room, level 2 Detail — reusing the existing panes unchanged, with no forked components and no routing library.

## Boundaries & Constraints

**Always:**
- Below 768 px render exactly one level; at ≥ 768 px the desktop/tablet three-pane behavior is unchanged (regression-tested).
- Reuse `ChatListPane` / `ConversationPane` / `DetailPanel` unchanged — no forked chat components.
- Derive the level purely from existing zustand selection state: `roomsStore.selected` (level 0 ↔ 1) and a lifted detail-open store (level 1 ↔ 2). No new source of truth for navigation.
- Keep level 0 (Inbox) mounted across pushes so its scroll position is preserved.
- Back / pop is keyboard- and assistive-tech-accessible; every tappable target ≥ 44 pt.
- Opening a Chat on the phone must NOT auto-focus the composer.

**Block If:**
- Honoring any acceptance criterion would force a change to desktop/tablet (≥ 768 px) layout behavior — stop rather than regress it.
- Preserving Inbox scroll would require unmounting/virtualization or other structural changes to `ChatListPane` that alter its desktop rendering.
- The phone tier and desktop tiers cannot share the same selection stores without diverging behavior.

**Never:**
- No routing library; `history.pushState` / `popstate` is out of scope (an optional future enhancer, not a dependency).
- No phone header chrome, push/pop animations, or edge-swipe back (Story 13.2); no drawer / sidebar-as-sheet (13.3); no Search surface (13.4); no safe-area / keyboard work (13.5); no touch idioms (13.6); no capability surface-hiding (13.7).
- No bottom tab bar anywhere.
- No second visual language, restyled components, or Matrix/IPC logic changes.
- No rewiring of the coarse notification-navigate path (Story 10.4) into exact landing — 13.1 relies only on the existing selection primitive.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Phone, nothing open | viewport < 768, `selected = null` | Single pane: level 0 Inbox (`ChatListPane`, honoring active view/filter) | No error expected |
| Phone, room open | viewport < 768, `selected` set, detail closed | Level 1 Room (`ConversationPane`) covers the still-mounted Inbox | No error expected |
| Phone, detail open | viewport < 768, `selected` set, detail open | Level 2 Detail (`DetailPanel`) covers level 1 | No error expected |
| Deep-link selection | viewport < 768, `requestFocus({accountId,roomId,eventId})` | `selected` set → renders level 1 Room; back returns to level 0 Inbox | No error expected |
| Back from Room | viewport < 768, at level 1, activate back | `selectRoom(null)` → level 0, Inbox scroll position preserved | No error expected |
| Back from Detail | viewport < 768, at level 2, activate back | detail-open → false → level 1 Room (one level popped) | No error expected |
| Open chat on phone | viewport < 768, tap a row | `selected` set; composer `focusNonce` NOT bumped (no auto-focus) | No error expected |
| Desktop/tablet | viewport ≥ 768 | `phone = false`; three-pane frame unchanged; `PhoneShell` not mounted | No error expected |
| Resize across 768 | width 800 → 700 with a room open | Switches to phone stack at level 1; `selected` preserved | No error expected |

</intent-contract>

## Code Map

- `src/hooks/use-shell-layout.ts` -- two-tier matchMedia hook (`sidebarCollapsed`, `detailFloating`); add the `phone` tier here.
- `src/hooks/use-shell-layout.test.ts` -- existing tier tests; extend with phone-tier cases.
- `src/lib/stores/rooms.ts` -- selection primitive: `selected`, `selectRoom(sel|null)`, `requestFocus(FocusEvent)`, `focusEvent` (read/use only; do not change).
- `src/lib/stores/detail-ui.ts` -- NEW store lifting detail-open state.
- `src/components/layout/app-shell.tsx` -- top-level composition; owns `detailOpen` local state today and arranges the three panes.
- `src/components/layout/app-shell.test.tsx` -- shell composition tests; extend for the phone branch + lift.
- `src/components/layout/phone-shell.tsx` -- NEW single-pane stack container.
- `src/components/layout/chat-list-pane.tsx` -- row-open handler calls `selectRoom` then `composerStore.requestFocus()` (~lines 528-529); gate the focus call off on phone.
- `src/components/layout/conversation-pane.tsx` / `detail-panel.tsx` -- the Room / Detail panes the stack reuses unchanged.

## Tasks & Acceptance

**Execution:**
- [x] `src/hooks/use-shell-layout.ts` -- add a `phone: boolean` field (viewport < 768 px, new `PHONE_BREAKPOINT = 768`) alongside the existing flags, using the same synchronous-init + `matchMedia` `change`-listener pattern -- one flash-free source for the phone tier.
- [x] `src/hooks/use-shell-layout.test.ts` -- add cases: < 768 → `phone` true, ≥ 768 → `phone` false, and existing `sidebarCollapsed` / `detailFloating` results unchanged at the boundary -- proves the tier and no desktop regression.
- [x] `src/lib/stores/detail-ui.ts` -- NEW vanilla zustand store `detailStore` with `{ open: boolean, openDetail(), closeDetail(), toggleDetail() }` plus `useDetailStore(selector)`, following the store pattern used across `src/lib/stores/` -- shared detail-open signal both tiers project (level 1 ↔ 2).
- [x] `src/lib/stores/detail-ui.test.ts` -- NEW: assert open / close / toggle transitions and initial `open = false` -- covers the new store.
- [x] `src/components/layout/app-shell.tsx` -- consume `useDetailStore` in place of the local `detailOpen` `useState` (keep the existing `toggleRef` focus-return on close via a wrapping `closeDetail`); when `useShellLayout().phone` is true, render `<PhoneShell/>` instead of the desktop `SidebarPane` + panes row, leaving all global overlays / dialogs / shortcut hooks mounted -- routes phone viewports to the stack while the desktop path stays byte-for-byte identical.
- [x] `src/components/layout/phone-shell.tsx` -- NEW stack container: derive `level = detailOpen && selected ? 2 : selected ? 1 : 0`; always mount level 0 `ChatListPane`; mount `ConversationPane` (level 1) as an opaque overlay when `selected`; mount `DetailPanel` (level 2, non-floating) over that when detail-open; provide an accessible back control (≥ 44 pt, `aria-label="Back"`) that pops one level (level 2 → `closeDetail`, level 1 → `selectRoom(null)`) -- the single-pane projection of existing selection state, Inbox kept mounted for scroll preservation.
- [x] `src/components/layout/phone-shell.test.tsx` -- NEW: level derivation for all three levels, back-pop transitions, Inbox stays mounted across a push, deep-link via `requestFocus` lands level 1 with back → Inbox, and opening a chat does not bump the composer `focusNonce` -- covers the I/O matrix.
- [x] `src/components/layout/chat-list-pane.tsx` -- gate the `composerStore.getState().requestFocus()` in the row-open handler behind `!phone` (read the phone tier from `useShellLayout`) -- suppresses composer auto-focus on phone (UX-DR22) while preserving desktop focus-on-open.
- [x] `src/components/layout/app-shell.test.tsx` -- extend: below 768 renders `PhoneShell` and not the desktop panes; at ≥ 768 the three-pane frame is unchanged; detail-open still toggles through the store -- regression guard for the desktop path and the lift.

**Acceptance Criteria:**
- Given a viewport ≥ 768 px, when the app renders, then the existing three-pane frame and all its behaviors are unchanged, `PhoneShell` is not mounted, and the `use-shell-layout` / `app-shell` suites pass.
- Given a phone viewport with a room open, when the user activates back, then selection clears to `null`, the Inbox reappears at its preserved scroll position, and no composer focus was stolen when the room was opened.
- Given a phone viewport, when selection state is set by the deep-link primitive (`requestFocus` with account / room / event), then the stack renders the Room level and back returns to the Inbox.
- Given the phone stack rendering any level, then it uses the unchanged `ChatListPane` / `ConversationPane` / `DetailPanel` and adds no routing dependency.

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 5: (high 0, medium 3, low 2)
- reject: 9
- addressed_findings:
  - none

Deferred (owned by named follow-on stories or unreachable through the 13.1 phone surface): bridges/approval routing + all sidebar-hosted surfaces on phone (Settings, account switcher, offline pill, Archive/Bridges/Approval nav) → 13.3 drawer [DW-108]; stale `detailStore.open` not reset on room switch/deselect [DW-109]; push/pop focus movement, covered-level a11y containment, and back focus-return/Escape → 13.2 header + UX-DR28 focus rules [DW-110]. Rejected: no history/OS-back (out of scope by spec; edge-swipe is 13.2 FR-60), matchMedia-in-effect (pre-existing pattern, guarded initializer, present in all real targets), duplicate `useShellLayout` subscription + second 768 constant (`use-mobile.ts` is unused dead code; nil consequence), redundant detail toggle (covered by the Detail overlay; the only phone open-path at 13.1), `level`-vs-booleans style, deferred-surface test-coverage observation, and the composer-focus resize micro-race.

## Design Notes

Level derivation (single source, no router):

```
detailOpen && selected  -> 2  (Detail)
selected                -> 1  (Room)
otherwise               -> 0  (Inbox)
```

Level 0 stays mounted under every push so its native scroll offset is preserved (unmounting would lose it) and so Story 13.2's "level beneath shifts back and dims" transition has something to animate. Higher levels are opaque (`bg-background`) overlays.

Detail-open lift: AD-31 is a *projection* — one selection state, two arrangements. Detail-open is trapped in desktop-only local state today; lifting it into `detailStore` lets both the desktop shell and the phone stack derive their layout from the same signal, and keeps the desktop toggle/focus-return behavior intact (the `toggleRef` focus-return stays in `AppShell`).

Deep-link scope: 13.1 satisfies the deep-link AC through the existing `roomsStore.requestFocus` primitive (it sets `selected` + `focusEvent`, exactly as search-result activation does). The coarse `use-notify-navigate` path (Story 10.4, deliberately coarse; iOS notifications deferred; desktop click seam carries no per-notification payload) is NOT rewired here — the stack only guarantees that a programmatically-set selection renders at the right level with back → Inbox.

Back affordance is minimal and accessible in 13.1; Story 13.2 replaces it with the 52 px `phone-header` (chevron + title), push/pop transitions, and edge-swipe back.

## Verification

**Commands:**
- `bun run check` -- expected: biome lint + tsc + vitest all pass, including the new `phone-shell`, `detail-ui`, and extended `use-shell-layout` / `app-shell` suites.
- `bun run test -- use-shell-layout phone-shell detail-ui app-shell` -- expected: the touched suites pass in isolation (jsdom `matchMedia` mock exercises < 768 px).

## Auto Run Result

Status: done

### Summary
Added a third `phone` layout tier (< 768px) to `useShellLayout` and a new `PhoneShell` single-pane navigation stack that projects the existing zustand selection state into one visible level at a time — level 0 Inbox, level 1 Room, level 2 Detail. Detail-open state was lifted out of `AppShell` local state into a shared `detailStore` so both the desktop three-pane frame and the phone stack derive their layout from one signal (AD-31 projection). Below 768px `AppShell` renders `PhoneShell` in place of the sidebar + panes row; the panes (`ChatListPane` / `ConversationPane` / `DetailPanel`) are reused unchanged, no routing library is added, level 0 stays mounted so Inbox scroll survives pushes, and the composer auto-focus on room-open is suppressed on the phone tier (UX-DR22). Desktop/tablet behavior at ≥ 768px is unchanged.

### Files changed
- `src/hooks/use-shell-layout.ts` — added the `phone` tier (`PHONE_BREAKPOINT = 768`) with the existing synchronous-init + matchMedia-listener pattern.
- `src/hooks/use-shell-layout.test.ts` — phone-tier cases (on <768, off at 768) + existing tier flags asserted unchanged.
- `src/lib/stores/detail-ui.ts` — new vanilla zustand `detailStore` (`open`/`openDetail`/`closeDetail`/`toggleDetail`) + `useDetailStore`.
- `src/lib/stores/detail-ui.test.ts` — open/close/toggle/idempotency coverage.
- `src/components/layout/phone-shell.tsx` — new single-pane stack container (level derivation, always-mounted Inbox, opaque overlays, minimal accessible back control popping one level).
- `src/components/layout/phone-shell.test.tsx` — 7 tests covering the full I/O matrix (levels, back-pop, scroll preservation, deep-link via `requestFocus`, no composer focus on open).
- `src/components/layout/app-shell.tsx` — consumes `detailStore` (local `useState` removed, toggle-focus-return preserved); renders `PhoneShell` on the phone tier; floating Sheet gated `!phone`.
- `src/components/layout/app-shell.test.tsx` — phone-branch + lifted-store regression tests.
- `src/components/layout/chat-list-pane.tsx` — gates the row-open composer `requestFocus()` off on the phone tier.

### Review findings breakdown
- Patches applied: none.
- Deferred (5 findings → ledger DW-108/DW-109/DW-110): phone access to sidebar-hosted surfaces + bridges/approval routing (→ Story 13.3 drawer); stale `detailStore.open` on room switch/deselect (→ Story 13.2); push/pop focus movement + covered-level a11y containment + back focus-return/Escape (→ Story 13.2, UX-DR28).
- Rejected (9 findings): no history/OS-back (out of scope by spec; edge-swipe is 13.2), matchMedia-in-effect (pre-existing, guarded, present in all real targets), duplicate `useShellLayout` subscription + second 768 constant (`use-mobile.ts` unused), redundant detail toggle (covered by the Detail overlay), `level`-vs-booleans style, deferred-surface test-coverage note, composer-focus resize micro-race.
- intent_gap: 0, bad_spec: 0.

### Follow-up review
`followup_review_recommended: false` — the review pass applied no patches and triggered no spec loopback, so there are no review-driven code changes to independently re-review.

### Verification
`bun run check` — green: Biome clean (274 files), `tsc --noEmit` clean, vitest 97 files / 971 tests passed, core-tauri-free convention check passed. Run independently after implementation and confirmed by the main loop.

### Residual risks
- On the phone tier the sidebar-hosted surfaces (Settings, account switcher, offline pill, Archive/Bridges/Approval nav) are intentionally unreachable until the Story 13.3 drawer restores them; on an actual iPhone at 13.1 these states are also not triggerable (no hardware keyboard, native menu bar is desktop-only). Tracked as DW-108.
- Phone stack keyboard/AT focus management (focus on push/pop, `inert`/`aria-hidden` on covered levels, back focus-return/Escape) is deferred to Story 13.2's header + UX-DR28 focus rules. Tracked as DW-110.
- `detailStore.open` is a global flag not reset on room change; not reachable through the 13.1 phone surface (no in-stack room switch without back; deep-link not wired on phone yet). Tracked as DW-109.
