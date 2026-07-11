---
title: 'Lifecycle Pause/Resume Through One Rust Entry'
type: 'feature'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: ['oversized']
baseline_revision: '75693600c72a61f48c1820b58271b347253fa4ed'
final_revision: '6e1d3bb859764a4a2bcd1d3408f54bc22fa5b13e'
---

<intent-contract>

## Intent

**Problem:** On iOS keeper has no lifecycle signal, so backgrounding leaves the sliding-sync long-poll to die mid-flight instead of pausing cleanly, and returning has no immediate sync kick — the OS-honest "pause when I leave, resume the instant I return" behavior is missing. Desktop, by contrast, deliberately keeps sync alive while hidden (Story 10.3) and must not regress.

**Approach:** Add one Rust lifecycle entry — a single `app_lifecycle_changed` command in a new shell module `lifecycle.rs` — that on background gracefully pauses every live account's `SyncService` (new `AccountManager::pause_all` → `SyncService::stop()`) and on foreground routes through the existing `AccountManager::sync_now` sync-kick (idempotent `start()`). A capability-gated frontend hook drives that entry from the webview `visibilitychange` event (the zero-native stopgap); a future Swift `UIApplication` plugin will call the same entry. Desktop never attaches the listener, so Story 10.3 is untouched.

## Boundaries & Constraints

**Always:**
- All lifecycle detection enters Rust through the single `app_lifecycle_changed` command in `keeper/src/lifecycle.rs` — no second lifecycle command or path.
- Foreground resume MUST delegate to `AccountManager::sync_now()` (the Story 13.6 sync-kick), so pull-to-refresh and foreground resume share that one core operation.
- Background pause MUST call `SyncService::stop()` per live account (graceful) and never tear down accounts, streams, or producers (that is `shutdown`).
- The `visibilitychange` listener attaches ONLY on the reduced-capability (iOS/phone) tier via `isReducedCapabilityPlatform`; never on desktop.
- Rust: no `.unwrap()`/bare `.expect()` in production paths; `cargo fmt` + clippy `-D warnings` clean. TS: no `any`, `import type` for types, Biome clean.
- Pause & resume are best-effort/infallible: an empty (or all-asleep) account set is a no-op; frontend IPC errors are swallowed (no toast).

**Block If:**
- Satisfying an AC would require shipping a Swift/native `UIApplication` plugin — that native plugin is the recorded upgrade path, out of scope for this zero-native webview stopgap.
- The single-entry invariant cannot hold without introducing a second lifecycle command or resume path.

**Never:**
- Never pause or stop sync on desktop from lifecycle/visibility (would regress Story 10.3 background operation).
- Never add a second sync-kick/resume operation competing with `AccountManager::sync_now()`.
- Never derive platform facts from user-agent or build flags — read only the capabilities mirror.
- Never force-activate signed-out or never-subscribed accounts on resume.
- No Swift/native plugin code in this story.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| App backgrounds (iOS) | `visibilitychange`, `document.visibilityState === "hidden"`, reduced-capability tier | Frontend calls `appLifecycleChanged("background")` → `pause_all()` stops each live account's SyncService within seconds; account state retained | IPC error swallowed; no toast |
| App foregrounds (iOS) | `visibilityState === "visible"`, reduced tier | `appLifecycleChanged("foreground")` → `sync_now()` kicks each live SyncService via idempotent `start()`; cached mirrors already on screen | IPC error swallowed |
| Visibility change (desktop) | any visibility change, non-reduced tier | No listener attached; no lifecycle command invoked; sync stays alive (Story 10.3) | n/a |
| Pause with no live accounts | empty / all-asleep account set | `pause_all()` is a no-op, returns Ok | infallible |
| Pull-to-refresh (13.6) | pull past threshold | `syncNow()` → `sync_now` command → `AccountManager::sync_now()` — same core op as foreground resume | existing swallow |
| Capabilities not yet hydrated | boot, `hydrated === false` | predicate false → no listener until hydration resolves the iOS tier | n/a |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/account.rs` -- `AccountManager`; add `pause_all()` mirroring `sync_now()` (line 687). Existing no-op test to mirror at line 5938.
- `src-tauri/crates/keeper/src/lifecycle.rs` -- NEW: the single lifecycle command `app_lifecycle_changed` + `LifecyclePhase` enum.
- `src-tauri/crates/keeper/src/lib.rs` -- declare `mod lifecycle;`, register the command in `generate_handler!`; leave the window-hide/`RunEvent` handlers (~lines 293–341) untouched.
- `src-tauri/crates/keeper/src/ipc.rs` -- `sync_now` command (line 2771) is the shared kick; reference only, unchanged.
- `src/lib/ipc/client.ts` -- add `appLifecycleChanged(phase)` wrapper; `syncNow()` (line 1442) reference.
- `src/lib/ipc/gen/LifecyclePhase.ts` -- NEW ts-rs-generated type (`"foreground" | "background"`).
- `src/hooks/use-app-lifecycle.ts` -- NEW: capability-gated `visibilitychange` → lifecycle hook.
- `src/App.tsx` -- mount `useAppLifecycle()` among the boot hooks (lines 15–25).
- `src/lib/stores/capabilities.ts` -- `isReducedCapabilityPlatform` / `useIsReducedCapabilityPlatform` gate (lines 76–89).
- `src/components/layout/phone-shell.tsx` -- pull-to-refresh calls `syncNow()` (line 537); convergence reference, unchanged.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- add `pub async fn pause_all(&self)`: clone each live account's `Arc<SyncService>` under the lock, then `sync.stop().await` outside it; doc it as the Epic 14-1 background-pause counterpart to `sync_now`; best-effort and infallible. -- graceful counterpart to the resume kick.
- [x] `src-tauri/crates/keeper/src/lifecycle.rs` -- NEW module: `LifecyclePhase` enum (`Foreground`/`Background`, serde `rename_all = "lowercase"`, ts-rs export to `src/lib/ipc/gen/`) and `#[tauri::command] pub async fn app_lifecycle_changed(state, phase)` routing `Background` → `pause_all()`, `Foreground` → `sync_now()`. -- the one Rust lifecycle entry.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- add `mod lifecycle;` and `lifecycle::app_lifecycle_changed` to `generate_handler!`; do not modify the existing window-hide/`RunEvent` handlers. -- wires the entry without touching desktop lifecycle.
- [x] `src/lib/ipc/client.ts` -- add `appLifecycleChanged(phase: LifecyclePhase)` wrapping `invoke<void>("app_lifecycle_changed", { phase })`; export the generated `LifecyclePhase`. -- typed frontend seam.
- [x] `src/hooks/use-app-lifecycle.ts` -- NEW hook: when `useIsReducedCapabilityPlatform()` is true, attach a `visibilitychange` listener that calls `appLifecycleChanged` with `"background"`/`"foreground"` derived from `document.visibilityState`; swallow IPC errors; remove the listener on cleanup / when the predicate flips false. Inert on desktop. -- the zero-native lifecycle driver.
- [x] `src/App.tsx` -- call `useAppLifecycle()` alongside the other top-level boot hooks. -- mounts the driver once.
- [x] `src-tauri/crates/keeper-core/src/account.rs` (tests) -- add `pause_all_with_no_live_accounts_is_a_noop`, mirroring the existing `sync_now` no-op test. -- guards infallible/no-op contract.
- [x] `src/hooks/use-app-lifecycle.test.ts` -- NEW: cover the I/O matrix — background & foreground dispatch on the reduced tier, no listener/no-op on desktop, no listener before hydration, swallowed IPC error. -- guards gating + routing.

**Acceptance Criteria:**
- Given the reduced-capability (iOS) tier, when the webview fires `visibilitychange` to hidden, then `app_lifecycle_changed("background")` runs and every live account's `SyncService` is stopped (long-poll paused, account state retained).
- Given the iOS tier, when the webview returns to visible, then `app_lifecycle_changed("foreground")` runs, delegates to `AccountManager::sync_now()`, and kicks each live `SyncService` via idempotent `start()` while cached mirrors are already rendered.
- Given the desktop tier, when visibility changes, then no lifecycle command is invoked and sync keeps running (Story 10.3 unregressed).
- Given pull-to-refresh (Story 13.6) and foreground resume, when either fires, then both converge on the single `AccountManager::sync_now()` sync-kick — no second lifecycle truth.
- Given the full change, when the quality gates run, then `bun run check`, `bun run check:rust`, and `bun run test:rust` all pass.

## Spec Change Log

<!-- Append-only. Empty until the first bad_spec loopback. -->

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3 (medium 1, low 2)
- defer: 3
- reject: 5
- addressed_findings:
  - `[medium]` `[patch]` Mount-time initial phase not dispatched — the hook only reacted to `visibilitychange` transitions, so an app that mounted (or hydrated capabilities) while already hidden never paused sync, silently failing the hidden⇒paused guarantee. The hook now emits the current visibility phase once on attach; added two tests (mount-while-hidden ⇒ `background`, mount-while-visible ⇒ `foreground`).
  - `[low]` `[patch]` Open-world visibility mapping — non-hidden collapsed to `"foreground"`. Mapping is now explicit: only `"visible"` → foreground and `"hidden"` → background; any other state is ignored so an off-screen state never spuriously kicks sync.
  - `[low]` `[patch]` `ipc.rs` module doc claimed to be the single home for `#[tauri::command]`s, which the spec-mandated `lifecycle.rs` seam contradicts; updated the doc to acknowledge `lifecycle.rs` as the deliberate app-lifecycle peer seam.
  - Deferred (3, logged to `deferred-work.md`): unthrottled foreground sync-kicks (debounce/coalesce), `pause_all`/`sync_now` transition ordering not serialized (generation token / lifecycle mutex), `pause_all` serial `stop()` loop (concurrent drain + timeout).
  - Rejected (5): `Result<(),IpcError>`-always-`Ok` (deliberately matches sibling `sync_now`); "iOS" gate wording (already framed as a capability read in code, matching `capabilities.ts`); `LifecyclePhase` crate-local binding test (verified non-bug — ts-rs auto-emits an export test and `bindings:check` guards drift); narrow subscribe-vs-`pause_all` snapshot race (matches accepted deferred subscription-lifecycle races; iOS suspends anyway); redundant rapid-toggle / live-account-resume tests (subsumed by the deferred concurrency items).

## Design Notes

- **Single entry, two seams.** The webview `visibilitychange` hook is the zero-native stopgap; a later micro Swift `UIApplication` plugin (`didEnterBackground` / `willEnterForeground`) will call the SAME `app_lifecycle_changed` command — recorded as the upgrade path, not built here. Keep the command signature stable for that.
- **Pause vs teardown.** Pause = `SyncService::stop()` (graceful; the sliding-sync long-poll ends cleanly, no mid-flight death), NOT `shutdown()` (which stops sync *and* aborts producers/streams for sign-out/quit). Resume relies on `sync_now`'s idempotent `start()` plus the SDK's snapshot-then-diff (AD-8): a resumed stream re-emits a full `Reset` first, so instant cached render then reconcile is automatic — no new frontend snapshot plumbing.
- **Behavioral bars are simulator-verified.** "renders instantly" and "new messages within 2 s on Wi-Fi" are Simulator/dogfooding bars, not unit-testable timings; the automated tests assert the routing, gating, and pause/resume wiring that make them possible.
- **Golden convergence.** The foreground branch body is exactly `state.accounts.sync_now().await` — the same call the `sync_now` command makes — so pull-to-refresh and foreground resume cannot diverge.

## Verification

**Commands:**
- `bun run check:rust` -- expected: `cargo fmt --check` clean + clippy `-D warnings` pass over the new `lifecycle` module and `pause_all`.
- `bun run test:rust` -- expected: cargo-nextest green, including `pause_all_with_no_live_accounts_is_a_noop`.
- `bun run check` -- expected: Biome + tsc + Vitest green, including `use-app-lifecycle.test.ts`.

## Auto Run Result

Status: done

**Summary:** Added the single Rust app-lifecycle entry for Epic 14-1. Backgrounding gracefully pauses every live account's `SyncService` (new `AccountManager::pause_all` → `stop()`); foregrounding routes through the existing `AccountManager::sync_now` sync-kick (idempotent `start()`), so pull-to-refresh (Story 13.6) and foreground resume converge on one core operation. A capability-gated frontend hook drives the entry from the webview `visibilitychange` event on the iOS/phone tier only — desktop attaches nothing, preserving Story 10.3 background operation. A future Swift `UIApplication` plugin will call the same command (upgrade path, not built here).

**Files changed:**
- `src-tauri/crates/keeper/src/lifecycle.rs` (new) — the one lifecycle command `app_lifecycle_changed` + `LifecyclePhase` enum (ts-rs exported).
- `src-tauri/crates/keeper-core/src/account.rs` — new `pause_all()` (mirrors `sync_now`, calls `stop()`); `pause_all_with_no_live_accounts_is_a_noop` test.
- `src-tauri/crates/keeper/src/lib.rs` — `mod lifecycle;` + command registration.
- `src-tauri/crates/keeper/src/ipc.rs` — module doc updated to acknowledge `lifecycle.rs` as a peer command seam (review patch).
- `src-tauri/crates/keeper/Cargo.toml` — added `ts-rs` dep so the shell crate exports `LifecyclePhase`.
- `src/lib/ipc/gen/LifecyclePhase.ts` (new, generated) — `"foreground" | "background"`.
- `src/lib/ipc/client.ts` — `appLifecycleChanged(phase)` wrapper + `LifecyclePhase` export.
- `src/hooks/use-app-lifecycle.ts` (new) — capability-gated `visibilitychange` driver; emits current phase on attach; explicit `visible`/`hidden` mapping.
- `src/hooks/use-app-lifecycle.test.ts` (new) — gating/routing + mount-time (hidden/visible) coverage.
- `src/App.tsx` — mounts `useAppLifecycle()`.

**Review findings breakdown:** 3 patches applied (1 medium: mount-time initial-phase dispatch so a launched-while-hidden app still pauses, with 2 new tests; 2 low: explicit visibility mapping, `ipc.rs` doc accuracy). 3 deferred to `deferred-work.md` (unthrottled foreground kicks; `pause_all`/`sync_now` transition-ordering serialization; `pause_all` concurrent-drain). 5 rejected (see Review Triage Log). No intent gaps, no spec repairs.

**Follow-up review recommended:** false — the three fixes are localized to one hook and one doc line, low/medium consequence, and fully test-covered.

**Verification:**
- `bun run check:rust` — PASS (fmt + clippy `-D warnings`).
- `bun run test:rust` — PASS (769 tests, incl. `pause_all_with_no_live_accounts_is_a_noop`).
- `bun run check` — PASS (Biome + tsc + Vitest 1170 tests, incl. the new `use-app-lifecycle` mount-time cases; core-tauri-free convention holds).

**Residual risks:** The three deferred hardening items (kick throttling, transition ordering, concurrent stop-drain) rely on matrix-sdk-ui's `start`/`stop` idempotency and OS-serialized visibility events — correct for the common single-event iOS case, narrow and self-healing under rapid toggling. The "instant render" / "within 2 s on Wi-Fi" behavioral bars are Simulator/SM-8-verified, not unit-tested.
