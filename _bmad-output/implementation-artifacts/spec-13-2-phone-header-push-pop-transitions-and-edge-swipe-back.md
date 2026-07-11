---
title: 'Phone Header, Push/Pop Transitions, and Edge-Swipe Back'
type: 'feature'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: 'b5aa22c4eb05c1f41585383aa4cd57bf2e3807ae'
final_revision: '80c55b3345bdea3bececc0a3b493e59e9ce7d3e5'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-13-context.md'
warnings:
  - oversized
---

<intent-contract>

## Intent

**Problem:** On the phone tier (< 768 px) the Story 13.1 stack has only a minimal `BackControl` bar stacked *above* the reused desktop `ConversationPane` header (two bars), no push/pop motion, no gesture back, and no keyboard/assistive-tech focus management — so moving through Inbox → Room → Detail neither looks native nor is reversible or accessible (DW-110), and a room re-selected with Detail already open jumps straight to the Detail level (DW-109).

**Approach:** Replace `BackControl` with a single styled 52 px `PhoneHeader` that the stack renders at every level (back chevron carrying the previous level's title; at the Room level also the identity block, incognito chip, and an overflow ⋯ menu), suppress `ConversationPane`'s own header on the phone tier via a new backward-compatible prop, and add transform-driven push/pop transitions (reduced-motion → cuts), an interactive edge-swipe-back gesture at level ≥ 1, and full push/pop focus management. Timeline/composer/detail trees stay shared and unchanged.

## Boundaries & Constraints

**Always:**
- Reuse the existing `ConversationPane` / `DetailPanel` / `ChatListPane` trees unchanged as chat content — no forked chat components. The only permitted `ConversationPane` change is a backward-compatible `showHeader?: boolean` (default `true`) prop and exporting its header sub-parts for reuse; the desktop three-pane path stays byte-for-byte identical.
- Every tappable target ≥ 44 pt; the back control has an accessible name and a full 44 pt hit area.
- No gesture is the sole path to back: the chevron and the Escape/system-escape key trigger the same `onBack` at every level; the edge-swipe is an additive affordance.
- Reduced motion (`prefers-reduced-motion: reduce`) renders push/pop as instant cuts (no slide/dim), while the chevron, Escape, and edge-swipe still function.
- Capability/tier gating flows only from `useShellLayout().phone`; never sniff platform or user-agent.
- Level 0's leading edge is reserved (no back gesture) so Story 13.3 can attach the drawer there; the edge-swipe-back is active only at level ≥ 1.

**Block If:**
- `useShellLayout` no longer exposes a `phone` tier, or `detailStore` / `roomsStore.selected` selection primitives are absent or renamed — the projection contract from Story 13.1 is broken (HALT: missing 13.1 foundation).

**Never:**
- No routing library and no `history.pushState` dependency (still an optional future enhancer only).
- No bottom tab bar.
- Do not build the deferred overflow entries here: **Search in chat** (owned by Story 13.4), **Mute ▸** / **Mention-only** (owned by Stories 10.2 / 13.7), **Archive** (owned by Stories 4.2 / 13.6). This story ships the ⋯ menu container with the already-wired **Export** action; the rest land with their owning stories.
- Do not add safe-area or keyboard-inset (`env(safe-area-inset-*)`, `--kb-inset`) handling — that is Story 13.5.
- Do not change desktop detail-open persistence; the DW-109 detail reset is a phone-scoped effect inside `PhoneShell` only.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Room header render | phone, `selected` set, `detailOpen` false | 52 px `PhoneHeader`: back chevron labelled "Inbox" + identity block + incognito chip (when effective) + ⋯; `ConversationPane` renders with `showHeader={false}` (no second bar) | Missing room VM → identity degrades to account chip (existing behavior) |
| Detail header render | phone, `selected` set, `detailOpen` true | `PhoneHeader` back chevron carries the Chat name as title; no identity/overflow at level 2 | Unknown room name → generic "Back" accessible name |
| Tap identity block | phone, Room level | `detailStore.openDetail()` → push to Detail (the ⌘I replacement) | — |
| Overflow → Export | phone, Room level, ⋯ opened | `exportStore` opens for the selected room (same action as desktop header) | — |
| Push (level increases) | selection/detail change raises the level | new level slides in from trailing edge ~250 ms ease-out; level beneath shifts back ~25 % and dims; focus moves to the new level's back button | reduced-motion → instant cut |
| Pop (chevron/Escape/commit) | level ≥ 1, back invoked | popped level slides out to trailing edge; underlying level returns to 0; focus returns to the element that pushed it | reduced-motion → instant cut |
| Edge-swipe commit | level ≥ 1, pointer drag from leading edge > 50 % width or a fast flick | commits `onBack` (one level) | released < threshold and no flick → animates back to 0, no pop |
| Edge-swipe at level 0 | level 0, drag from leading edge | no back; edge reserved for the drawer (Story 13.3) | — |
| Room re-selected w/ Detail open (DW-109) | phone, `selected` changes while `detailOpen` true | phone-scoped effect closes Detail so the stack lands on the Room level, not Detail | — |
| Covered levels | any push | lower levels are `inert` (out of tab order + a11y tree) while covered | — |

</intent-contract>

## Code Map

- `src/hooks/use-shell-layout.ts` -- source of the `phone` tier (`useShellLayout().phone`); read-only here.
- `src/hooks/use-reduced-motion.ts` -- NEW: `matchMedia('(prefers-reduced-motion: reduce)')` hook (mirror the synchronous-init + `change`-listener pattern of `use-shell-layout.ts`).
- `src/lib/stores/detail-ui.ts` -- `detailStore` (`open`/`openDetail`/`closeDetail`/`toggleDetail`); read/call, do not change its shape.
- `src/lib/stores/rooms.ts` -- `roomsStore.selected` / `useRoomsStore`; read only.
- `src/lib/stores/export.ts` -- `exportStore` for the ⋯ → Export action.
- `src/components/layout/conversation-pane.tsx` -- add `showHeader?: boolean` (default true); export `ConversationHeaderIdentity` and `ConversationIncognitoChip` (and keep `incognitoChipLabel`) for the phone header to reuse.
- `src/components/layout/detail-panel.tsx` -- headerless `<aside>` reused unchanged (its header is provided by `PhoneHeader`).
- `src/components/layout/phone-header.tsx` -- NEW: the 52 px phone header (back chevron + contextual title; Room level adds identity block → Detail, incognito chip, ⋯ menu with Export).
- `src/components/layout/phone-shell.tsx` -- MODIFY: use `PhoneHeader` at both levels; suppress `ConversationPane` header on phone; push/pop transforms + presence; edge-swipe back (level ≥ 1); focus management + `inert`; DW-109 detail reset effect.
- `src/components/ui/dropdown-menu.tsx` -- existing shadcn menu used for the ⋯ overflow.
- `src/index.css` -- add the `--phone-header: 52px` token.

## Tasks & Acceptance

**Execution:**
- [x] `src/hooks/use-reduced-motion.ts` + `src/hooks/use-reduced-motion.test.ts` -- NEW hook returning `true` when `(prefers-reduced-motion: reduce)` matches, synchronous SSR-safe init + `change` listener exactly like `use-shell-layout.ts`; tests cover match/no-match and reactive change -- single flash-free source for the cut-vs-slide decision.
- [x] `src/index.css` -- add `--phone-header: 52px` to `:root` -- shared header-height token consumed by `PhoneHeader` (`h-[var(--phone-header)]`).
- [x] `src/components/layout/conversation-pane.tsx` -- add `showHeader?: boolean` (default `true`); when `false`, skip rendering the header row only (timeline + composer unchanged); `export` `ConversationHeaderIdentity` and `ConversationIncognitoChip` -- lets `PhoneHeader` reuse the identity/incognito UI verbatim and lets `PhoneShell` suppress the desktop header on phone so there is exactly one bar. Desktop callers omit the prop → identical behavior.
- [x] `src/components/layout/phone-header.tsx` -- NEW: `PhoneHeader({ level, onBack, backRef })` renders a `h-[var(--phone-header)]` bar. Back button: `ChevronLeft` + previous-level title (`level===1` → "Inbox"; `level===2` → selected room's display name via the selected-room VM), ≥ 44 pt hit area, `aria-label` = `Back to {title}`, forwards `backRef`. At `level===1` also render the `ConversationHeaderIdentity` block wrapped as a `button` that calls `detailStore.getState().openDetail()` (the ⌘I replacement, `aria-label="Open details"`), the `ConversationIncognitoChip`, and a `DropdownMenu` (⋯, `aria-label="More"`) whose only item today is **Export** (opens `exportStore`) -- the single 52 px Room bar per UX-DR21; deferred menu entries land with their owning stories.
- [x] `src/components/layout/phone-header.test.tsx` -- NEW: back title + `aria-label` per level; identity tap opens Detail; ⋯ → Export triggers `exportStore`; incognito chip shows only when effective -- covers the header I/O.
- [x] `src/components/layout/phone-shell.tsx` -- MODIFY: render `PhoneHeader` at levels 1 and 2 (drop `BackControl`); pass `showHeader={false}` to `ConversationPane`; drive each overlay level's horizontal transform from its role (`active` = 0, `covered` = −25 % + dim, `entering`/`exiting` = 100 %) with `transition-transform duration-[250ms] ease-out`, gated to `duration-0` when `useReducedMotion()`; keep an exiting level mounted until `onTransitionEnd` (presence) so pop animates; attach a leading-edge pointer handler active only at level ≥ 1 that tracks the drag (translate active level by `clamp(dx,0..width)`, covered level `−25%→0` proportionally), commits `onBack` past 50 % width or on a flick and otherwise snaps back; move focus to the new level's back button on push, capture `document.activeElement` before a push and restore it on pop, handle `Escape`/`onKeyDown` → `onBack`, and apply `inert` to covered levels; add a phone-scoped `useEffect(..., [selected])` that calls `detailStore.getState().closeDetail()` so a selection change never lands on Detail (DW-109) -- the native-feeling, accessible, reversible stack.
- [x] `src/components/layout/phone-shell.test.tsx` -- extend: exactly one header bar on phone (ConversationPane header suppressed); push adds the slide transform and focuses the new back button; reduced-motion renders `duration-0` (cut); edge-swipe past threshold commits back and below threshold cancels; level-0 leading edge does not pop; Escape pops; pop restores focus to the pushing element; covered levels are `inert`; DW-109: changing `selected` with detail open lands on the Room level -- guards every new behavior and the no-double-header regression.

**Acceptance Criteria:**
- Given a phone viewport with a room open, when the Room level renders, then exactly one 52 px header shows (back "Inbox" + identity + incognito-when-effective + ⋯), `ConversationPane`'s own header is not rendered, and tapping the identity block pushes Detail.
- Given a push, when motion is allowed, then the new level slides in ~250 ms ease-out with the level beneath shifted back ~25 % and dimmed, and focus lands on the new level's back button; when reduced-motion is set, the change is an instant cut with the same focus move.
- Given a pop via chevron, Escape, or a committed edge-swipe at level ≥ 1, then the stack drops exactly one level and focus returns to the element that pushed it; an edge-swipe released below threshold does not pop; at level 0 the leading edge never pops.
- Given a phone selection change while Detail was open, then Detail is closed and the stack renders the Room level (DW-109); covered levels are `inert` throughout.
- Given `bun run check`, then Biome + tsc + vitest (including the new/updated phone suites) pass, and the desktop `app-shell` / `conversation-pane` suites are unchanged.

## Spec Change Log

_No bad_spec loopback occurred; the spec was implemented as written._

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 1, low 3)
- defer: 0
- reject: 14
- addressed_findings:
  - `[medium]` `[patch]` Escape cascade collision: `ConversationPane`'s message-region Escape now calls `preventDefault` only when it actually consumes pending composer context / message selection, so the phone shell's `defaultPrevented` guard stops a single Escape from both cancelling a reply and popping to the Inbox. Added a `conversation-pane` regression test asserting Escape is marked handled iff there was something to clear.
  - `[low]` `[patch]` DW-109 detail-reset now compares selection by `(accountId, roomId)` value instead of object identity, so a future deep-link/search re-emit of the *same* room can't spuriously close an open Detail.
  - `[low]` `[patch]` `StackLevel` presence gained a 400 ms timeout fallback so a missed transform `transitionend` (interrupted transition, tab hidden mid-animation) can't leak a permanently-mounted inert pane.
  - `[low]` `[patch]` Edge-swipe zone added `onLostPointerCapture` cleanup so a mid-drag capture loss can't strand `pointerRef`/`drag` state and block the next gesture.

Rejected (14): `inert={false}` serialization (React 19 handles it, asserted by tests); button-in-button in the identity block (`RoomAvatar` has no interactive descendants); empty overflow menu when selection is null (the exiting level is `pointer-events-none`, unreachable); flick velocity via timestamps (correct for real pointer streams; only synthetic test events collapse `dt`); `useLayoutEffect` slide-vs-cut on WebKit (speculative; on-device animation verification belongs to Epic 14/15); mouse edge-swipe (phone-tier touch semantics apply to any pointer by design); focus-to-body on a pop that lands on the always-present Inbox (acceptable destination); covered-level 25 % width assumption (holds for the full-width flex child); mid-drag external `level` change freezing the transform, multi-touch second pointer, zero-width `getBoundingClientRect` fallback degeneracy, shared `edgeSwipeZone` element identity, global `getBoundingClientRect` test mock, and the Escape-vs-portalled-menu double action (already guarded by the container `contains` check).

## Design Notes

**Header ownership (single bar).** Story 13.1 stacked `BackControl` above `ConversationPane`'s own desktop header (two bars). UX-DR21 wants one 52 px bar, and `DetailPanel` is headerless — so the stack must own the header at *both* levels. `PhoneHeader` renders it; `ConversationPane` suppresses its header on phone via `showHeader={false}`. This is not a fork: the timeline/composer/detail trees stay shared and unchanged; only the identity/incognito sub-parts are exported for reuse. "Tap identity → Detail" is the phone's ⌘I (there is no ⌘I keybinding; desktop uses the `PanelRight` toggle button, which the phone omits).

**Transition model (transform + presence, no animation lib).** No `framer-motion`; use CSS transforms + `transition-transform` (prior art: `ui/sheet.tsx` data-attribute slide). Each overlay is `absolute inset-0`; its role maps to a transform:

```
active   -> translateX(0)
covered  -> translateX(-25%) + opacity/brightness dim   (level beneath a push)
exiting  -> translateX(100%)   (popped, kept mounted until onTransitionEnd, then unmounted)
```

Level 0 (`ChatListPane`) stays mounted (Inbox scroll survives). `useReducedMotion()` swaps the duration to `0` for cuts. Follow the `use-shell-layout.ts` matchMedia pattern for the hook so tests can mock it with the existing `phone-shell.test.tsx` `mockViewportWidth` regex helper (extend it to also match `prefers-reduced-motion`).

**Edge-swipe.** No gesture code exists in the repo — build fresh with pointer events (`onPointerDown`/`Move`/`Up`, `setPointerCapture` is polyfilled in `src/test/setup.ts`). A ~20 px leading-edge hit zone on the active overlay captures the drag only at level ≥ 1; commit when `dx > width * 0.5` or a fast flick (`dx/dt` over a small threshold), else animate back to 0. Read width from the container's `getBoundingClientRect().width` with a `window.innerWidth` fallback (tests mock the rect). Level 0's edge is deliberately inert here (reserved for the 13.3 drawer).

**Focus (UX-DR28 / DW-110).** On push, focus the new `PhoneHeader` back button (`backRef`); before a push, capture `document.activeElement` and restore it on the matching pop; `Escape` on the stack calls `onBack`; covered levels get the `inert` attribute (React 19 supports it) so keyboard/VoiceOver cannot reach behind the visible level.

**DW-109.** A phone-only `useEffect` keyed on `selected` calls `closeDetail()` so a room (re)selected while `detailOpen` is true resolves to the Room level, not Detail. It never fires on `openDetail()` (which leaves `selected` unchanged), so tap-identity → Detail still works. Desktop detail-open persistence is untouched (the effect lives in `PhoneShell`, which mounts only on phone).

## Verification

**Commands:**
- `bun run check` -- expected: Biome + `tsc --noEmit` + vitest all green, including `use-reduced-motion`, `phone-header`, and the extended `phone-shell` suites; desktop `app-shell` / `conversation-pane` suites unchanged.
- `bun run test -- phone-header phone-shell use-reduced-motion conversation-pane` -- expected: the touched suites pass in isolation (jsdom matchMedia + pointer-capture polyfills exercise the phone tier, reduced-motion, and the edge-swipe).

## Auto Run Result

Status: done

### Summary
Replaced Story 13.1's minimal `BackControl` with a single styled 52 px `PhoneHeader` that the phone stack renders at both overlay levels: the Room level shows back-chevron "Inbox" + the (reused) identity block as an "Open details" button (the ⌘I replacement) + the incognito chip + a ⋯ overflow menu (Export today; Search/Mute/Mention-only/Archive land with their owning stories), and the Detail level shows the back-chevron carrying the room name. `ConversationPane` gained a backward-compatible `showHeader` prop (default `true`) so the phone tier renders exactly one bar; its identity/incognito sub-parts are exported and reused verbatim (no forked chat trees). Added transform-driven push/pop transitions with a presence state machine (level beneath shifts back 25 % + dims; reduced-motion → instant cuts via a new `useReducedMotion` hook), an interactive edge-swipe-back gesture at level ≥ 1 (level 0's edge reserved for the Story 13.3 drawer), full push/pop focus management (focus the new back button on push, restore the pusher on pop, Escape pops, `inert` on covered levels — DW-110/UX-DR28), and a phone-scoped effect that closes Detail on a genuine selection change so the stack never lands on Detail (DW-109). Desktop/tablet behavior at ≥ 768 px is unchanged.

### Files changed
- `src/hooks/use-reduced-motion.ts` — NEW `(prefers-reduced-motion: reduce)` hook (synchronous-init + `change`-listener, `use-shell-layout` pattern).
- `src/hooks/use-reduced-motion.test.ts` — NEW match/no-match/reactive-flip/cleanup coverage.
- `src/index.css` — added the `--phone-header: 52px` token.
- `src/components/layout/conversation-pane.tsx` — `showHeader?` prop (default true) gating only the header row; exported `ConversationHeaderIdentity` / `ConversationIncognitoChip`; Escape now `preventDefault`s only when it consumes composer context (phone-shell cascade fix).
- `src/components/layout/conversation-pane.test.tsx` — regression test for the Escape cascade contract.
- `src/components/layout/phone-header.tsx` — NEW 52 px phone header.
- `src/components/layout/phone-header.test.tsx` — NEW header I/O coverage.
- `src/components/layout/phone-shell.tsx` — `PhoneHeader` at both levels, `showHeader={false}`, transform/presence transitions, edge-swipe back, focus/`inert` management, DW-109 value-compared reset, presence timeout fallback, `onLostPointerCapture` cleanup.
- `src/components/layout/phone-shell.test.tsx` — extended for the full 13.2 behavior matrix.

### Review findings breakdown
- Patches applied: 4 — Escape cascade collision (medium); DW-109 value-compare, presence timeout fallback, and edge-swipe `onLostPointerCapture` cleanup (low). See the Review Triage Log.
- Deferred: 0.
- Rejected: 14 (noise / not reachable through the phone surface / by-design / speculative on-device animation). See the Review Triage Log.
- intent_gap: 0, bad_spec: 0.

### Follow-up review
`followup_review_recommended: false` — the four fixes are localized and well-understood (no API/security/data-model impact, no behavior re-derivation), each verified by the green gate and the added Escape-cascade regression test.

### Verification
`bun run check` — green: Biome clean (278 files), `tsc --noEmit` clean, vitest 99 files / 996 tests passed, core-tauri-free convention check passed. Run independently by the main loop after implementation and again after the four review patches.

### Residual risks
- Push/pop slide smoothness and the interactive edge-swipe feel are only verifiable on a real WKWebView/device; per the epic, on-device confirmation folds into the Epic 14/15 hardening + SM-8 dogfooding gate, not this story's acceptance.
- The ⋯ overflow ships with Export only; Search-in-chat (13.4), Mute ▸ / Mention-only (10.2 / 13.7), and Archive (4.2 / 13.6) entries are intentionally deferred to their owning stories.
- The phone Detail level remains the headerless placeholder from 13.1 (its content is a later story); only its `PhoneHeader` chrome is delivered here.
