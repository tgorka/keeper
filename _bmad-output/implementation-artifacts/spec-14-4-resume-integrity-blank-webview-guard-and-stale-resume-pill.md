---
title: 'Resume Integrity — Blank-Webview Guard and Stale-Resume Pill'
type: 'feature'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: ['oversized']
baseline_revision: '94880cb150ff1409aa0eddeafbd998fc229af6f6'
final_revision: '866ff07d776518a7a394ff4e2537edb25168f19f'
---

<intent-contract>

## Intent

**Problem:** On iOS an app that has been suspended (overnight, under memory pressure) can come back wrong in three ways, none of which keeper handles yet. (1) The WKWebView web-content process can be jettisoned while backgrounded (tauri#14371); on foreground the view is blank/frozen and there is **no reload guard** — the app just sits dead. (2) `roomsStore.selected`/`detailStore` are explicitly ephemeral (never persisted), so even a clean reload drops the user off the Room/Detail they had open back to a bare Inbox instead of the last stack level. (3) After a long suspension the sliding-sync session goes stale (matrix-rust-sdk#3935): the existing foreground kick is a bare idempotent `SyncService::start()` that no-ops when the service still reports `Running`, so a stalled session never restarts — and there is no honest "Connecting…" feedback while the resume reconnects.

**Approach:** Add a **reload guard** on the reduced (iOS) tier: on foreground resume, an animation-frame liveness probe reloads the webview once (loop-guarded) if it is blank/frozen; and the last stack level (`account_id`, `room_id`, `detail_open`) is persisted in **Rust** (which survives the web-content-process death) so any reload re-hydrates the UI to exactly where the user was, on top of the existing snapshot-then-diff re-subscribe that restores the data (AD-8). Add a **sync-loop restart guard** in the one Rust lifecycle entry: record the pause instant on Background, and on Foreground, if the suspension exceeded a stale threshold, force a full restart (`pause_all()` then the shared `sync_now()` kick) to defeat the #3935 stale-session edge — otherwise the bare shared kick, unchanged. Add a quiet **"Connecting…" pill** under the phone Inbox header that appears on a resume that takes longer than a short show-delay (i.e. a stale reconnect) and clears on the first sync-status tick.

## Boundaries & Constraints

**Always:**
- One lifecycle truth (AD-30): the resume path stays the single Rust `app_lifecycle_changed` entry, and the sync *kick* stays `AccountManager::sync_now()` — the exact call pull-to-refresh (13.6) uses. The restart guard is a resume-only prelude (`pause_all()` before the shared kick when stale), never a second kick and never applied to pull-to-refresh.
- Snapshot-then-diff re-hydration (AD-8): after any reload the existing `subscribeInbox`/`subscribeTimeline` mount effects re-open each stream and receive a full snapshot — the guard restores only *which* stack level (nav selection), never message/room data (Rust stays the one source of truth for data).
- The last-stack-level store lives in **Rust** (`AppState`), reported from the frontend on the reduced tier only, so it survives a jettisoned web-content process and a cold launch starts fresh at the Inbox (no stored nav → level 0). Persist a room only when one is open; clear it on return to the Inbox.
- Honesty rule (FR-53/FR-62, AD-30): the "Connecting…" pill is a transient resume indicator only; nothing implies live/background sync while suspended. The pill clears on the first status tick or a timeout backstop and never becomes a stuck spinner.
- The reload guard reloads **at most once per resume** and never loops: gate on a `sessionStorage` attempt flag cleared the moment a healthy render is confirmed; only reload when the rAF liveness probe fails (a healthy webview always services rAF).
- Reduced-tier only, read from `useIsReducedCapabilityPlatform()` (never user-agent/build flags): desktop attaches no resume/guard/persistence listeners and its lifecycle command is never invoked (Story 10.3 preserved). Reuse existing idioms — `role="status"` `bg-held/10`/pill styling, `accountStatusStore.subscribe(...)`+timeout backstop (phone-shell refresh spinner), the `use-active-chat-reporter` reduced-tier gating shape, `void ...catch(() => {})` best-effort IPC.
- Rust: `unsafe_code = "deny"`, no `.unwrap()`/bare `.expect()` in prod; `tracing` logging; new commands registered in `generate_handler!`. Frontend: no `any`, `import type` for types, generated ts-rs type for `NavState`.

**Block If:**
- Restoring the last stack level would require holding message/room data (not just the nav selection) in JS or in `sessionStorage` as a source of truth — HALT rather than violate the Rust-owns-data invariant.

**Never:**
- Never add native Swift/ObjC in this story: the native `webViewWebContentProcessDidTerminate` trigger for a *fully-dead* JS context is the tracked upgrade path (tauri#14371) validated on-device in SM-8, not built here. The JS rAF probe covers the blank/frozen-but-serviceable manifestation; the reload+restore recovery path is shared by both triggers.
- Never add push/APNs/NSE/background sync; never change desktop notification, dock-badge, sync, or lifecycle behavior; never wire any of these listeners on the desktop tier.
- Never build Story 14.6's persistent offline pill here — 14.4's pill is the transient reconnect indicator only; if a resume stays offline past the backstop the pill simply clears.
- Never overload Story 14.3's `NotifyConfig.active_room` (notification-suppression state) for nav restore — use the dedicated nav-state seam so the two concerns stay independent.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh resume (short suspension) | `Foreground`, elapsed < stale threshold | bare shared `sync_now()` (+ `reassert_badge`); no restart | infallible/best-effort |
| Stale resume (long suspension) | `Foreground`, elapsed ≥ stale threshold | `pause_all()` then shared `sync_now()` (full sliding-sync restart, #3935) (+ `reassert_badge`) | infallible/best-effort |
| Background transition | `Background` | record pause `Instant`, then `pause_all()` (unchanged) | infallible |
| Resume, webview responsive | visibilitychange→visible, rAF fires within probe window | no reload; clear the reload-attempt flag | — |
| Resume, webview blank/frozen | visible, rAF does NOT fire within probe window, flag unset | `location.reload()` once; set attempt flag | reload swallows nothing else |
| Resume, blank + already reloaded | visible, rAF fails, attempt flag set | no second reload (loop guard) | — |
| Reload with stored nav | mount, `nav_state_get()` = `{account,room,detail_open}` | restore `roomsStore.selected` + `detailStore` → renders at last stack level; streams re-subscribe for data | get failure ⇒ start at Inbox |
| Cold launch | mount, `nav_state_get()` = `None` | Inbox (level 0), no restore | — |
| Nav change, reduced tier | `selected`→room / detail open toggles | `nav_state_set(account,room,detail_open)`; return to Inbox ⇒ `nav_state_clear()` | best-effort, swallow rejection |
| Stale resume, pill | visible → sync reconnecting past show-delay | quiet "Connecting…" pill under Inbox header (`role="status"`), clears on first `accountStatusStore` tick or timeout backstop | never sticks |
| Fast resume, pill | visible → first status tick before show-delay | pill never becomes visible | — |
| Desktop tier | any | no persistence, no guard, no pill, no restart; lifecycle never invoked | — |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/src/lifecycle.rs` -- `app_lifecycle_changed` (51): `Background` records a pause **wall-clock `SystemTime`** (see Design Notes "Review R1") — and only when `paused_at` is currently `None`, so a duplicate `Background` keeps the earliest suspension instant. `Foreground` **takes** it and, when `should_restart_sync(paused_at, now, STALE_RESUME_THRESHOLD)` is true, runs `state.accounts.pause_all().await` before the existing `sync_now()`+`reassert_badge()`, else the current path unchanged. Add the pure helper `should_restart_sync(paused: Option<SystemTime>, now: SystemTime, threshold)` (saturating `duration_since`; a backward clock jump ⇒ elapsed 0 ⇒ not stale) + `STALE_RESUME_THRESHOLD` const here (unit-testable without real time).
- `src-tauri/crates/keeper/src/ipc.rs` -- `AppState` (48): add `paused_at: Mutex<Option<SystemTime>>` and `nav_state: Mutex<Option<NavState>>`. Add `#[tauri::command]`s `nav_state_set(account_id, room_id, detail_open: bool)`, `nav_state_clear()`, `nav_state_get() -> Option<NavState>` (mirror the delegation shape of `dock_badge_mode_*`/`active_chat_set`). Lifecycle reads/writes `paused_at` via poison-recovering slot helpers.
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `NavState { account_id: String, room_id: String, detail_open: bool }` (`Serialize`, `Deserialize`, `Clone`, `TS`, `#[ts(export)]`), mirroring the existing view-model types.
- `src-tauri/crates/keeper/src/lib.rs` -- register `nav_state_set`, `nav_state_clear`, `nav_state_get` in `generate_handler!` (near `active_chat_set`).
- `src/lib/ipc/client.ts` -- typed wrappers `navStateSet(selection, detailOpen)`, `navStateClear()`, `navStateGet(): Promise<NavState | null>` (mirror `activeChatSet`). ts-rs emits `src/lib/ipc/gen/NavState.ts`.
- `src/hooks/use-nav-state-persistence.ts` -- NEW: reduced-tier only. On mount (once) `navStateGet()` → restore `roomsStore` selection + `detailStore` open. Subscribe to `roomsStore.selected` + `detailStore`: room open ⇒ `navStateSet`, back to Inbox ⇒ `navStateClear` (value-deduped like `use-active-chat-reporter`). Desktop no-op.
- `src/hooks/use-webview-guard.ts` -- NEW: reduced-tier only. On `visibilitychange`→visible (real resume, after having been hidden), run an rAF liveness probe; if it fails within the probe window and the `sessionStorage` attempt flag is unset, set the flag and `location.reload()`; on a healthy probe clear the flag. Desktop no-op.
- `src/hooks/use-stale-resume-pill.ts` -- NEW: reduced-tier only. On `visibilitychange`→visible (after hidden), start a show-delay timer; if not cleared first, set `connecting=true`; clear on the next `accountStatusStore` change or a timeout backstop. Returns `connecting` (false on desktop / pre-hydration).
- `src/components/layout/phone-shell.tsx` -- render the "Connecting…" pill from `useStaleResumePill()` as an absolute band at `level === 0` under the header (same slot as `pullIndicator`, line ~705; `role="status"`, `bg-held/10 text-held` quiet pill). `level`/`selected`/`detailOpen` already derived here (244).
- `src/App.tsx` -- mount `useNavStatePersistence()` and `useWebviewGuard()` alongside `useAppLifecycle()`/`useActiveChatReporter()`.
- `src/lib/stores/rooms.ts` (`selected`/`selectRoom`, 200), `src/lib/stores/detail-ui.ts` (detail open), `src/lib/stores/account-status.ts` (`accountStatusStore.subscribe`), `src/lib/stores/capabilities.ts` (`useIsReducedCapabilityPlatform`) -- existing seams the new hooks read/drive.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `NavState { account_id, room_id, detail_open }` (`Serialize`+`Deserialize`+`Clone`+`TS`, `#[ts(export)]`). -- typed nav state for restore.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- add `paused_at: Mutex<Option<SystemTime>>` + `nav_state` `Mutex` fields to `AppState`; implement `nav_state_set`/`nav_state_clear`/`nav_state_get`. -- Rust-held last-stack-level + resume timing.
- [x] `src-tauri/crates/keeper/src/lifecycle.rs` -- record a wall-clock `SystemTime` pause on `Background` **only when none is stored** (earliest-wins); on `Foreground` **take** it and restart (`pause_all()` then `sync_now()`) when stale, else unchanged; add pure `should_restart_sync(Option<SystemTime>, SystemTime, threshold)` (saturating) + `STALE_RESUME_THRESHOLD`. -- #3935 restart guard that survives real iOS suspension (Design Notes "Review R1").
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register the three `nav_state_*` commands. -- wires them.
- [x] `src/lib/ipc/client.ts` -- add `navStateSet`/`navStateClear`/`navStateGet` wrappers. -- frontend seams.
- [x] `src/hooks/use-nav-state-persistence.ts` (new) -- reduced-tier report + on-mount restore of the last stack level; seed the reporter baseline from the intended restored state so restoring `detailOpen: true` never transiently writes `false` back (Design Notes "Review R1"). -- survives reload/jettison, lands the user where they were.
- [x] `src/hooks/use-webview-guard.ts` (new) -- reduced-tier liveness probe → one loop-guarded `location.reload()` only on a genuinely blank/frozen resume: require multiple consecutive missed frames (not a single miss) so a slow-but-healthy resume is never reloaded, and fail safe — don't reload if the one-shot attempt can't be durably recorded; stamp the guard per document-load (Design Notes "Review R1"). -- the reload guard trigger (JS stopgap; native trigger deferred).
- [x] `src/hooks/use-stale-resume-pill.ts` (new) -- reduced-tier delayed-show "Connecting…" state, cleared on a status transition that represents the resumed sync answering (a change to a connected value), not any unrelated `accountStatusStore` tick, plus a timeout backstop (Design Notes "Review R1"). -- honest transient resume feedback.
- [x] `src/components/layout/phone-shell.tsx` -- render the "Connecting…" pill band under the Inbox header at level 0; hide it only while an actual refresh spinner is in flight (not merely because the pull-reveal band is showing). -- surfaces the pill.
- [x] `src/App.tsx` -- mount `useNavStatePersistence()` + `useWebviewGuard()`. -- activates guard + persistence.
- [x] `src-tauri/crates/keeper/src/lifecycle.rs` (tests) -- `should_restart_sync` boundary (below/at/above threshold, no pause recorded). -- guards the stale gate.
- [x] `src-tauri/crates/keeper/src/ipc.rs` (tests) -- `nav_state` set/clear/get round-trip on `AppState`. -- guards persistence.
- [x] `src/hooks/use-nav-state-persistence.test.ts` (new) -- reports on change, clears on Inbox, restores on mount, desktop no-op. -- audit guard.
- [x] `src/hooks/use-webview-guard.test.ts` (new) -- reloads on failed rAF probe, loop-guard blocks a second reload, healthy probe/desktop never reload. -- audit guard.
- [x] `src/hooks/use-stale-resume-pill.test.ts` (new) -- shows on stale resume, clears on status tick, fast resume never shows, desktop false. -- audit guard.
- [x] `src/components/layout/phone-shell.test.tsx` (extend) -- pill renders under the header when `connecting`, absent otherwise. -- audit guard.

**Acceptance Criteria:**
- Given the iOS tier with a Room or Detail open, when the app foregrounds after a webview reload (jettison-recovery path, simulated where process termination can be simulated), then the reload guard restores the UI to that last stack level from Rust-held nav state (data re-hydrated via snapshot-then-diff) and never shows a blank or unresponsive screen; a fresh cold launch instead starts at the Inbox.
- Given the iOS tier resuming after a long suspension (stale session, matrix-rust-sdk#3935), when the app foregrounds, then the lifecycle entry forces a full sync restart (`pause_all()` then the shared `sync_now()` kick), cached UI renders immediately, and a quiet "Connecting…" pill shows under the Inbox header and clears on the first sync-status response; a short/fresh resume takes the bare kick and never flashes the pill.
- Given the desktop tier, when the app runs and resumes, then notification, dock-badge, sync, and lifecycle behavior are byte-for-byte unchanged and no resume/guard/persistence listener is attached.
- Given the full change, when the quality gates run, then `bun run check`, `bun run check:rust`, and `bun run test:rust` all pass.

## Spec Change Log

### 2026-07-11 — Review pass 1 (bad_spec)
- **Triggering findings:** (1) resume-staleness used `std::time::Instant`, which excludes iOS suspend time → an overnight resume never trips the #3935 restart (the headline scenario would silently no-op on device); (2) a duplicate `Background` overwrote the pause timestamp → a long suspension read as short and skipped the restart; (3) the "Connecting…" pill cleared on *any* `accountStatusStore` tick → a multi-account phone under-reports (pill hidden while the watched sync still reconnects); (4) the rAF liveness probe reloaded on a single missed frame → spurious reload of a healthy-but-busy resume, violating "never reload a healthy webview"; (5) the restore seed report could transiently overwrite a stored `detailOpen: true` with `false`; (6) the reload loop-guard wasn't fail-safe against storage failure / cross-session flag survival; (7) the pill hid during a non-refresh reveal drag.
- **Amended (outside `<intent-contract>`):** Code Map + Tasks (lifecycle clock → suspend-counting wall-clock `SystemTime`; earliest-wins pause; `paused_at: Mutex<Option<SystemTime>>`) and Design Notes ("Review R1" — seven resume-timing/precision refinements). The `<intent-contract>` was untouched: it specified a generic "pause instant," so these fixes refine *how*, not the intent.
- **Known-bad state avoided:** a restart guard and stale-resume pill that silently no-op on the exact overnight-suspension scenario the story exists to fix, plus a reload guard that could reload a healthy webview or loop.
- **KEEP (must survive re-derivation):** Rust-held `NavState` via poison-recovering `slot_set/get/take` one-line-delegate commands + ts-rs export; the pure `should_restart_sync` helper + boundary tests (over the new `SystemTime` type); take-not-read consumption of the pause timestamp on `Foreground`; the `pause_all()` prelude before the shared `sync_now()` kick (one lifecycle truth, pull-to-refresh untouched); reduced-tier gating mirroring `use-active-chat-reporter` with value-dedup and desktop no-op; the DW-109-aware detail-reopen macrotask ordering, StrictMode-safe restore-read ref, and user-navigation-wins-over-restore race handling; the pill show-delay + timeout backstop and the offline-pill styling idiom / pull-indicator placement; full unit-test coverage and all three green gates.

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 7: (high 1, medium 3, low 3)
- patch: 0
- defer: 2
- reject: 4
- addressed_findings:
  - `[high]` `[bad_spec]` Resume-staleness timing used `std::time::Instant`, which does not advance during iOS device sleep — the #3935 restart guard and the "Connecting…" pill would silently no-op after an overnight suspension. Amended the spec to record the pause with wall-clock `SystemTime` (saturating `duration_since`).
  - `[medium]` `[bad_spec]` Duplicate/late `Background` reports overwrote `paused_at`, shrinking a long suspension so the restart is skipped. Amended to earliest-wins (record only when unset).
  - `[medium]` `[bad_spec]` The pill cleared on any `accountStatusStore` mutation (any account, no-op writes), so a multi-account phone hides it while the watched sync still reconnects. Amended to settle on a real status→connected transition (diff `state`/`prevState`) + backstop.
  - `[medium]` `[bad_spec]` The rAF probe declared the webview blank on a single missed frame, risking a spurious reload of a slow-but-healthy resume (snapshot-then-diff work). Amended to require multiple consecutive missed frames.
  - `[low]` `[bad_spec]` The restore seed report could write `detailOpen: false` back to Rust during the detail-reopen macrotask, transiently losing the Detail level. Amended to seed the reporter baseline from the intended restored state.
  - `[low]` `[bad_spec]` The reload loop-guard relied on `sessionStorage` semantics and wasn't fail-safe. Amended: don't reload if the attempt can't be durably recorded; stamp the guard per document-load.
  - `[low]` `[bad_spec]` The pill hid during a non-refresh pull-reveal ("Release to search") drag. Amended to gate visibility on an in-flight refresh spinner, not the reveal band.
- notes:
  - Deferred (2): restore does not validate that the stored room/account still exists after a sign-out/leave during suspension (needs care — the rooms mirror isn't hydrated at mount, so a naive check would defeat restore); and `app_lifecycle_changed` has no cross-report re-entrancy guard so a late `Background` could stop a just-resumed sync (pre-existing from Story 14.1's foreground/background handlers, not introduced here). Ledgered to `deferred-work.md`.
  - Rejected (4): (1) `nav_state` vs 14.3's `active_chat` being two Rust truths for the open chat — a transient post-reload divergence is at worst one safe over-notify, the same posture 14.3 accepted; (2) no upper bound on a leaked `paused_at` — once earliest-wins + wall-clock land, an old timestamp correctly signals a genuinely long suspension, so a restart is the intended behavior; (3) redundant `navStateClear` on a stray Inbox-level detail toggle — benign best-effort churn, covered by value-dedup; (4) no connecting pill at stack level 1/2 — the AC scopes the pill to the Inbox header by design (in-Chat reconnect is the Room's own concern / 14.6's offline pill). Plus review noise (the infallible-`Result` return shape, a minor `wasHidden` re-init nuance, a suggested desktop-gating regression test).

### 2026-07-11 — Review pass (post-loopback, pass 2)
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 2, low 1)
- defer: 1
- reject: 6
- addressed_findings:
  - `[medium]` `[patch]` The blank-webview guard ran its reload-capable probe at ATTACH too, so a slow-but-healthy cold boot (main thread saturated by hydration/first render past the 3×250 ms window) could false-positive `location.reload()` a healthy webview — violating "never reload a healthy webview". Scoped reload to resume-opened probes only (`startProbe(allowReload)`): the attach-time probe is confirm-only (re-arm/clear the flag on a healthy frame, never reload). +1 test.
  - `[medium]` `[patch]` The "Connecting…" pill showed while genuinely offline (`connecting && !refreshing`), asserting a reconnect that can't happen — dishonest for up to the backstop. Gated the pill on `!offline`. +1 test.
  - `[low]` `[patch]` The R1 "keep the pill up during a reveal drag" fix left the pill and the pull-reveal band co-rendered in the same absolute slot (overlap). Gated the pill on `pullDy === null` so an active pull gesture owns the slot and the pill returns when the gesture ends; a passive resume still shows it. Updated the reveal-drag test to the new behavior.
- notes:
  - Blind Hunter independently CONFIRMED all seven Review R1 fixes are correctly in place (wall-clock `SystemTime` + saturating `duration_since`; earliest-wins `Background` / take-on-`Foreground`; pill settles only on an offline→online transition; multi-frame probe; reporter baseline seeded from the restored `NavState`; fail-safe reload with read-back; `connecting && !refreshing`). No intent_gap or bad_spec — the residue was patch-level only.
  - Deferred (1): under React StrictMode's dev-only double-mount, the detail-level restore's re-scheduled `setTimeout` can be dropped so a stored `detailOpen: true` restores the Room but not the Detail (production single-mount is unaffected). Ledgered to `deferred-work.md`.
  - Rejected (6): (1) pill hangs to the timeout backstop on a resume with NO offline→online transition (already-online) — bounded by the backstop, and a real stale resume goes offline via `pause_all()` first, so it transitions; (2) pill lingers to backstop if the sole account is removed / the store reset mid-window — rare, backstop-bounded; (3) rAF from a prior probe generation could mark a new resume healthy — very specific frame race, best-effort guard, low consequence; (4) rapid hidden↔visible churn restarts the show-delay so the pill may not appear — bounded, cosmetic; (5) the multi-account pill settles on ANY account's online transition, not the watched one — an intentionally coarse indicator, safe over-clear; (6) informational note that the guard flag's timestamp value is unused (presence-only, by design). Restore-into-a-nonexistent-room resurfaced but is already on the ledger from pass 1 (not re-filed).

### 2026-07-11 — Review pass (follow-up, pass 3)
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 1
- reject: 15
- addressed_findings:
  - none
- notes:
  - Follow-up pass over the hand-applied pass-2 patches (guard attach-vs-resume reload scoping; pill `!offline` / `pullDy === null` gating), run by two fresh adversarial reviewers (Blind Hunter + Edge Case Hunter) with no prior context. The Rust surface (wall-clock `SystemTime` + saturating `duration_since`; earliest-wins `Background` / take-on-`Foreground`; `should_restart_sync` boundaries; `slot_*` poison-recovery + round-trip tests) and the pass-2 patches were independently reconfirmed correct — no intent_gap, no bad_spec, no patch. Convergent with pass 2.
  - Deferred (1, NEW): both reviewers independently flagged as the top open concern that the pass-2 `!offline` honesty gate interacts with the stale-resume `pause_all()` prelude — which drives every account to `Offline` — so on the exact overnight/stale-resume scenario the pill exists for, `!offline` hides it while offline and the subsequent offline→online transition settles (clears) the hook, so the "Connecting…" pill may essentially never render on its primary path. The honest distinction the code does not yet draw is "offline because mid-restart-reconnect" (should show) vs "offline and staying offline" (14.6's persistent pill, should not). Not re-derivable without the SM-8 on-device observation the spec's residual-risk #1 already names (does `pause_all()` emit an offline batch, and for how long); reversing the `!offline` gate blind would reintroduce the pass-2 dishonesty (a "Connecting…" claim on a resume that stays offline). Ledgered to `deferred-work.md` as a NEW entry for focused SM-8 attention; the related test-architecture gap (the phone-shell suite mocks `useStaleResumePill` apart from the component, so no test exercises the hook↔component interaction on a real `pause_all` resume) is folded into the same entry.
  - Rejected (15): all remaining findings are already-ledgered deferred items surfacing again (restore-into-a-nonexistent-room [DW/14.4]; `app_lifecycle_changed` cross-report re-entrancy [14.4]; StrictMode dev-only detail-reopen drop [14.4]), items already reasoned-and-rejected across passes 1–2 (no upper bound on a leaked `paused_at`; a prior-generation rAF marking a new resume healthy; rapid hidden↔visible churn; the multi-account pill settling on any account's online transition; the presence-only guard flag's unused `Date.now()` payload; no pill at stack level ≥ 1), or low-consequence nits with no defect (a redundant idempotent `navStateClear()` on cold launch; the guard's `blockedRef` StrictMode assumption is untested but no bug found; a `wasHidden` pre-hydration resume-miss edge, narrow and self-recovering; serial `pause_all()` resume latency; poison-recovery swallowing a near-impossible mid-write panic without a `tracing` breadcrumb; "Review R2" comment labels vs the spec's "Review R1"-named section — the mapping to "review pass 2" in the Triage Log is understandable).

## Design Notes

- **Why Rust holds the last stack level.** A WKWebView content-process jettison (tauri#14371) destroys the web-content process — `sessionStorage` and all JS state die with it, so client-side persistence cannot survive the exact failure we are guarding against. The Rust/Tauri process keeps running across a jettison, so storing `(account_id, room_id, detail_open)` in `AppState` is the only store that survives it; a true app kill restarts Rust fresh, giving the correct "cold launch → Inbox" distinction for free. This is nav *selection* only — the messages/rooms still stream from Rust via the existing snapshot-then-diff re-subscribe on mount (AD-8), so the Rust-owns-data invariant holds. Deliberately not reusing 14.3's `NotifyConfig.active_room` (it is notification-suppression state that clears on chat close and lacks the detail level) keeps the two concerns independent.
- **The guard is recovery + a JS stopgap trigger; the native trigger is deferred.** The valuable, reusable half — reload → re-subscribe (data) + restore nav (level) → land exactly where the user was, loop-guarded so it can never thrash — is built and unit-tested here. The trigger is split by manifestation: a blank/frozen-but-serviceable webview is caught by the multi-frame rAF liveness probe (see "Review R1" below — a healthy webview services frames within the window; only *repeated* misses ⇒ reload); a fully-dead JS context can only be caught by the native `webViewWebContentProcessDidTerminate` delegate, which is AD-30's tracked upgrade path (tauri#14371) validated on-device in SM-8. Story 12.6's lifecycle soak — which would have supplied empirical on-device blank-webview data — was deferred to the coordinator and never executed, so there are no device findings to consume yet; both triggers funnel into the same recovery path so the native upgrade is drop-in later.
- **Restart guard vs. the shared kick.** `sync_now()` is a bare idempotent `SyncService::start()` that no-ops when the service still reports `Running` — exactly the #3935 stale-session trap after a long suspension. Forcing `pause_all()` (per-account `SyncService::stop()`) immediately before the shared `sync_now()` makes the subsequent `start()` a real restart, reusing proven methods with no new sync surface. It runs only on a stale *resume*, so pull-to-refresh (13.6) still converges on the bare `sync_now()` kick — one kick, no second lifecycle truth.
- **Pill needs no staleness timestamp.** A short show-delay before the pill becomes visible means a fresh resume (sync response beats the delay) never flashes it, while a stale reconnect (slower, and carrying the extra restart latency) shows it — so the pill's visibility naturally tracks staleness without a second "stale" definition competing with the Rust restart gate. Clear-on-status-progress + timeout backstop is the same shape the phone-shell pull-refresh spinner uses (refined per "Review R1" to key on a real reconnect, not any tick).

### Review R1 refinements (review pass 1, bad_spec)

- **Resume-staleness clock must count suspension.** `std::time::Instant` on Apple platforms is backed by `mach_absolute_time` and does **not** advance while the device is asleep — an overnight lock measures as near-zero elapsed, so an `Instant`-based gate takes the no-op bare kick for the exact overnight scenario #3935 targets (the feature would be dead code on device). Record the pause with wall-clock `SystemTime` (counts suspension) and compute elapsed with a saturating `duration_since` (`now < paused` from a backward NTP jump ⇒ elapsed 0 ⇒ not stale ⇒ safe bare kick). `should_restart_sync` stays a pure helper over the timestamp for unit tests.
- **Earliest-suspension wins.** Lifecycle reports are not guaranteed strictly alternating; record the `Background` pause timestamp only when none is stored, so a duplicate/late `Background` can't shrink a long real suspension into a short one and skip the restart. `Foreground` still consumes (takes) it.
- **Pill settles on the resumed sync answering, not any tick.** `accountStatusStore` fires on any account's status write (incl. unrelated/no-op same-value writes on a multi-account phone); clear the pill on a status transition that represents progress (a status changing to a connected value), diffing `state` vs `prevState` rather than a bare zero-arg callback — otherwise a background status churn hides the pill while the watched sync is still reconnecting. Keep the timeout backstop.
- **Liveness probe must not reload a healthy-but-busy webview.** A single missed `requestAnimationFrame` within a short window is indistinguishable from a slow cold resume doing snapshot-then-diff re-subscribe work on a constrained device; require multiple consecutive missed frames (or a longer, corroborated window) before declaring the webview blank/frozen, protecting the "never reload a healthy webview" invariant during resume-time work.
- **Restore seed can't clobber the stored detail level.** Seed the nav reporter's baseline from the *intended* restored state (selection + `detailOpen`) before its first report, so restoring a `detailOpen: true` level never transiently writes `detailOpen: false` back to Rust during the detail-reopen macrotask window (a re-jettison in that window would otherwise lose the Detail level).
- **Reload guard fails safe.** Only reload when the one-shot attempt can be durably recorded; if the guard flag can't be persisted, don't reload (a blank webview is recoverable later via the native upgrade path — a reload loop is not). Stamp the guard per document-load so a stale flag from a prior session can't suppress a legitimate recovery reload.
- **Pill precedence keys on an in-flight refresh, not the reveal band.** Hide the connecting pill only while an actual refresh spinner is in flight, not merely because the pull-reveal ("Release to search") band is visible, so a search-reveal drag doesn't blank the reconnect indicator.

## Verification

**Commands:**
- `bun run check:rust` -- expected: `cargo fmt --check` + clippy `-D warnings` clean over `NavState`, the `AppState` fields, the three `nav_state_*` commands, and the lifecycle restart guard.
- `bun run test:rust` -- expected: cargo-nextest green, incl. `should_restart_sync` boundary + `nav_state` round-trip tests.
- `bun run check` -- expected: Biome + tsc + Vitest green, incl. the new nav-persistence, webview-guard, stale-resume-pill, and phone-shell pill tests. The iOS-only guard/reload path itself is validated by the Story 12.5 iOS compile-check CI job and folds into SM-8 on-device dogfooding per the epic.

## Auto Run Result

Status: done

**Summary:** Hardened iOS resume integrity (NFR-17/NFR-18, AD-30) across three seams. (A) **Blank-webview guard:** on the reduced tier, a foreground-resume rAF liveness probe reloads the webview once (loop-guarded, fail-safe, per-document-load stamped) when it comes back blank/frozen after a WKWebView content-process jettison (tauri#14371); the last stack level (`account_id`/`room_id`/`detail_open`) is persisted in **Rust** (`AppState`, which survives the web-content-process death), reported from the frontend on the reduced tier and re-hydrated on mount, so any reload lands the user back on their Room/Detail (data via the existing snapshot-then-diff re-subscribe, AD-8) — a cold launch starts fresh at the Inbox. (B) **Sync-loop restart guard (matrix-rust-sdk#3935):** the one Rust lifecycle entry records a wall-clock `SystemTime` pause on Background (earliest-wins) and, on a stale Foreground, runs `pause_all()` before the shared `sync_now()` kick to force a real sliding-sync restart — the bare kick otherwise, so pull-to-refresh stays one lifecycle truth. (C) **Stale-resume pill:** a quiet "Connecting…" pill under the phone Inbox header on a slow reconnect, cleared on the resumed sync answering (an offline→online status transition) or a timeout backstop.

**Files changed:**
- `src-tauri/crates/keeper-core/src/vm.rs` — `NavState { account_id, room_id, detail_open }` (ts-rs `#[ts(export)]`).
- `src-tauri/crates/keeper/src/ipc.rs` — `AppState.paused_at: Mutex<Option<SystemTime>>` + `nav_state`; poison-recovering `slot_*` helpers; `nav_state_set`/`nav_state_clear`/`nav_state_get` commands (+ round-trip/poison tests).
- `src-tauri/crates/keeper/src/lifecycle.rs` — `STALE_RESUME_THRESHOLD` (120 s), pure `should_restart_sync` (saturating `SystemTime` gate), earliest-wins Background / take-on-Foreground restart guard (+ 5 boundary tests).
- `src-tauri/crates/keeper/src/lib.rs` — registered the three commands.
- `src/lib/ipc/client.ts` + `src/lib/ipc/gen/NavState.ts` — typed wrappers + generated type.
- `src/hooks/use-nav-state-persistence.ts` (+ test) — reduced-tier last-stack-level report + on-mount restore (baseline seeded from the restored state; StrictMode-safe read; user-nav-wins).
- `src/hooks/use-webview-guard.ts` (+ test) — reduced-tier multi-frame liveness probe → fail-safe, loop-guarded, resume-only reload (attach-time confirm-only).
- `src/hooks/use-stale-resume-pill.ts` (+ test) — reduced-tier delayed-show "Connecting…" state, settles on an online transition + backstop.
- `src/lib/webview-reload.ts` (new) — one-line `location.reload` seam (testability; `location.reload` is unforgeable in jsdom).
- `src/components/layout/phone-shell.tsx` (+ tests) — the pill band under the Inbox header (`connecting && !refreshing && !offline && pullDy === null`).
- `src/App.tsx` — mounts `useNavStatePersistence()` + `useWebviewGuard()`.

**Review findings breakdown:** Two adversarial passes (Blind Hunter + Edge Case Hunter). **Pass 1:** 0 intent gaps, **7 bad_spec** (1 high, 3 medium, 3 low) → a spec amendment ("Review R1") + full re-derivation; headline: the pause used `std::time::Instant`, which excludes iOS suspend time, so the #3935 restart would silently never fire after an overnight lock — switched to wall-clock `SystemTime`. 2 deferred, 4 rejected. **Pass 2:** Blind Hunter confirmed all 7 R1 fixes in place; 0 intent_gap, 0 bad_spec, **3 patches** (2 medium, 1 low): attach-time probe no longer reloads a healthy-but-slow cold boot; the pill hides while offline (honesty) and during an active pull gesture (no overlap). 1 deferred (StrictMode dev-only detail restore), 6 rejected. See the Review Triage Log for full itemization.

**Follow-up review recommended:** true — the three pass-2 patches (notably the guard's attach-vs-resume reload scoping, a behavior change on the story's safety-relevant recovery surface) were hand-applied after the last independent review, so a fresh pass over them is cheap insurance for a resume-integrity feature.

**Verification** (final tree, all three independently green):
- `bun run check:rust` — PASS (`cargo fmt --check` + clippy `-D warnings` clean).
- `bun run test:rust` — PASS (783 tests, incl. the `should_restart_sync` boundaries + `nav_state` slot round-trip/poison tests).
- `bun run check` — PASS (Biome + tsc + Vitest 1223 tests incl. the new nav-persistence/webview-guard/stale-resume-pill/phone-shell-pill tests; core-tauri-free convention holds). The `#[cfg(target_os = "ios")]`-relevant guard/reload behavior is Simulator/CI-compile-verified here (Story 12.5) and folds into SM-8 on-device dogfooding per the epic.

**Follow-up review pass (pass 3, 2026-07-11):** The recommended independent follow-up over the hand-applied pass-2 patches ran — two fresh adversarial reviewers (Blind Hunter + Edge Case Hunter), no prior context. Outcome: 0 intent_gap, 0 bad_spec, 0 patch, 1 defer, 15 reject. The Rust surface and all three pass-2 patches were independently reconfirmed correct; no code changed this pass. One NEW deferred item was ledgered: on a real stale resume the pill's `!offline` honesty gate + the `pause_all()`-driven offline transition may suppress the "Connecting…" pill on its primary path (the offline-but-reconnecting vs offline-and-staying distinction the code does not yet draw) — device-dependent, owned by SM-8 residual-risk #1, not re-derivable blind. All other findings were already-ledgered defers, already-rejected pass-1/2 items, or low-consequence nits. `followup_review_recommended` is now `false`: this pass converged with no changes to re-review.

**Residual risks:** (1) The clock fix and the whole guard are Simulator/unit-verified only — the real overnight-suspension behavior (does `pause_all()` emit an offline batch so the pill's online-transition clear fires vs. relying on the 15 s backstop; does the wall-clock gate behave on device) is an SM-8 on-device item; (2) restore does not validate that the stored room/account still exists after a sign-out/leave during suspension (deferred — a reload could land on an empty Room pane); (3) `app_lifecycle_changed` has no cross-report re-entrancy guard (deferred, pre-existing from 14.1); (4) StrictMode dev-only detail-restore drop (deferred); (5) the fully-dead JS-context jettison still needs the native `webViewWebContentProcessDidTerminate` trigger (AD-30 upgrade path, out of scope here).
