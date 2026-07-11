---
title: 'Leading Drawer with Status Cluster'
type: 'feature'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '2aee5a8a747e1376e83b32063921b6a0f1883d1f'
final_revision: '8ade0bf3551bb7d38fd9543bd57335afa57ef57b'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-13-context.md'
warnings:
  - oversized
---

<intent-contract>

## Intent

**Problem:** On the phone tier (< 768 px) the Story 13.1/13.2 stack renders the Inbox (level 0) as a bare `ChatListPane` with no header, and the entire desktop sidebar — primary views, SPACES, NETWORKS, account switcher, sync/offline status — has no reachable home on phone. The states that must never hide (worst-state bridge health, pending-approval count, account filter) have nowhere to surface (FR-58 rail leg, UX-DR23).

**Approach:** Add a 52 px level-0 Inbox header carrying a leading avatar button (drawer trigger, quiet-when-healthy status cluster) plus a trailing amber Approval chip, magnifier, and compose button; render the existing desktop `SidebarPane` **verbatim** inside a leading `Sheet` drawer opened by that button or by a leading-edge swipe at level 0 only. Selecting any view/filter/row closes the drawer, applies it, and returns focus to the trigger. No forked sidebar, no bottom tab bar.

## Boundaries & Constraints

**Always:**
- Reuse the desktop `SidebarPane` **unchanged**, mounted with `collapsed={false}` inside the drawer — no forked or second sidebar component. Its store subscriptions and the shared `SettingsDialog` (app-shell-mounted) work as-is.
- The drawer is the existing shadcn `Sheet` with `side="left"`; it is a modal that traps focus and hides the rest of the app from assistive tech (radix Dialog default).
- Status cluster is **quiet when healthy**: the bridge-health dot renders only for `degraded`/`disconnected` (hidden on `healthy`/`null`); the amber Approval chip renders only when pending-Draft count > 0; the account-filter cue shows only when a filter is active.
- Every tappable target ≥ 44 pt; the avatar/drawer, magnifier, and compose buttons each have an accessible name.
- On drawer close (row-select, scrim tap, Escape, or swipe-to-close), focus returns to the avatar drawer button (UX-DR28).
- Level 0's leading edge opens the drawer (the edge Story 13.2 deliberately reserved); the level ≥ 1 back-swipe from Story 13.2 stays unchanged.
- Tier/gating flows only from `useShellLayout().phone`; never sniff platform or user-agent.
- Reduced motion (`prefers-reduced-motion: reduce`) renders the drawer open/close as an instant cut (no slide), applied additively via `motion-reduce:*` classes without modifying the shared `Sheet`.

**Block If:**
- `useShellLayout` no longer exposes a `phone` tier, or `SidebarPane` / the status-cluster primitives (`useWorstBridgeHealth`, `usePendingDraftCount`, `accountsStore.filterAccountId`, `primaryViewStore.setView`) are absent or renamed — the Story 13.1 projection contract is broken (HALT: missing 13.1 foundation).

**Never:**
- No bottom tab bar anywhere in the phone UI.
- No forked/second sidebar; do not restyle or reflow `SidebarPane` for phone beyond the drawer container sizing it.
- Do not build the merged full-screen Search surface — Story 13.4 owns it. The magnifier ships as the header affordance and, for now, opens the existing command palette (`commandPaletteStore.open()`); 13.4 repoints it.
- Do not add safe-area or keyboard-inset handling (`env(safe-area-inset-*)`, `--kb-inset`) — that is Story 13.5.
- Do not change the desktop docked-sidebar behavior (`app-shell` renders `SidebarPane` at ≥ 768 px, byte-for-byte identical).
- Do not build in-drawer Settings content; the settings gear opens the already-mounted shared `SettingsDialog`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Open via avatar | phone, level 0, avatar button tapped | leading `Sheet` opens rendering `SidebarPane` verbatim (primary views w/ amber approval count + bridge roll-up, SPACES, NETWORKS, account footer w/ settings gear + sync/offline); focus enters the drawer | — |
| Open via edge-swipe | phone, level 0, leading-edge pointer drag past 50 % width or a flick | drawer opens | released below threshold → no open |
| Edge-swipe at level ≥ 1 | phone, level 1/2, leading-edge drag | Story 13.2 back-swipe (unchanged); no drawer | — |
| Select view/filter/row | drawer open, a `SidebarPane` control changes primary view / space / network / account filter / selected room, or opens settings | drawer closes, the selection is applied, focus returns to the avatar button; the active filter chip appears above the chat list (existing `ChatListPane`) | — |
| Dismiss chrome | drawer open, scrim tap **or** Escape **or** trailing→leading swipe past threshold | drawer closes; focus returns to the avatar button | swipe below threshold → stays open |
| Bridge health dot | `useWorstBridgeHealth()` = `degraded`/`disconnected` | avatar shows a health dot in the matching `BRIDGE_HEALTH_DOT_CLASS` color | `healthy`/`null` → no dot (quiet) |
| Approval chip | `usePendingDraftCount()` = N | N = 0 → no chip; N > 0 → amber chip showing N; tap → `primaryViewStore.setView("approval")` (closes the drawer if open) | — |
| Account-filter cue | `accountsStore.filterAccountId` | set → avatar renders that account's avatar as the filter cue; `null` → neutral all-accounts avatar | — |
| Magnifier / compose | tap magnifier / tap compose | magnifier → `commandPaletteStore.open()` (interim; 13.4 repoints); compose → `newChatStore.open()` | — |
| Reduced motion | `prefers-reduced-motion: reduce`, drawer toggled | drawer appears/disappears as a cut (no slide), all controls still function | — |
| No tab bar | any phone level | no bottom tab bar element is rendered | — |

</intent-contract>

## Code Map

- `src/components/layout/phone-shell.tsx` -- MODIFY: render `PhoneInboxHeader` + `LeadingDrawer` at level 0; add a level-0 leading-edge swipe-to-open zone (mirrors the 13.2 `w-5` pointer pattern, active only at level 0); restore focus to the drawer button on close.
- `src/components/layout/phone-inbox-header.tsx` -- NEW: the 52 px level-0 header (`h-[var(--phone-header)]`): leading avatar drawer button (bridge-health dot overlay + account-filter cue) + trailing amber Approval chip + magnifier + compose.
- `src/components/layout/leading-drawer.tsx` -- NEW: `Sheet side="left"` (sized to the sidebar's width, full-height so the account footer pins to the bottom) wrapping `SidebarPane collapsed={false}` verbatim; close-on-select effect; `motion-reduce:*` cut.
- `src/lib/stores/leading-drawer.ts` -- NEW: tiny always-mounted-overlay store (`isOpen`/`open`/`close`/`toggle`), mirroring `command-palette.ts` / `new-chat.ts`.
- `src/components/layout/sidebar-pane.tsx` -- REUSE unchanged (`SidebarPane({ collapsed })`); read-only.
- `src/components/ui/sheet.tsx` -- REUSE: shadcn `Sheet` with `side="left"`; read-only.
- `src/components/ui/avatar.tsx` -- REUSE: `Avatar` + `AvatarBadge` for the avatar + health-dot overlay.
- `src/lib/stores/bridge-health.ts` -- `useWorstBridgeHealth()`; read only.
- `src/lib/bridges.ts` -- `BRIDGE_HEALTH_DOT_CLASS` / `BRIDGE_HEALTH_LABEL` for the dot color/aria.
- `src/lib/stores/drafts.ts` -- `usePendingDraftCount()`; read only.
- `src/lib/stores/primary-view.ts` -- `setView("approval")` for the Approval-chip deep link.
- `src/lib/stores/accounts.ts` -- `filterAccountId` for the account-filter cue; read only.
- `src/lib/stores/new-chat.ts` -- `open()` for the compose button.
- `src/lib/stores/command-palette.ts` -- `open()` interim magnifier target (13.4 repoints).
- `src/hooks/use-reduced-motion.ts` -- REUSE (13.2) for the reduced-motion decision if a JS gate is preferable to `motion-reduce:*`.

## Tasks & Acceptance

**Execution:**
- [x] `src/lib/stores/leading-drawer.ts` + `src/lib/stores/leading-drawer.test.ts` -- NEW zustand store (`isOpen: false`, `open`/`close`/`toggle`) following the `command-palette.ts` shape; tests cover the three transitions -- one source of drawer open-state usable from the header button and the edge-swipe zone.
- [x] `src/components/layout/leading-drawer.tsx` + `src/components/layout/leading-drawer.test.tsx` -- NEW: render `<Sheet side="left" open={isOpen} onOpenChange=…>` containing `<SidebarPane collapsed={false} />` verbatim, drawer content sized to the sidebar width (~260 px, capped for narrow devices) and full-height; a close-on-select `useEffect` that closes the drawer when primary view / active space / active network / `filterAccountId` / selected room / settings-open transitions while open (skip the initial render, compare by value); `motion-reduce:animate-none motion-reduce:transition-none` on the content for the reduced-motion cut; a trailing→leading swipe-to-close pointer handler on the content. Tests: sidebar sub-sections render; selecting a view closes the drawer; scrim/Escape close; swipe past threshold closes and below cancels -- the reused-verbatim drawer with correct dismissal.
- [x] `src/components/layout/phone-inbox-header.tsx` + `src/components/layout/phone-inbox-header.test.tsx` -- NEW: `PhoneInboxHeader({ drawerButtonRef })` renders a `h-[var(--phone-header)]` bar. Leading: an avatar `button` (`aria-label="Open navigation"`, ≥ 44 pt, forwards `drawerButtonRef`) that calls `leadingDrawerStore.open()`, showing the `filterAccountId` account's avatar when filtered else a neutral all-accounts avatar, with an `AvatarBadge` bridge-health dot from `useWorstBridgeHealth()` (hidden on `healthy`/`null`). Trailing cluster: an amber Approval chip shown only when `usePendingDraftCount() > 0` (label includes the count, tap → `primaryViewStore.getState().setView("approval")`), a magnifier button (`aria-label="Search"` → `commandPaletteStore.getState().open()`), and a compose button (`aria-label="New chat"` → `newChatStore.getState().open()`). Tests: dot shows only for unhealthy; chip shows only when count > 0 and deep-links; filtered vs unfiltered avatar; magnifier/compose fire the right store; all targets have accessible names -- the quiet-when-healthy status cluster.
- [x] `src/components/layout/phone-shell.tsx` -- MODIFY: at level 0 render `PhoneInboxHeader` above `ChatListPane` (exactly one 52 px bar) and mount `LeadingDrawer`; add a leading-edge (~20 px, `w-5`) pointer zone active **only at level 0** that opens the drawer past a 50 %/flick threshold (reuse the 13.2 pointer math; do not add it at level ≥ 1 where the back-swipe lives); hold the `drawerButtonRef` and focus it whenever the drawer transitions from open→closed (UX-DR28). Do not touch the level ≥ 1/2 `PhoneHeader` wiring.
- [x] `src/components/layout/phone-shell.test.tsx` -- extend: level 0 shows exactly one header with the status cluster and no bottom tab bar; avatar tap opens the drawer; level-0 leading-edge swipe opens it and level ≥ 1 edge-swipe still pops (unchanged); selecting a drawer row closes the drawer and returns focus to the avatar button; Escape/scrim close and restore focus; reduced-motion renders the cut -- guards the level-0 integration and the 13.2 non-regression.

**Acceptance Criteria:**
- Given a phone viewport at the Inbox (level 0), when it renders, then exactly one 52 px header shows (leading avatar drawer button + trailing Approval-chip-when-pending + magnifier + compose), `ChatListPane` renders below unchanged, and no bottom tab bar exists.
- Given the drawer is open, then the reused `SidebarPane` renders verbatim with all sections (primary views incl. the amber approval count and bridge-health roll-up, SPACES, NETWORKS chips, account footer with the settings gear and sync/offline status), it is a focus-trapping modal, and no forked sidebar component was introduced.
- Given any drawer selection (view, space, network, account filter, or room) or dismissal (scrim, Escape, swipe), then the drawer closes, the selection is applied (the active filter chip appears above the chat list as on desktop), and focus returns to the avatar drawer button.
- Given a desktop/tablet viewport (≥ 768 px), then behavior is unchanged — `SidebarPane` stays docked, no phone header/drawer mounts — and the `app-shell` / `sidebar-pane` suites pass.
- Given `bun run check`, then Biome + `tsc --noEmit` + vitest (including the new `leading-drawer`, `phone-inbox-header`, `leading-drawer` store, and extended `phone-shell` suites) pass.

## Spec Change Log

_No bad_spec loopback occurred; the spec was implemented as written._

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 1, low 0)
- defer: 0
- reject: 12
- addressed_findings:
  - `[medium]` `[patch]` Drawer-open swipe zone overlapped the avatar button: the level-0 `edge-swipe-open` zone was `absolute inset-y-0 left-0 z-10 w-5`, shadowing the leading ~16 px of the 44 pt avatar drawer button (header `px-1`), so a tap there started a below-threshold swipe instead of activating the button. Constrained the zone to `top-[var(--phone-header)] bottom-0` so it covers only the chat-list leading edge; the avatar tap area is now unobstructed and the list-edge swipe still opens the drawer. Added a `phone-shell` regression test asserting the zone starts below the header (never `inset-y-0`).

Rejected (12): open-swipe half-width/flick commit threshold "too large" and the open gesture lacking live finger-tracking (both by-spec — the Design Notes define a discrete threshold gesture mirroring the accepted 13.2 back-swipe; on-device feel folds into Epic 14/15); focus "lost" on drawer close (verified non-issue — on a room-select close the `[level]` push effect focuses the level-1 back button first and the later `drawerButtonRef.focus()` is a no-op on the now-`inert` level-0 button, so it never blurs; on the stay-at-level-0 path focus returns to the avatar button exactly per UX-DR28); health-dot `bg-primary` bleed-through (false positive — empirically `cn`/tailwind-merge dedupes `bg-primary`→`bg-bridge-degraded` and `bg-blend-color`→`bg-blend-normal`); stale account-filter shows the neutral avatar (pre-existing account-filter edge, not caused here; the header faithfully mirrors `filterAccountId`); `openPointerRef` wedge on silent capture loss (already mitigated by `onLostPointerCapture`, parity with the accepted 13.2 back-swipe); Approval chip lacking a "99+" cap (unrealistic draft volumes; cosmetic); magnifier `aria-label="Search"` opening the command palette (by-spec interim — the palette is the desktop search/⌘K surface; 13.4 repoints); `settingsOpen` closing the drawer under the Settings dialog (by-spec — the I/O matrix lists "opens settings" as a close trigger); same-tick coalesced open+select leaving the drawer open (unreachable — the drawer only opens via user gesture, never programmatically alongside a selection); close-swipe zero-width `getBoundingClientRect` fallback to `window.innerWidth` (SheetContent is visible/non-zero by the time it is swipe-closable; parity with accepted 13.2 pattern); unused public `toggle()` desync (speculative future caller); and the tap-vs-swipe / stuck-ref test-coverage gap (folded into the patch's new regression test).

## Design Notes

**Verbatim sidebar reuse.** `SidebarPane` is a self-contained presentational nav (only prop: `collapsed`) with app-lifetime store subscriptions and no layout-context assumptions. The drawer mounts it with `collapsed={false}` (the drawer itself is the hidden/collapsed state) and sizes the `Sheet` content to the sidebar's natural ~260 px width, full-height so the account footer's `mt-auto` still pins to the bottom. The macOS traffic-light inset inside `SidebarPane` is an empty 12 px top spacer — benign in the drawer, and safe-area padding is Story 13.5, so `SidebarPane` needs **no** change. The shared `SettingsDialog` stays app-shell-mounted, so the footer's settings gear works unchanged.

**Overlay store pattern.** The drawer is opened from two places (the header avatar and the level-0 edge-swipe) and closed by an effect on selection, so a tiny `leadingDrawerStore` (mirroring `command-palette.ts` / `new-chat.ts`, the codebase's always-mounted-overlay idiom) is cleaner than threading props through `PhoneShell`.

**Close-on-select without forking.** Rather than adding callbacks into `SidebarPane`, `LeadingDrawer` subscribes to the nav/filter stores and closes when any of `{primary view, active space, active network, filterAccountId, selected room, settings-open}` changes value while open (skipping the initial render, value-compared like the 13.2 DW-109 effect). This keeps `SidebarPane` byte-for-byte reused.

**Gestures with a radix drawer.** The `Sheet` is a radix Dialog, so the open/close swipes are discrete threshold gestures (open: leading→trailing at level 0; close: trailing→leading on the open content), not finger-tracked reveals — interactive tracking would require replacing the shared `Sheet`, which is out of scope. The Sheet animates its own slide; `motion-reduce:*` classes turn it into a cut without forking the component. Focus returns to the avatar button on every close (radix auto-restores when opened from the button; the explicit focus effect covers the edge-swipe-open path).

**Magnifier scoping.** Story 13.4 owns the merged full-screen Search surface and depends on this story's magnifier affordance. To avoid a dead control, the magnifier opens the existing command palette here; 13.4 repoints it — the same "ship the container, owning story wires the destination" pattern 13.2 used for the ⋯ overflow.

## Verification

**Commands:**
- `bun run check` -- expected: Biome + `tsc --noEmit` + vitest all green, including the new `leading-drawer`, `phone-inbox-header`, and `leading-drawer` store suites and the extended `phone-shell` suite; desktop `app-shell` / `sidebar-pane` suites unchanged.
- `bun run test -- phone-shell phone-inbox-header leading-drawer sidebar-pane` -- expected: the touched suites pass in isolation (jsdom matchMedia + pointer-capture polyfills exercise the phone tier, the drawer, and the edge-swipe).

## Auto Run Result

Status: done

### Summary
Added the iPhone Inbox (level-0) chrome that Stories 13.1/13.2 left bare. A new 52 px `PhoneInboxHeader` sits above the reused `ChatListPane` with a leading avatar drawer button (worst-state bridge-health dot overlay + account-filter cue, quiet when healthy) and a trailing cluster — an amber Approval chip that shows the pending-Draft count only when > 0 and deep-links to the Approval Pane, a magnifier (interim: opens the command palette; Story 13.4 repoints it), and a compose button. The entire desktop `SidebarPane` renders **verbatim** (`collapsed={false}`, no fork) inside a leading `Sheet` drawer opened by the avatar button or a level-0 leading-edge swipe (the edge 13.2 reserved), backed by a tiny `leadingDrawerStore`. Selecting any view/filter/room or dismissing (scrim, Escape, swipe-to-close) closes the drawer, applies the selection, and returns focus to the avatar button (UX-DR28); reduced-motion renders the drawer as a cut. Desktop/tablet ≥ 768 px is untouched (`app-shell.tsx` / `sidebar-pane.tsx` unchanged). No bottom tab bar.

### Files changed
- `src/lib/stores/leading-drawer.ts` — NEW always-mounted-overlay store (`isOpen`/`open`/`close`/`toggle`), modeled on `command-palette.ts`.
- `src/lib/stores/leading-drawer.test.ts` — NEW open/close/toggle + default-closed coverage.
- `src/components/layout/leading-drawer.tsx` — NEW controlled `Sheet side="left"` (~260 px, `max-w-[85vw]`, full-height, `p-0`) wrapping `SidebarPane collapsed={false}` verbatim; value-signature close-on-select effect (baselined on open); `motion-reduce:*` cut; trailing→leading swipe-to-close; visually-hidden dialog title.
- `src/components/layout/leading-drawer.test.tsx` — NEW: verbatim sidebar sections, modal, close on view/filter/room select, no-close on initial open, Escape close, swipe close/cancel, reduced-motion class.
- `src/components/layout/phone-inbox-header.tsx` — NEW 52 px status-cluster header.
- `src/components/layout/phone-inbox-header.test.tsx` — NEW quiet-when-healthy cluster coverage (dot only when unhealthy, chip only > 0 + deep-link, filtered vs neutral avatar, magnifier/compose fire, accessible names, no tab bar).
- `src/components/layout/phone-shell.tsx` — MODIFY: render `PhoneInboxHeader` + mount `LeadingDrawer` at level 0; level-0-only leading-edge drawer-open swipe zone (below the header after the review patch); focus the drawer button on open→closed.
- `src/components/layout/phone-shell.test.tsx` — MODIFY: level-0 header/no-tab-bar, avatar-tap open, edge-swipe open (+ below-threshold no-op), level ≥ 1 back-swipe non-regression, close+focus-return on select/Escape, reduced-motion cut, and the open-zone-below-header regression test.

### Review findings breakdown
- Patches applied: 1 — the drawer-open swipe zone overlapped the avatar button's leading ~16 px (medium); constrained it below the header. See the Review Triage Log.
- Deferred: 0.
- Rejected: 12 (by-spec, already-mitigated / 13.2-parity, unreachable, or verified false positives — including the health-dot color bleed, disproved empirically via tailwind-merge). See the Review Triage Log.
- intent_gap: 0, bad_spec: 0.

### Follow-up review
`followup_review_recommended: false` — the single fix is a localized, well-understood CSS-geometry change (swipe zone moved below the header) with no API/security/data-model/behavior-re-derivation impact, verified green and guarded by a new regression test.

### Verification
`bun run check` — green: Biome clean (284 files), `tsc --noEmit` clean, vitest 102 files / 1026 tests passed, core-tauri-free convention check passed. Run independently by the main loop after implementation and again after the review patch.

### Residual risks
- Push/pop and drawer slide smoothness plus the interactive edge-swipe feel (including the deliberately discrete, non-finger-tracked drawer-open gesture) are only verifiable on a real WKWebView/device; per the epic, on-device confirmation folds into the Epic 14/15 hardening + SM-8 dogfooding gate, not this story's acceptance.
- The magnifier opens the command palette as an interim search entry; Story 13.4 owns the merged full-screen Search surface and repoints it.
- Safe-area / keyboard-inset padding for the header and drawer is intentionally deferred to Story 13.5.
