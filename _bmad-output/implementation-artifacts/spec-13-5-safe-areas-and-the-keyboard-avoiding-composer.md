---
title: 'Safe Areas and the Keyboard-Avoiding Composer'
type: 'feature'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: 'c6c3a074eb3e8490ff2b66b23df99da05d2dde63'
final_revision: 'e7e69b3742bc1d971254e737797bb4658b5b0282'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-13-context.md'
warnings:
  - oversized
---

<intent-contract>

## Intent

**Problem:** The phone tier (< 768 px, Stories 13.1–13.4) renders as if the viewport were a plain rectangle: the header would slide under the notch/Dynamic Island, the composer would sit under the home indicator, and the on-screen keyboard would cover the composer entirely — there is no `viewport-fit=cover`, no `env(safe-area-inset-*)` plumbing, no keyboard inset, and the shared composer carries no phone touch/keyboard deltas. Edge-to-edge, keyboard-aware rendering is FR-59 / UX-DR21 / UX-DR25.

**Approach:** Expose the iOS safe-area insets as CSS vars via `viewport-fit=cover` and pad the phone header, composer, drawer, search surface, and overlays with them; drive a `--kb-inset` CSS var from a `visualViewport` hook so the bottom-anchored composer floats above the keyboard (`bottom: calc(var(--kb-inset,0px) + env(safe-area-inset-bottom))`); keep the timeline bottom-pinned with `overscroll-behavior: contain`; and apply the phone composer deltas (≥ 44 pt send button, on-screen return = newline, 5-line autogrow cap, `+` attach, undo-send pill above the composer) conditionally on `useShellLayout().phone` — never a forked composer.

## Boundaries & Constraints

**Always:**
- Gate every phone-only behavior on `useShellLayout().phone`; never sniff `navigator.userAgent`, platform, or build flags.
- Reuse the existing shared composer/timeline/drawer/search components with conditional classes via `cn()` — no forked "mobile" component and no second visual language; only insets + touch sizing change.
- Insets flow from CSS `env(safe-area-inset-*)` (populated by `viewport-fit=cover`), surfaced as theme CSS vars in `src/index.css` and applied to header, composer footer, drawer, search surface, and full-screen overlays in both orientations.
- `--kb-inset` is driven only by a `visualViewport` hook (resize/scroll listeners), active on the phone tier, cleaning up its listeners on unmount and no-op when `window.visualViewport` is absent (desktop/jsdom-without-stub).
- A timeline already scrolled to bottom stays pinned to bottom across keyboard open and dismiss with no stranded offset; the timeline scroller uses `overscroll-behavior: contain`.
- Phone composer deltas: send button hit target ≥ 44 pt (primary-tinted; tap is the FR-41 approval trigger), on-screen return inserts a newline (send is button-only on phone), autogrow caps at 5 lines then scrolls, attach via a `+` affordance (≥ 44 pt) presenting the native iOS picker, undo-send pill floats above the composer with tap replacing ⌘⇧Z.
- Reduced motion (`prefers-reduced-motion: reduce`) leaves inset/keyboard layout instantaneous (these are layout, not animation — no transitions introduced on the kb-inset shift).

**Block If:**
- Edge-to-edge rendering is found to *require* the native `contentInsetAdjustmentBehavior = .never` (i.e. `viewport-fit=cover` + our fixed, non-body-scrolling shell still leaves an unstyled band or a double top inset) — because there is **no committed native seam** to set it: `gen/apple/Sources/keeper/main.mm` only calls `ffi::start_app()` (the WKWebView is wry-owned), and `unsafe_code = "deny"` forbids the objc2 route from Rust. HALT (architecture: no native webview-config seam for AD-32).
- `useShellLayout`, `useReducedMotion`, the shared composer, or the conversation-pane scroll container are absent or renamed — the reuse contract is broken. HALT (missing reuse target).

**Never:**
- No forked/second composer, timeline, or header; no re-implemented autogrow/draft/typing/undo-send logic.
- No change to desktop/tablet (≥ 768 px) behavior: composer Enter=send, send-button size, 8-line autogrow cap, and desktop padding stay byte-for-byte; the phone deltas are additive and tier-gated.
- No bottom tab bar. No pull-to-refresh (Story 13.6). No hand-edit of regenerated Swift/`.pbxproj`, and no `unsafe` Rust to reach the WKWebView.
- Do not attempt to distinguish a hardware keyboard from the soft keyboard at runtime (WKWebView exposes no reliable signal) — the phone default is return = newline; on-device hardware-keyboard Enter-to-send folds into Epic 14/15.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Safe-area top | phone, notched viewport, `viewport-fit=cover` | header + inbox header padded by `env(safe-area-inset-top)`; no content under the notch/status bar in portrait or landscape | env unsupported → var resolves 0, layout unbroken |
| Safe-area bottom | phone, home-indicator device, no keyboard | composer footer padded by `env(safe-area-inset-bottom)`; nothing under the home indicator | — |
| Keyboard opens | phone, composer focused, `visualViewport` shrinks | `--kb-inset` set to the covered height; composer rises to `calc(var(--kb-inset)+env(safe-area-inset-bottom))`; a bottom-pinned timeline stays pinned | no `visualViewport` → `--kb-inset` stays `0px`, composer at safe-bottom only |
| Keyboard dismiss | phone, `visualViewport` restores | `--kb-inset` returns to `0px`; composer settles at safe-bottom; no stranded offset or overshoot | — |
| On-screen return | phone, textarea focused, Enter pressed (no shift) | a newline is inserted; the message is NOT sent (send is button-only on phone) | IME composing (`isComposing`) → default, never sends |
| Send tap | phone, non-empty composer, send button tapped | message sends (FR-41 approval trigger); button hit target ≥ 44 pt | empty/whitespace-only → disabled, no send |
| Autogrow | phone, > 5 lines typed | textarea grows to 5 lines then scrolls internally | — |
| Desktop tier | ≥ 768 px | composer Enter=send, default button size, 8-line cap, no safe-area/kb padding — byte-for-byte as before | — |

</intent-contract>

## Code Map

- `index.html` -- MODIFY: viewport meta → `width=device-width, initial-scale=1.0, viewport-fit=cover, interactive-widget=resizes-content`.
- `src/index.css` -- MODIFY: expose safe-area theme CSS vars in `:root` (`--safe-top/right/bottom/left = env(safe-area-inset-*)`) and a default `--kb-inset: 0px`; add an `overscroll-contain` utility class (or apply via arbitrary value). Body already `bg-background` (no-flash) — leave the theme/launch background as-is.
- `src/hooks/use-keyboard-inset.ts` -- NEW: effect that, when enabled, subscribes to `window.visualViewport` `resize`/`scroll`, computes the keyboard-covered height (`layoutViewportHeight - visualViewport.height - visualViewport.offsetTop`, clamped ≥ 0), writes it to `--kb-inset` on `document.documentElement`, and restores `0px` + removes listeners on cleanup; no-op when `visualViewport` is undefined.
- `src/hooks/use-keyboard-inset.test.ts` -- NEW: mock `window.visualViewport`, fire resize, assert `--kb-inset` set/cleared and listeners cleaned up; no-op path when unset.
- `src/components/layout/phone-shell.tsx` -- MODIFY: call `useKeyboardInset({ enabled: phone })` (only the phone tier drives the var).
- `src/components/layout/phone-header.tsx`, `phone-inbox-header.tsx` -- MODIFY: pad top by `var(--safe-top)` above the 52 px `--phone-header` content (header total = safe-top + 52 px); keep ≥ 44 pt targets.
- `src/components/layout/leading-drawer.tsx`, `phone-search-surface.tsx` -- MODIFY: pad the sheet/overlay content by the relevant safe-area vars (top and bottom) so drawer/search never collide with notch or home indicator.
- `src/components/chat/composer.tsx` -- MODIFY: tier-gated phone deltas via `useShellLayout().phone` + `cn()` — ≥ 44 pt send + attach (`+`) buttons, 5-line autogrow cap, and phone `Enter` → newline (skip the send branch when `phone`); desktop path untouched.
- `src/components/layout/conversation-pane.tsx` -- MODIFY: on phone, pad the composer footer bottom by `calc(var(--kb-inset,0px) + var(--safe-bottom))`; add `overscroll-behavior: contain` to the timeline scroll container; preserve the bottom-pin logic across keyboard open/close.
- `src/components/chat/undo-send-pill.tsx` -- REUSE (read-only): already floats above the composer within the footer stack; the footer inset carries it above the keyboard.
- `src/hooks/use-shell-layout.ts`, `src/hooks/use-reduced-motion.ts` -- REUSE (read-only): `.phone` tier gate; motion pattern.
- `src/test/setup.ts` -- MODIFY (if needed): add a `visualViewport` stub for keyboard-inset tests (mirrors the existing `matchMedia`/pointer stubs).
- `gen/apple/*` -- NO CODE CHANGE this story (see Block If / Design Notes): `viewport-fit=cover` is the effective inset lever; `contentInsetAdjustmentBehavior` has no committed seam.

## Tasks & Acceptance

**Execution:**
- [x] `index.html` -- MODIFY the viewport meta to add `viewport-fit=cover` (exposes `env(safe-area-inset-*)`) and `interactive-widget=resizes-content` (honored by Chromium webviews; ignored by WKWebView, which uses the `--kb-inset` path) -- the single edge-to-edge enable point.
- [x] `src/index.css` -- MODIFY to expose `--safe-top/right/bottom/left` from `env(safe-area-inset-*)` and a default `--kb-inset: 0px` on `:root`, plus a reusable `overscroll-contain` affordance -- one source of the inset tokens the components consume.
- [x] `src/hooks/use-keyboard-inset.ts` + `.test.ts` -- NEW `visualViewport`-driven hook writing `--kb-inset` to the document root; clamped ≥ 0, cleaned up on unmount, no-op without `visualViewport`. Tests cover set-on-resize, clear-on-dismiss, cleanup, and the no-op path -- the keyboard-avoidance engine.
- [x] `src/components/layout/phone-shell.tsx` -- MODIFY to mount `useKeyboardInset({ enabled: phone })`; extend `phone-shell.test.tsx` to assert the hook runs only on the phone tier -- wires the engine to the phone stack.
- [x] `src/components/layout/phone-header.tsx` + `phone-inbox-header.tsx` (+ tests) -- MODIFY to add `var(--safe-top)` top padding; assert the safe-area padding class/style is present on the phone tier -- notch-safe headers.
- [x] `src/components/layout/leading-drawer.tsx` + `phone-search-surface.tsx` (+ existing tests stay green) -- MODIFY to pad content by safe-area vars -- notch/home-indicator-safe overlays.
- [x] `src/components/chat/composer.tsx` (+ `composer.test.tsx`) -- MODIFY: tier-gated ≥ 44 pt send/attach targets, 5-line autogrow cap, and phone `Enter`→newline (desktop Enter=send/8-line/size unchanged). Tests: phone Enter inserts newline & does not send; desktop Enter still sends; phone send button meets the 44 pt sizing class -- the composer deltas.
- [x] `src/components/layout/conversation-pane.tsx` (+ `conversation-pane.test.tsx`) -- MODIFY: phone composer-footer bottom inset `calc(var(--kb-inset,0px)+var(--safe-bottom))` + `overscroll-behavior: contain` on the timeline scroller; keep bottom-pin across keyboard toggles. Tests: footer carries the inset style on phone; scroller has overscroll-contain; desktop footer unchanged -- keyboard-avoiding composer placement.
- [x] `src/test/setup.ts` -- MODIFY only if the keyboard-inset tests need a shared `visualViewport` stub -- test infra for the new hook.

**Acceptance Criteria:**
- Given a phone viewport with `viewport-fit=cover`, when the app renders on a notched device, then the header, composer, drawer, search surface, and overlays are padded by `env(safe-area-inset-*)` in both orientations with no unstyled band at the notch or home indicator, and the launch/window background tracks the active appearance with no flash.
- Given the on-screen keyboard opens then closes on the phone, when `visualViewport` changes, then `--kb-inset` rises to the covered height and returns to `0px`, the composer sits at `calc(var(--kb-inset,0px)+env(safe-area-inset-bottom))`, a bottom-pinned timeline stays pinned, and dismissal leaves no stranded offset (`overscroll-behavior: contain` set).
- Given the phone composer, then the send button hit target is ≥ 44 pt (tap = FR-41 approval trigger), the on-screen return inserts a newline (send is button-only), autogrow caps at 5 lines then scrolls, attach is a ≥ 44 pt `+`, and the undo-send pill floats above the composer with tap replacing ⌘⇧Z.
- Given a desktop/tablet viewport (≥ 768 px), then the composer (Enter=send, default button size, 8-line cap) and layout behave byte-for-byte as before — the phone deltas are tier-gated and additive.
- Given `bun run check`, then Biome + `tsc --noEmit` + vitest pass, including the new `use-keyboard-inset` hook and the extended composer / conversation-pane / phone-shell / phone-header suites, with desktop suites green.

## Design Notes

**Keyboard-avoidance decision (recorded per epic).** Two levers were evaluated. `interactive-widget=resizes-content` resizes the *layout* viewport when the keyboard opens, so a bottom-anchored composer lifts with no JS — but WebKit/WKWebView does not implement `interactive-widget` (as of 2026), so on iOS it is inert. We therefore keep **both**: the meta flag (a free win on Chromium webviews, the "any sub-768px webview" test target) *and* the `visualViewport`-driven `--kb-inset` (the effective path on WKWebView). They do not double-count: where the layout viewport resizes, `visualViewport.height` matches it and the computed `--kb-inset` is ≈ 0; where it does not (WKWebView), `--kb-inset` carries the full lift.

**Native inset seam (why no gen/apple change).** AD-32 assumed a committed `gen/apple` Swift patch for `contentInsetAdjustmentBehavior = .never`, but the actual architecture has no seam for it: `main.mm` merely calls `ffi::start_app()` and the WKWebView is created by wry inside Rust — there is no Swift view controller to patch, and reaching the webview's `scrollView` from Rust needs an objc2 message send that `unsafe_code = "deny"` forbids. For our fixed, non-body-scrolling shell, `viewport-fit=cover` is the necessary and sufficient lever to populate `env(safe-area-inset-*)` and render edge-to-edge; the scroll-view auto-inset is moot because the root webview never scrolls (inner panes do). The Block If guards the case where a real device proves `.never` is nonetheless required.

**Composer Enter on phone.** WKWebView cannot reliably tell a hardware keyboard from the soft keyboard, so the phone tier takes the safe touch default — `Enter` inserts a newline and the ≥ 44 pt send button is the sole send path (and the FR-41 approval trigger). The existing `isComposing` IME guard is preserved. Desktop keeps Enter=send. Honoring a paired hardware keyboard's Enter-to-send is deferred to on-device Epic 14/15 hardening.

**Reuse, not fork.** Every delta is a tier-gated `cn()` class or a conditional branch inside the shared composer/pane/header — the same "new arrangement, reused internals" discipline as 13.1–13.4. Safe-area padding is CSS-var driven so it is inert (vars resolve to 0) on desktop and in jsdom, keeping desktop suites byte-for-byte and phone tests asserting class/style presence rather than computed pixels (jsdom cannot evaluate `env()`).

## Verification

**Commands:**
- `bun run check` -- expected: Biome + `tsc --noEmit` + vitest all green, including the new `use-keyboard-inset` hook and extended `composer` / `conversation-pane` / `phone-shell` / `phone-header` / `phone-inbox-header` suites; desktop composer/search/app-shell suites unchanged and green.
- `bun run test -- use-keyboard-inset composer conversation-pane phone-shell phone-header phone-inbox-header` -- expected: the touched suites pass in isolation (mocked `visualViewport` + `matchMedia` exercise the phone tier, the kb-inset set/clear, and the composer Enter/size/autogrow deltas).

**Manual checks (no device required for acceptance):**
- In a sub-768px webview (e.g. resized Chromium / iOS Simulator), confirm the header clears the notch, the composer clears the home indicator, and focusing the composer lifts it above the on-screen keyboard with the timeline staying bottom-pinned. On-device WKWebView confirmation of the inset/keyboard feel folds into the Epic 14/15 hardening + SM-8 dogfooding gate, not this story's acceptance.

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 1, low 3)
- defer: 1
- reject: 13
- addressed_findings:
  - `[medium]` `[patch]` `overscroll-behavior: contain` shipped un-gated on the shared timeline scroller, touching the desktop className (behaviorally inert — the fixed shell has no scrollable ancestor to chain to — but a byte-for-byte-contract concern). Fixed: gated behind the `phone` tier via `cn()`; desktop scroller className is unchanged.
  - `[low]` `[patch]` Keyboard-inset hook could go stale after an orientation change that updates `window.innerHeight` without emitting a `visualViewport` event. Fixed: added a `window` `resize` listener (removed on cleanup) that recomputes; new regression test.
  - `[low]` `[patch]` Pinch-zoom shrinks the visual viewport for zoom (not the keyboard), which would read as a phantom positive inset shoving the composer up. Fixed: `visualViewport.scale > 1` short-circuits `--kb-inset` to `0px`; new regression test.
  - `[low]` `[patch]` The desktop composer-footer test used a brittle exact-string `toBe` on the merged className. Fixed: replaced with the semantic invariant (`.not.toContain("kb-inset"/"safe-bottom")` + a positive `border-t` check).

Rejected (13): Chromium keyboard "double-count" (the `interactive-widget` path is engine-specific and the on-device target is WKWebView, which ignores the flag entirely — no double-count there; unverifiable without the non-target engine); global viewport meta affecting desktop (inherent to the HTML-level mechanism the spec prescribes; desktop has no virtual keyboard, so it is inert); phone hardware-keyboard loses Enter-to-send (explicitly by-spec — WKWebView can't detect a hardware keyboard, folds to Epic 14/15); phone edit-mode Enter inserts newline (by-design — the ≥44pt "Save" button commits the edit, consistent with button-only send on touch); Plus/Paperclip icon swap focus/a11y and small-glyph-in-44pt-box (button element identity is stable across the child-icon swap so focus is retained; the box meets ≥44pt; glyph size matches the ghost icon-button idiom); send `min-w-11` vs attach `size-11` idiom mismatch (both satisfy ≥44pt; informational); global `--kb-inset` var latent coupling (no second consumer today; desktop carries no inset class); unthrottled `visualViewport` scroll listener (premature perf optimization; on-device perf folds to Epic 14/15); initial ResizeObserver re-pin callback (only re-anchors when already near-bottom → a no-op) and tier-flip-while-keyboard-open stranding (foldable/resizable-window-only, extraordinarily rare on a phone that can't cross 768px); redundant `,0px` calc fallback (harmless defensive redundancy).

The one defer (DW: keyboard-resize bottom-pin ResizeObserver vs simultaneous history-prepend scroll-anchor race) is recorded in `deferred-work.md` — a narrow phone-only timing race best tuned against on-device WKWebView behavior in Epic 14/15.

## Auto Run Result

Status: done

### Summary
Delivered the phone tier's edge-to-edge safe-area rendering and keyboard-avoiding composer (FR-59, UX-DR21/DR25). `viewport-fit=cover` (plus `interactive-widget=resizes-content` for Chromium webviews) exposes `env(safe-area-inset-*)`, surfaced as `--safe-top/right/bottom/left` theme vars in `index.css`; the phone header, inbox header, leading drawer, and full-screen search surface pad by them so nothing slides under the notch or home indicator. A new `useKeyboardInset` hook drives a `--kb-inset` CSS var from `visualViewport` (covered height = `max(0, innerHeight − visualViewport.height − offsetTop)`, guarded against pinch-zoom and refreshed on window resize/rotation), and the phone composer footer floats at `calc(var(--kb-inset,0px) + var(--safe-bottom))` — carrying the undo-send pill and typing indicator above the keyboard — while the timeline scroller keeps a bottom-pinned view pinned across keyboard open/close via `overscroll-contain` (phone-gated) and a phone-only ResizeObserver re-pin. Composer phone deltas are all tier-gated on `useShellLayout().phone` via `cn()` (no fork): ≥44pt primary send button (the sole send path on touch = FR-41 trigger), on-screen return inserts a newline, ≥44pt `+` attach presenting the native picker, and a 5-line autogrow cap. Desktop/tablet (≥768px) is byte-for-byte unchanged. The native `contentInsetAdjustmentBehavior = .never` was intentionally not attempted: there is no committed native seam (the WKWebView is wry-owned; `main.mm` only calls `ffi::start_app()`; `unsafe_code = "deny"` forbids the Rust objc2 route), and `viewport-fit=cover` is the effective, sufficient inset lever for the fixed non-body-scrolling shell — recorded as a decision with a Block If that never triggered.

### Files changed
- `index.html` — viewport meta gains `viewport-fit=cover, interactive-widget=resizes-content`.
- `src/index.css` — NEW `--safe-top/right/bottom/left` (from `env()`, 0px fallback) + `--kb-inset: 0px` on `:root`.
- `src/hooks/use-keyboard-inset.ts` (+ `.test.ts`) — NEW `visualViewport`-driven `--kb-inset` engine (pinch-zoom + window-resize guarded, cleanup restores 0px); 9 tests.
- `src/components/layout/phone-shell.tsx` (+ test) — mounts `useKeyboardInset({ enabled: phone })`; gesture zones track the safe-top-taller header.
- `src/components/layout/phone-header.tsx` / `phone-inbox-header.tsx` (+ tests) — `pt-[var(--safe-top)]` above the 52px content row.
- `src/components/layout/leading-drawer.tsx` / `phone-search-surface.tsx` — safe-area padding on the sheet/overlay content.
- `src/components/chat/composer.tsx` (+ test) — tier-gated ≥44pt send/`+`-attach, Enter→newline on phone, 5-line autogrow cap; desktop untouched.
- `src/components/layout/conversation-pane.tsx` (+ test) — phone composer-footer `calc(--kb-inset + --safe-bottom)` inset, phone-gated `overscroll-contain`, phone-only bottom-pin ResizeObserver.

### Review findings breakdown
- Patches applied: 4 — un-gated `overscroll-contain` (medium), keyboard-inset orientation staleness (low), pinch-zoom phantom inset (low), brittle desktop-footer test (low). See the Review Triage Log.
- Deferred: 1 — the keyboard-resize re-pin vs history-prepend scroll-anchor race (`deferred-work.md`), on-device tuning for Epic 14/15.
- Rejected: 13 — by-spec, by-design, speculative/non-target, or null-effect. See the Review Triage Log.
- intent_gap: 0, bad_spec: 0.

### Follow-up review
`followup_review_recommended: false` — the four review patches are localized and low-consequence (a phone-gating of an inert-on-desktop class, two defensive `visualViewport` guards each covered by a new test, and a test-quality fix) with no API/security/data-model/behavior-re-derivation impact; desktop stays byte-for-byte.

### Verification
`bun run check` — green: Biome clean (294 files), `tsc --noEmit` clean, vitest **107 files / 1076 tests passed** (+18 over baseline's 1058), core-tauri-free convention check passed. Run independently by the main loop after implementation and again after the four review patches.

### Residual risks
- On-device WKWebView feel (keyboard lift timing, safe-area bands in both orientations, the bottom-pin across keyboard toggles, and any orientation/pinch-zoom edge) is only fully verifiable in the iOS Simulator / on a device; per the epic, that confirmation folds into Epic 14/15 hardening + the SM-8 dogfooding gate, not this story's acceptance.
- If a real device proves edge-to-edge rendering nonetheless needs `contentInsetAdjustmentBehavior = .never`, that requires a native webview-config seam that does not exist today (Block If) — a genuine architecture escalation for a later story, not a workaround.
- DW (deferred): a narrow phone-only scroll-anchor race between the keyboard-resize re-pin and a simultaneous history prepend.
