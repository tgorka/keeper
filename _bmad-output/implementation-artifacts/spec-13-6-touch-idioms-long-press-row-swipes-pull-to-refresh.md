---
title: 'Touch Idioms — Long-Press, Row Swipes, Pull-to-Refresh'
type: 'feature'
created: '2026-07-11'
status: 'done'
baseline_revision: 'b12dc4d3d5f9a8f470d5c5feb0c1e99493668d7c'
final_revision: '7b4ad286e368d69f8e800e5175994c71ae2827d9'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** On the phone tier (<768px) every desktop action is bound to a mouse/keyboard affordance — right-click ContextMenus, a hover action bar over message bubbles, drag-to-reorder Pins, ⌘-shortcuts on Approval rows. Touch users cannot triage: no long-press menus, no row swipes, no pull-to-refresh, and the Approval Pane and Pins strip have no touch path at all.

**Approach:** Add iOS touch idioms as **phone-gated, reuse-only layers** over the existing components — a shared long-press→ContextMenu bridge, a pointer-based row-swipe hook (following the Story 13.2 edge-swipe idiom), a pull-to-refresh that continues the Story 13.4 pull axis into a real sync-loop kick, and touch affordances on Approval rows and the Pins strip. Every gesture is backed by a non-gesture path (the long-press menu, an explicit button, or a "Sync now" palette action), and desktop/tablet stay byte-for-byte unchanged.

## Boundaries & Constraints

**Always:**
- Gate every delta on `useShellLayout().phone`; desktop/tablet (≥768px) render and behave byte-for-byte as before (regression-tested).
- Reuse the existing handlers/IPC only — `archiveRoom`/`unarchiveRoom`, `markRoomRead`/`markRoomUnread`, `chatNotifyModeSet`, `reorderPins`, `approveDraft`, `clearDraft`/`saveDraft`/`mirrorDraft`, and the message `onReact`/`onReply`/`onEdit`/`onDelete` the hover bar already wires. No forked components, no second visual language.
- Every tappable target ≥44pt (`size-11` / `min-h-11 min-w-11`) with an accessible name.
- **No gesture is the sole path to any action.** Every row-swipe action is also present in that row's long-press ContextMenu (inbox) or as an explicit ≥44pt button (Approval Approve/Discard); Pins long-press-drag reorder is duplicated by "Move up"/"Move down" items in the pin's long-press menu; pull-to-refresh is duplicated by the `sync-now` palette action.
- Suppress `-webkit-touch-callout` and `-webkit-tap-highlight-color` (and `user-select`/`touch-action` as needed) on long-press and swipe targets so the native callout/selection never fights the custom menus.
- `sync_now` calls the SDK's idempotent `SyncService::start()` (resume) on each **already-active** account handle — it never builds a second `Client`/`SyncService`, never force-activates signed-out or never-subscribed accounts, and never tears down live streams. This is the single sync-kick entry Epic 14-1 will later route foreground-resume through.
- Offline pull-to-refresh resolves the spinner into the **existing persistent offline pill** (`useShellOffline()`), never an error toast; an `IpcError` from `sync_now` is swallowed (best-effort) and clears the spinner with no toast.
- Reduced-motion (`useReducedMotion()`): swipe/pull animations degrade to instant state changes.

**Block If:**
- `matrix-sdk-ui` 0.18 `SyncService` cannot be resumed idempotently (i.e. `start()` is unsafe when already running and there is no restart/resume primitive) — then "kick the sync loop (the same operation as foreground resume)" requires the Epic 14-1 lifecycle entry that does not yet exist, and the sync-kick cannot be honestly built here.

**Never:**
- No new message actions (a Copy item, a Jump-to-original **menu entry**, or a Delete▸ submenu) beyond the desktop bubble's existing set — touch parity mirrors React-row / Reply / Edit-own / Delete-own only. Jump-to-original stays the existing clickable reply-quote affordance.
- No approve-all / bulk / select-all on the Approval Pane (FR-41 invariant).
- No Matrix/sync logic in TypeScript beyond invoking `sync_now`; Rust owns the SyncService.
- No second Rust sync-lifecycle entry competing with Epic 14-1; no routing library; no bottom tab bar.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Long-press a row / pin / bubble | ≥500ms press, movement <10px | The identical ContextMenu (bubble: phone-gated menu of React/Reply/Edit-own/Delete-own) opens at the press point; native callout suppressed | Lift <500ms or move >10px cancels → normal tap/scroll, no menu |
| Inbox trailing swipe | drag left past half-width | Archive + More(mute ▸) revealed; label appears past the half-swipe commit; full-swipe commits Archive (`archiveRoom`) | Released before threshold → snaps back, no action |
| Inbox leading swipe | drag right past half-width | read/unread toggles (`markRoomRead`/`markRoomUnread`, with the existing optimistic-unread overlay) | Snap back if released early; vertical scroll intent wins if |dy|>|dx| |
| Pull between thresholds | release in [reveal, refresh) at scroll-top | opens Search (Story 13.4 behavior, unchanged) | — |
| Pull past refresh threshold, online | pull ≥ refresh threshold, release | refresh spinner shows; `syncNow()` kicks each active `SyncService`; spinner clears on the next connection-status tick | `sync_now` `IpcError` → spinner clears, no toast |
| Pull past refresh threshold, offline | all signed-in accounts `offline` | spinner resolves into the persistent offline pill | never an error toast |
| Approval row tap (phone) | tap row body | inline editor opens (`onEnterEdit`) | — |
| Approval per-row Approve | tap ≥44pt Approve button | `approveDraft` through the single gate (clears only on success) | in-flight guard blocks double-approve |
| Approval trailing swipe | swipe left past threshold | Discard (`clearDraft` + mirror/marker) + the existing 5s undo toast; Undo restores via `saveDraft`+`mirrorDraft` | Undo within 5s restores the row |
| Pins long-press-drag | long-press then drag a pin | reorder preview; drop persists via `reorderPins` | Non-gesture path: "Move up"/"Move down" in the pin long-press menu; disabled when the account filter makes `pins` a partial subset (`reorderable=false`) |

</intent-contract>

## Code Map

- `src/hooks/use-long-press.ts` -- NEW: phone-gated hook returning pointer handlers that, on a ≥500ms stationary press, dispatch a synthetic `contextmenu` event at the press point (movement/scroll/second-pointer cancels); no-op off-phone. The bridge that opens Radix ContextMenus by touch.
- `src/hooks/use-long-press.test.ts` -- NEW: fake-timer tests for fire-on-hold, cancel-on-move, cancel-on-early-lift, and the off-phone no-op.
- `src/hooks/use-swipe-actions.ts` -- NEW: pointer-based horizontal swipe hook mirroring the `phone-shell` edge-swipe idiom (pointer capture, clamped `dx`, half-width commit, flick via `FLICK_MIN_DX_PX`/`FLICK_VELOCITY_PX_PER_MS`, vertical-intent bailout). Returns `{ dx, committing, handlers }` for leading/trailing actions.
- `src/hooks/use-swipe-actions.test.ts` -- NEW: pointer-event tests for reveal, half-swipe label, full-swipe commit, snap-back, and vertical-scroll bailout.
- `src/index.css` -- MODIFY: add `--swipe-archive`/`--swipe-read`/`--swipe-discard` (+ foreground) tokens mapped to existing theme colors, and a `touch-callout-none` utility (`-webkit-touch-callout:none; -webkit-tap-highlight-color:transparent`).
- `src/components/chat/chat-row.tsx` -- MODIFY: on phone, wire `useLongPress` to the existing ContextMenuTrigger and `useSwipeActions` (trailing → Archive + More(mute ▸ reusing the notify submenu), leading → read/unread); the existing ContextMenu is the non-gesture duplicate. Desktop path untouched.
- `src/components/chat/message-bubble.tsx` -- MODIFY: when `phone`, wrap the bubble in a ContextMenu (items = React row / Reply / Edit-own / Delete-own, reusing the same `onReact`/`onReply`/`onEdit`/`onDelete`) opened via `useLongPress`; desktop keeps the hover action bar only (ContextMenu not mounted off-phone).
- `src/components/layout/pins-strip.tsx` -- MODIFY: phone `useLongPress` opens the existing Unpin menu; add "Move up"/"Move down" items (non-gesture reorder via `reorderPins`); add phone long-press-drag reorder (pointer-based) persisting via `reorderPins`; honor the `reorderable` guard.
- `src/components/layout/favorites-section.tsx`, `src/components/layout/networks-group.tsx` -- MODIFY: attach `useLongPress` + callout suppression to their existing ContextMenu triggers (phone).
- `src/components/approval/approval-pane.tsx` -- MODIFY: on phone, row-tap → `onEnterEdit`; add an explicit ≥44pt per-row Approve button; trailing swipe (`useSwipeActions`) → `onDiscard` (reusing the existing 5s sonner undo toast). Desktop keyboard path (Enter/⌘Enter/⌘⌫) unchanged; still no approve-all.
- `src/components/layout/phone-shell.tsx` -- MODIFY: extend the pull handler with a second `PULL_REFRESH_THRESHOLD_PX` (> reveal); past it the release triggers refresh instead of Search; render the refresh spinner; call `syncNow()`; on `useShellOffline()` resolve to the offline pill.
- `src/lib/ipc/client.ts` -- MODIFY: add `syncNow(): Promise<void>` → `invoke("sync_now")`.
- `src/components/command-palette/actions.ts` -- MODIFY: add `"sync-now"` handler → `syncNow()`.
- `src-tauri/crates/keeper-core/src/account.rs` -- MODIFY: add a manager method (e.g. `sync_now`) iterating live account handles and awaiting `handle.sync.start()` (resume) on each; no new `Client`/`SyncService`; best-effort per account.
- `src-tauri/crates/keeper-core/src/palette.rs` -- MODIFY: register a global `"sync-now"` `PaletteActionVm` (keywords "sync"/"refresh"); update the registry / menu-mapping / cheat-sheet parity tests so all stay green.
- `src-tauri/crates/keeper/src/ipc.rs` (+ command registration in the Tauri `generate_handler!`) -- MODIFY: add `#[tauri::command] sync_now` delegating to the core method, errors via `to_ipc_error`.

## Tasks & Acceptance

**Execution:**
- [x] `src/hooks/use-long-press.ts` (+ `.test.ts`) -- NEW long-press→`contextmenu` bridge (phone-gated, 500ms, movement cancels) -- opens existing Radix menus by touch.
- [x] `src/hooks/use-swipe-actions.ts` (+ `.test.ts`) -- NEW pointer-based horizontal swipe hook (half-width commit, flick, vertical bailout) reusing the edge-swipe constants -- the shared row-swipe engine.
- [x] `src/index.css` -- MODIFY to add `swipe-action` tokens + `touch-callout-none` utility -- one source for swipe styling + callout suppression.
- [x] `src/components/chat/chat-row.tsx` (+ test) -- MODIFY: phone long-press + trailing(Archive/More·mute) / leading(read-unread) swipes; ContextMenu unchanged as the non-gesture duplicate -- inbox row touch idioms.
- [x] `src/components/chat/message-bubble.tsx` (+ test) -- MODIFY: phone-gated ContextMenu (React/Reply/Edit-own/Delete-own via the existing handlers) opened by long-press; desktop hover bar unchanged -- bubble touch menu.
- [x] `src/components/layout/pins-strip.tsx` (+ test) -- MODIFY: phone long-press menu with Unpin + Move up/down; long-press-drag reorder via `reorderPins`; `reorderable` guard honored -- Pins touch reorder with a non-gesture fallback.
- [x] `src/components/layout/favorites-section.tsx`, `networks-group.tsx` (+ existing tests green) -- MODIFY: phone `useLongPress` + callout suppression on the existing triggers -- long-press parity for the remaining menus.
- [x] `src/components/approval/approval-pane.tsx` (+ test) -- MODIFY: phone row-tap→editor, ≥44pt Approve button, trailing-swipe→Discard reusing the 5s undo toast; no approve-all -- Approval touch idioms.
- [x] `src/components/layout/phone-shell.tsx` (+ test) -- MODIFY: second refresh threshold on the pull axis, spinner, `syncNow()`, offline-pill resolution; Search-reveal band preserved -- pull-to-refresh.
- [x] `src/lib/ipc/client.ts` + `src/components/command-palette/actions.ts` -- MODIFY: `syncNow()` IPC + `"sync-now"` palette handler -- the non-gesture "Sync now".
- [x] `src-tauri/crates/keeper-core/src/account.rs` + `keeper/src/ipc.rs` (+ command registration) -- MODIFY: `sync_now` core method (resume each active `SyncService`) + `#[tauri::command] sync_now` -- the honest sync-loop kick.
- [x] `src-tauri/crates/keeper-core/src/palette.rs` -- MODIFY: register `"sync-now"`; keep registry/menu/cheat-sheet parity tests green -- the search-parity + menu-bar leg.

**Acceptance Criteria:**
- Given a phone viewport, when the user long-presses a row, pin, favorite, network, or message bubble, then the identical ContextMenu (for bubbles: React-row/Reply/Edit-own/Delete-own) opens at the press point with the native callout/selection suppressed, and a short tap or a moving press still scrolls/selects normally.
- Given a phone inbox row, when the user swipes trailing, then Archive + More(mute ▸) is revealed with the label past the half-swipe threshold and a full-swipe commits Archive; when the user swipes leading, then read/unread toggles — and each of these actions is also present in the row's long-press ContextMenu.
- Given the phone Pins strip, when the user long-press-drags a pin, then the order updates and persists via `reorderPins`, and the same reorder is reachable without a gesture via "Move up"/"Move down" in the pin's long-press menu.
- Given the phone Approval Pane, when the user taps a row it opens the inline editor, a ≥44pt per-row Approve button approves through the single gate, and a trailing swipe discards behind the existing 5s undo toast — with no approve-all/bulk affordance anywhere.
- Given pull-to-refresh on the phone Inbox pulled past the refresh threshold, then it kicks each active account's SyncService (the same resume operation as foreground resume) with a visible spinner; when every account is offline the spinner resolves into the persistent offline pill and never an error toast; a pull only into the search-reveal band still opens Search.
- Given a desktop/tablet viewport (≥768px), then all touched components behave byte-for-byte as before (the touch layers are phone-gated and additive), except the additive global `sync-now` palette/menu/cheat-sheet action.
- Given `bun run check`, `bun run check:rust`, and `bun run test:rust`, then Biome + `tsc --noEmit` + vitest and rustfmt + clippy + nextest all pass, including the new hooks, the extended component suites, and the updated Rust palette parity tests.

## Design Notes

**Two-threshold pull axis (reconciling 13.4 + 13.6).** Story 13.4 shipped "release past 64px → open Search". 13.6 keeps that and adds a second, larger `PULL_REFRESH_THRESHOLD_PX`: releasing in `[reveal, refresh)` opens Search (unchanged); releasing at `≥ refresh` triggers refresh instead — "one continuous gesture axis". The pull indicator's affordance switches (e.g. "Release to search" → refresh spinner) as the finger crosses the refresh threshold. This is the only reading consistent with both the already-shipped 13.4 gesture and 13.6's AC.

**Message bubbles have no ContextMenu today.** The bubble uses a hover/focus action bar (`message-actions.tsx`: React / Reply / Edit-own / Delete-own) — there is no Radix ContextMenu and no desktop right-click menu, and no Copy / Jump-to-original menu item / Delete submenu exist. The faithful touch-idiom deliverable is to surface **exactly those existing actions** on long-press via a phone-gated ContextMenu that reuses the same handlers, at ≥44pt. Inventing Copy / a Jump menu entry / a Delete submenu would be new message-action features, out of scope for a touch-parity story (Never boundary). Desktop stays on the hover bar (the ContextMenu is not mounted off-phone).

**`sync_now` is the honest, minimal sync-kick and the Epic 14-1 seam.** Sync is `SyncService`-driven and Rust-authoritative; there is no manual sync primitive today. `SyncService::start()` resumes a stopped/errored service and is a near-no-op when already running — exactly "the same operation as foreground resume". The core method resumes each already-active account handle only (never a second Client/SyncService, never force-activating signed-out accounts). Epic 14-1 ("lifecycle pause/resume through one Rust entry") should route foreground-resume through this same entry rather than adding a competing one. The Block If guards the case where the SDK version offers no safe idempotent resume.

**VoiceOver custom actions (web caveat).** True iOS VoiceOver custom actions are not a web/ARIA API. The epic's "duplicated as VoiceOver custom actions" is satisfied on the web platform by the same actions being present as AT-focusable controls off the gesture — the long-press ContextMenu items (inbox/pins/bubbles) and the explicit Approve/Discard buttons (Approval). On-device VoiceOver confirmation folds into Epic 14/15 hardening + the SM-8 dogfooding gate, per the epic, not this story's acceptance.

**Reuse, not fork.** Every delta is a phone-gated hook, `cn()` branch, or conditional wrapper over the shared component; swipe/long-press hooks reuse the 13.2 pointer idiom and constants. Gesture animations degrade to cuts under `useReducedMotion()`. jsdom can't evaluate real touch/callout CSS, so tests assert handler wiring, class/style presence, and dispatched events rather than rendered pixels.

## Verification

**Commands:**
- `bun run check` -- expected: Biome + `tsc --noEmit` + vitest green, including `use-long-press`, `use-swipe-actions`, and the extended `chat-row` / `message-bubble` / `pins-strip` / `approval-pane` / `phone-shell` suites; desktop suites unchanged and green.
- `bun run test -- use-long-press use-swipe-actions chat-row message-bubble pins-strip approval-pane phone-shell` -- expected: the touched suites pass in isolation.
- `bun run check:rust` && `bun run test:rust` -- expected: rustfmt + clippy (`-D warnings`) clean and nextest green, including the updated `palette` registry/menu/cheat-sheet parity tests and the new `sync_now` path.

**Manual checks (no device required for acceptance):**
- In a sub-768px webview, confirm long-press opens each menu (native callout suppressed), trailing/leading row swipes reveal-and-commit with the label past half-swipe, Pins long-press-drag reorders (and Move up/down works), Approval tap-to-edit + Approve + swipe-to-Discard-with-undo work, and pull-to-refresh spins then (offline) resolves into the offline pill with no error toast. On-device WKWebView feel (long-press timing, swipe momentum, VoiceOver custom actions) folds into Epic 14/15.

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 2, low 1)
- defer: 0
- reject: 16
- addressed_findings:
  - `[medium]` `[patch]` Pull-zone pointer stranding: a pull started while the Inbox was scrolled away stored `pullPointerRef` **without** taking pointer capture (correct — native scroll must own it), but if the finger then lifted off the thin pull band no `pointerup` reached the zone, so `pullPointerRef` never cleared and the `!== null` guard killed **every** later pull (Search *and* refresh) until remount. Fixed: `onPullPointerDown` now returns early when not at the top and tracks nothing (armed pulls capture the pointer, guaranteeing a terminal event). Regression test added (`phone-shell.test.tsx`).
  - `[medium]` `[patch]` Snap-back swipe click-through: a horizontal drag that snapped back (or committed) still let the browser-synthesized `click` reach the row's `onClick`, spuriously opening the conversation (`chat-row`) or the inline editor (`approval-pane`). `longPress.onClickCapture` only suppressed a *long-press* click, not a swipe one. Fixed: `use-swipe-actions` now exposes an `onClickCapture` that swallows the click after a drag that returned to origin or committed (a settled reveal deliberately does **not** suppress, so the close-tap / revealed action buttons still work); merged into `chat-row`'s capture handler and carried by the spread in `approval-pane`. Three hook regression tests + one `chat-row` integration test added.
  - `[low]` `[patch]` Repeated pulls past the refresh threshold re-fired `syncNow()` and re-armed the spinner timeout with no in-flight guard. Fixed: `onPullPointerUp` now guards the refresh branch with `!refreshing`.

Rejected (16): `firedRef` "stuck-true" swallowing a later tap (every fresh `pointerdown` resets it before a touch click, so it needs a click with no preceding pointerdown — not reachable by touch); spinner clears on "any store write, not the exact sync answer" (any streamed status batch *is* a connection tick, and the 8s ceiling bounds the worst case — acceptable, matches the spec); swipe `width` 0 → instant commit (guarded by `rectWidth > 0 ? rectWidth : window.innerWidth`; the double-zero case is jsdom-only, `innerWidth > 0` in any real webview); flick commits below half-width without the "commit" label / light-flick-from-settled-reveal (flick is the established 13.2 idiom and needs ≥40px fast travel; the destructive discard carries a 5s undo — by-design); `chat-row` merged-handler lost-capture firing a late `contextmenu` (a >10px move cancels the long-press before the swipe ever captures, so the timer is already gone); "byte-for-byte asserted only by absence" (the desktop suites are green and cover desktop behavior; DOM-equality snapshots are not this repo's idiom); flick constants triplicated (identical values, zero user impact; 13.2 already kept its own copy); `sync_now` idempotence untested on a *live* running service (unreachable without a homeserver; the SDK's `start()` idempotence was verified in the vendored crate source and the on-device confirmation is an already-documented residual risk folding into Epic 14/15); Pins preview-vs-authoritative order divergence (preview and persist are both `movePin(pins, liftedIndex, sameTarget)` — consistent by construction — and the drop re-validates indices against `pins.length`); Pins length-change mid-drag (drop re-validates; a mid-drag stream Reset is only a transient preview); Pins second long-press mid-preview (the long-press hook cancels on a second pointer, so a preview-remapped index can't be captured); Approve press-and-slip starting a swipe (needs a >12px slip off the button; non-destructive — the row just snaps back and Approve is retried); swipe pending-phase stale `pointerRef` (touch pointercancel reliably fires before capture; symmetric to the pull fix but not independently reachable); pull second-pointer indicator persistence (the armed pointer holds capture, guaranteeing a terminal event); `sync_now` sign-out mid-iteration (the cloned `Arc<SyncService>` keeps the service alive and `start()` on a torn-down service is an SDK no-op); message-bubble React-row item closing on select (one reaction per long-press vs the desktop popover's multi — within a touch-idiom reading, reuses the identical handler).

No defers: the two "for later" candidates (a desktop DOM-equality guard test; de-duplicating the flick constants) are hygiene with no user-facing consequence, and the on-device sync-kick confirmation is already tracked by the epic's Epic-14/15 fold and this spec's residual risks.

## Auto Run Result

Status: done

### Summary
Delivered the phone tier's complete touch-idiom layer (FR-60, FR-41; UX-DR26/DR28) as phone-gated, reuse-only additions with desktop/tablet left byte-for-byte unchanged. A shared `useLongPress` bridge dispatches a synthetic `contextmenu` at the press point after a 500ms stationary hold, opening the *identical* Radix ContextMenus by touch on inbox rows, message bubbles (a phone-only ContextMenu reusing the hover bar's React/Reply/Edit-own/Delete-own handlers), pins, favorites, and network rows — with `-webkit-touch-callout`/tap-highlight suppressed. A `useSwipeActions` pointer engine (mirroring the 13.2 edge-swipe idiom: 12px intent slop, half-width commit, flick, vertical bailout, sticky reveal) drives inbox trailing→Archive+More(mute ▸) / leading→read-unread swipes and Approval trailing→Discard. Pull-to-refresh extends the 13.4 pull axis with a second `PULL_REFRESH_THRESHOLD_PX` (128px): a release in `[64,128)` opens Search unchanged, ≥128px kicks the sync loop via a new Rust `sync_now` command that resumes each already-active account's `SyncService` (the honest "foreground resume", and the single seam Epic 14-1 will route through) — offline resolves into the persistent offline pill, never an error toast. Approval rows gain phone tap-to-edit, an explicit ≥44pt per-row Approve through the single gate, and swipe-to-Discard behind the existing 5s undo toast (still no approve-all). Pins gain long-press-drag reorder (persisted via `reorderPins`) plus non-gesture Move up/down menu items. The non-gesture "Sync now" is a new global `sync-now` palette action (Rust catalog + TS handler), reachable from the phone Search Actions scope.

### Files changed
- `src/hooks/use-long-press.ts` (+ test) — NEW 500ms/10px long-press→`contextmenu` bridge; phone-gated, mouse-ignored, post-fire click suppressed, optional `onLongPress` for the Pins lift.
- `src/hooks/use-swipe-actions.ts` (+ test) — NEW pointer swipe engine (half-width commit, flick, vertical bailout, sticky reveal); **review-patched** to suppress the post-drag synthetic click.
- `src/index.css` — `--swipe-archive/read/discard` (+ foregrounds) tokens and a `touch-callout-none` utility.
- `src/components/chat/chat-row.tsx` (+ test) — phone long-press + trailing/leading swipes over the existing ContextMenu; **review-patched** to merge swipe click-suppression.
- `src/components/chat/message-bubble.tsx` (+ `reaction-popover.tsx` export, + test) — phone-gated ContextMenu (React/Reply/Edit-own/Delete-own) via long-press; desktop hover bar untouched.
- `src/components/layout/pins-strip.tsx` (+ test) — long-press-drag reorder + Move up/down menu items; desktop HTML5 drag untouched.
- `src/components/layout/favorites-section.tsx`, `networks-group.tsx` — long-press + callout suppression on the existing triggers.
- `src/components/approval/approval-pane.tsx` (+ test) — phone tap-to-edit, ≥44pt Approve, swipe-to-Discard with the existing 5s undo toast.
- `src/components/layout/phone-shell.tsx` (+ `sidebar-pane.tsx` export, + test) — two-threshold pull axis, refresh spinner, `syncNow()`, offline-pill resolution; **review-patched** for pull-zone non-stranding and a refresh re-entrancy guard.
- `src/lib/ipc/client.ts` (`syncNow()`), `src/components/command-palette/actions.ts` (`sync-now` handler).
- `src-tauri/crates/keeper-core/src/account.rs` (`AccountManager::sync_now`, clone-Arc-under-lock then `start()` outside it, live handles only) + `keeper/src/ipc.rs` (`#[tauri::command] sync_now`) + `keeper/src/lib.rs` (registration) + `keeper-core/src/palette.rs` (global `sync-now` action; registry/menu/cheat-sheet parity tests updated).

### Review findings breakdown
- Patches applied: 3 — pull-zone pointer stranding (medium), snap-back swipe click-through (medium), duplicate-`syncNow` re-entrancy guard (low). See the Review Triage Log. Each is covered by a new regression test.
- Deferred: 0.
- Rejected: 16 — not-reachable-by-touch, by-spec/by-design, jsdom-only, consistent-by-construction, or already-documented residual risk. See the Review Triage Log.
- intent_gap: 0, bad_spec: 0.

### Follow-up review
`followup_review_recommended: false` — the three patches are localized frontend guard additions (a pointer-tracking non-strand, a capture-phase click suppressor, and a one-line re-entrancy guard), each with a new regression test, and none touch the API surface, data model, security, or Rust — desktop stays byte-for-byte.

### Verification
- `bun run check` — green: Biome clean (298 files), `tsc --noEmit` clean, vitest **109 files / 1145 tests passed** (+5 over the post-implementation 1140: three swipe click-suppression tests, one `chat-row` snap-back integration test, one `phone-shell` non-strand test), core-tauri-free convention check passed. Run independently after implementation and again after the review patches.
- `bun run check:rust` — green: rustfmt clean, clippy `-D warnings` clean.
- `bun run test:rust` — green: **767/767** nextest, including `sync_now_with_no_live_accounts_is_a_noop` and the updated palette registry/parity tests. (Rust was untouched by the review patches, which were TS-only.)

### Residual risks
- On-device WKWebView feel — long-press timing, swipe momentum/labels, `-webkit-touch-callout` suppression, and the live `sync_now` resume without stream teardown/duplication — is only fully verifiable in the iOS Simulator / on a device; per the epic that confirmation folds into Epic 14/15 hardening + the SM-8 dogfooding gate, not this story's acceptance.
- VoiceOver "custom actions" are satisfied on the web platform by AT-focusable non-gesture equivalents (long-press menus + explicit buttons); true native VoiceOver custom-action parity is an on-device Epic 14/15 check.
- The `sync-now` palette action is an additive global (menu bar + cheat sheet + palette) — the one sanctioned desktop-visible delta; sync is universal so it is not capability-gated.
