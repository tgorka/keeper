# Epic 13 Context: iPhone Shell — Single-Pane Navigation

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

This epic projects the existing desktop shell onto a phone-width viewport: same components, same design tokens, same IPC — just one new arrangement container. It delivers the navigation stack (Inbox → Room → Detail), the leading drawer that replaces the sidebar, a merged full-screen Search surface, a safe-area/keyboard-aware composer, complete touch idioms for every desktop interaction, and capability-honest surface hiding with an "On this iPhone" disclosure. Everything is pure frontend plus small native glue, verifiable in the iOS Simulator or any sub-768px webview — no physical device required. It runs after the Epic 12 on-device walking-skeleton gate (SM-7) passes, in parallel with Epic 14.

## Stories

- Story 13.1: Phone Layout Tier and Navigation Stack
- Story 13.2: Phone Header, Push/Pop Transitions, and Edge-Swipe Back
- Story 13.3: Leading Drawer with Status Cluster
- Story 13.4: Merged Full-Screen Search Surface
- Story 13.5: Safe Areas and the Keyboard-Avoiding Composer
- Story 13.6: Touch Idioms — Long-Press, Row Swipes, Pull-to-Refresh
- Story 13.7: Capability-Gated Surfaces and "On this iPhone" Disclosure

## Requirements & Constraints

- Below a 768px viewport, a third `phone` layout tier activates a single-pane stack: level 0 Inbox, level 1 Room, level 2 Detail. Desktop and tablet tiers at ≥768px must remain unchanged (regression-tested).
- The stack must reuse the existing InboxList/ChatView/DetailPanel component trees unchanged — no forked chat components — driven by the existing zustand selection state (`selectedRoomId`, detail-open). No routing library is introduced; `history.pushState` integration is an optional enhancer, not a dependency.
- A notification deep link `(account_id, room_id, event_id)` must set selection state and render at the correct stack level, with back leading to the Inbox and Inbox scroll position preserved. Opening a Chat on the phone must not auto-focus the composer.
- Every tappable target must be ≥44pt. No gesture may be the sole path to any action — row swipes need VoiceOver custom-action and long-press-menu duplicates; pull-to-refresh needs a "Sync now" equivalent.
- VoiceOver: every push moves focus to the new level's header (back button first in swipe order); every pop returns focus to the pushing element; the system escape gesture triggers back at every level.
- No bottom tab bar anywhere in the phone UI (explicit decision).
- Capability hiding must flow exclusively from the capabilities store, never platform/user-agent sniffing (enforced by a convention test); a disabled capability reached programmatically returns a clean "unsupported on this platform" `IpcError`.
- Search parity is a release gate: every registered palette action must be reachable from the phone Search "Actions" scope, with desktop-only actions unregistered via capabilities so no dead entries appear.
- Text sizing is rem-based so it holds up under roughly 130% Dynamic-Type-style scaling.
- Reduced-motion: push/pop transitions render as cuts instead of animated slides.

## Technical Decisions

- Phone tier is a third tier in the existing `useShellLayout` — a projection of existing zustand selection state, not a separate routing concept. Component/token reuse is absolute: no second visual language, forked components, or platform-specific restyling. Shared tokens: `phone-breakpoint` 768px, `touch-target-min` 44pt, `safe-area`, `--kb-inset`, `phone-header` 52px.
- Safe areas: `viewport-fit=cover` plus `contentInsetAdjustmentBehavior = .never` (via a `gen/apple` Swift patch added in Story 13.5, on top of what Epic 12 already established) expose `env(safe-area-inset-*)` as theme CSS vars, padding header, composer, drawer, sheets, and overlays in both orientations. Window/launch background matches the active theme with no flash.
- Keyboard avoidance: a `visualViewport`-driven `--kb-inset` CSS var positions the composer at `bottom: calc(var(--kb-inset, 0px) + env(safe-area-inset-bottom))`; evaluate `interactive-widget=resizes-content` as the simpler alternative and record the decision. Timeline scroller uses `overscroll-behavior: contain`; a bottom-pinned timeline stays pinned across keyboard open/dismiss with no stranded offsets.
- Capabilities: hiding is driven by the single `CapabilitiesVm` served over the IPC handshake at startup (established in Epic 12); this epic completes its UI-surface-hiding leg. `Platform::sidecar_path` already returns a clean Unsupported error on iOS for bbctl.
- `phone-header` carries the back chevron with the previous level's title. Room header order: back → avatar + network badge → name + Account chip → incognito chip (when applicable) → overflow (⋯: Search in chat, Mute ▸, Mention-only, Incognito for this Chat, Archive, Export). Tapping the identity block pushes Detail (the phone's replacement for ⌘I).
- Transitions: new level slides in from the trailing edge over ~250ms ease-out, the level beneath shifts back ~25% and dims; pop reverses. Edge-swipe back is interactive (tracks the finger, commits past 50% travel or on a flick) since WKWebView provides no native swipe-back for an in-page stack; at level 0 the leading edge is reserved for the drawer instead.
- Drawer: the entire desktop sidebar (primary views, SPACES, NETWORKS chips, account switcher footer, sync/offline status) renders verbatim inside a leading Sheet, opened via the Inbox header avatar button or edge-swipe at level 0 only; dismissal returns focus to the drawer button. Inbox header status cluster: avatar carries a worst-state bridge-health dot overlay, amber Approval chip shows pending-Draft count when >0, magnifier + compose trail — quiet when healthy.
- Search: full-screen, segmented Chats / Messages / Actions scopes on the same engines and ≤100ms/offline bars as desktop ⌘K/⌘⇧F. Opened via header magnifier or pull-down on the Inbox list (past the reveal threshold becomes pull-to-refresh — one continuous gesture axis). Typing `>` first jumps to Actions scope; in-chat search routes through Room overflow → "Search in chat".
- Composer deltas from desktop: ≥44pt primary-tinted send button (tap = the FR-41 approval trigger); on-screen return key inserts newline while hardware keyboard follows the desktop Enter setting; autogrow caps at 5 lines; attach via + → system photo library/camera/Files; undo-send pill tap replaces ⌘⇧Z.
- Touch idioms: long-press = right-click everywhere (identical ContextMenus); row swipes: trailing → Archive + More (mute ▸), leading → read/unread, full-swipe commits the first action; long-press-drag reorders Pins. Approval Pane rows: tap → inline editor, explicit per-row Approve button, trailing swipe → Discard with a 5s undo toast — still no approve-all.
- "On this iPhone" disclosure (Settings → About) states plainly: foreground-only sync, no bbctl, no global hotkey, updates by reinstall/7-day signature renewal, link to docs/ios.md. Settings → Archive & Storage notes the phone's Local Archive is excluded from backup; the Mac remains the durable exportable copy.

## Cross-Story Dependencies

- 13.1 depends on the Epic 12 SM-7 gate and the capabilities plumbing from Story 12.2; it establishes the stack container that every other story in this epic builds on.
- 13.2 (header/transitions/edge-swipe) depends on 13.1.
- 13.3 (drawer) depends on 13.1.
- 13.4 (Search) depends on 13.1 and 13.3 (header magnifier affordance).
- 13.5 (safe areas/keyboard) depends on 13.1 and 13.2 (header insets).
- 13.6 (touch idioms) depends on 13.1, 13.4 (shares the pull-down gesture axis with Search/pull-to-refresh), and 13.5 (scroll interplay with the keyboard-avoiding composer).
- 13.7 (capability-gated surfaces) depends on 13.1 and 13.3 (drawer/settings surfaces host the disclosure).
- This epic completes the surface-hiding leg of the capability mechanism whose handshake mechanism landed in Epic 12; on-device confirmation of anything built here folds into Epic 14/15 hardening and the SM-8 dogfooding gate, not into this epic's own acceptance.
