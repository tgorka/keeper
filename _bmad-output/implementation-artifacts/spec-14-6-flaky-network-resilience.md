---
title: 'Flaky-Network Resilience'
type: 'feature'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: ['oversized']
baseline_revision: '771f8e9aba1eaef011ab89eaf10948494107c615'
final_revision: '596d17c3caa1c4fca308f33eec8cfa9939aa1d3f'
---

<intent-contract>

## Intent

**Problem:** On the iOS/phone tier keeper already rides the matrix-sdk Simplified Sliding Sync offline mode + SDK-native backoff (Story 1.7), the single lifecycle sync-kick (14.1), the stale-resume guard (14.4), and a durable Undo-Send outbox + persistent SDK send queue — yet two phone surfaces are missing and one promise is unproven. There is **no persistent offline indicator on the phone**: the phone offline pill only appears mid pull-refresh (`refreshing && offline`) and the 14.4 "Connecting…" pill deliberately suppresses itself when offline, so a flapping connection is silent. The queued-send caption uses desktop wording that never tells an iOS user their message waits until keeper is foreground. And "queued sends survive suspension and dispatch on resume without a restart or silent loss" (NFR-5 → iOS) has no automated regression test.

**Approach:** Fill the two phone surface gaps and lock the resilience with tests — **add no new sync/backoff/queue machinery**. Add a persistent offline pill to the phone shell (reduced-capability tier), rendered from the existing `useShellOffline()` connection status in the same header slot as the pull/connecting pills, under a single-pill precedence so at most one connectivity indicator ever shows; it clears on reconnect with no toast. Tier-gate the outgoing "Queued …" caption so iOS reads "Queued — sends when keeper is open and back online" while desktop keeps "…when you're back online". Add a Rust regression test proving an Undo-Send outbox row whose window elapsed during a paused/suspended period is durable (no loss) and selected as due on the next scheduler tick after resume (dispatches with no app restart). Ledger the on-device airplane-mode / Wi-Fi↔cellular scenarios as SM-8 dogfooding.

## Boundaries & Constraints

**Always:** Reuse the existing connectivity source (`useShellOffline()` / the Rust-streamed `ConnectionStatus`) and the existing `OFFLINE_PILL_TEXT` copy — one source of truth, no second connectivity store. Persistent offline pill and the caption wording change are **reduced-capability (iOS/phone) tier only**, gated on `useIsReducedCapabilityPlatform()`; desktop stays byte-identical. At most one connectivity pill visible at a time — precedence: active pull gesture > refresh spinner / pull-offline pill > persistent offline pill > "Connecting…" pill. All connectivity indicators clear on reconnect with no toast/alert; the UI keeps rendering from the local mirror throughout. Preserve one-lifecycle-truth (AD-30): dispatch-on-resume rides the *existing* sync-kick and the *existing* outbox scheduler tick. Rust: no `.unwrap()`/bare `.expect()` in prod paths, `?` + `thiserror`. TS: no `any`, `import type` for types.

**Block If:** Investigation shows the existing outbox scheduler + SDK send queue actually **loses** a queued send across a suspend/resume (silent loss, violating NFR-5) and the fix requires breaking the single-lifecycle-entry invariant or adding native code — HALT (phase-level architecture decision). Or if the persistent offline pill cannot be reconciled with the 14.4 "Connecting…" pill without visible flicker/stacking that a precedence rule cannot resolve — HALT.

**Never:** Do not build custom backoff or a new offline-detection mechanism — matrix-sdk `.with_offline_mode()` + the reconnect supervisor own that. Do not add a native Swift reachability plugin (zero-native posture, AD-30). Do not add a second `visibilitychange` listener or a separate outbox/sync "kick" — the ~250 ms scheduler tick after the process unfreezes already drains elapsed rows. Do not add toast/banner spam on connectivity change — pill only. Do not change desktop connectivity UI or the desktop caption wording. Do not turn the on-device airplane-mode / Wi-Fi↔cellular scenarios into a story-blocking automated gate (SM-8 dogfooding).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Phone goes offline | reduced tier, `useShellOffline()` true, no pull/refresh in flight | Persistent offline pill shows (`OFFLINE_PILL_TEXT`); UI keeps rendering from local mirror | Best-effort; no toast |
| Phone reconnects | status → online | Offline pill clears; no app restart, no blank webview | Swallow |
| Offline during stale resume | reduced tier, offline within the post-resume window | Offline pill shows; "Connecting…" pill stays suppressed (offline owns the slot) | Single-pill precedence |
| Own message queued while offline | reduced tier, own msg `sending` + offline | Caption "Queued — sends when keeper is open and back online" | Pure projection |
| Same on desktop | desktop tier, own msg `sending` + offline | Caption "Queued — sends when you're back online" (unchanged) | Pure projection |
| Undo window elapsed while suspended | outbox row with `dispatch_at_ts` in the past after a paused period | Row is durable (not lost), selected as due, dispatched on the next scheduler tick after resume — no restart | Row left to retry if room unresolved |
| Desktop / pre-hydration | any connectivity | No persistent phone pill; connectivity UI byte-identical to today | No error expected |

</intent-contract>

## Code Map

- `src/components/layout/phone-shell.tsx` -- the shared header-slot pills (pull-indicator / pull-offline / "Connecting…"); the offline pill currently only renders under `refreshing && offline` (l.616) and the connecting pill is suppressed when offline (l.659) — add the persistent offline pill here with the single-pill precedence.
- `src/hooks/use-stale-resume-pill.ts` -- the 14.4 "Connecting…" hook; its doc (l.17-19) already cedes the persistent offline surface to Story 14.6. No logic change needed — it already requires `!offline`; just confirm precedence holds.
- `src/components/layout/sidebar-pane.tsx` -- exports `OFFLINE_PILL_TEXT` and the desktop `WifiOff` pill; reuse the constant (already imported by phone-shell) — do not fork the copy.
- `src/lib/stores/account-status.ts` -- `useShellOffline()` (all signed-in accounts offline) + `ConnectionStatus` = `"online" | "offline"`; the connectivity source.
- `src/lib/stores/capabilities.ts` -- `useIsReducedCapabilityPlatform()` tier gate.
- `src/components/chat/message-bubble.tsx` -- `SendStateCaption` (l.532-536) renders the offline "Queued …" caption; tier-gate the wording.
- `src/components/chat/conversation-pane.tsx` -- derives `offline` via `useAccountStatus` and passes it to `MessageBubble` (l.499); resolve the tier here and thread it the same way.
- `src-tauri/crates/keeper-core/src/account.rs` -- `run_outbox_scheduler` (l.388), due-filter `dispatch_at_ts <= now` (l.425), `registry::list_outbox_rows_for_account`, `hold_send`; existing outbox tests at `outbox_test_dir` (l.6302+). Add the elapsed-during-suspension regression test here.
- `_bmad-output/implementation-artifacts/deferred-work.md` -- append the 14.6 SM-8 on-device network scenarios.

## Tasks & Acceptance

**Execution:**
- [x] `src/components/layout/phone-shell.tsx` -- add a **persistent** offline pill in the existing header-slot (`top-[calc(var(--safe-top)+var(--phone-header))]`), shown when `offline && !refreshing && pullDy === null` on the reduced tier, using `OFFLINE_PILL_TEXT` + `WifiOff` + `role="status"` (match the existing `pull-offline-pill` styling; new `data-testid` e.g. `offline-pill`). Enforce the single-pill precedence so pull gesture, refresh spinner/pull-offline, this persistent offline pill, and the "Connecting…" pill are mutually exclusive (the connecting pill already requires `!offline`; keep it that way). Clears on reconnect; never a toast. Update the l.604-608 comment so it no longer claims the pull-indicator is the persistent surface.
- [x] `src/components/chat/message-bubble.tsx` + `src/components/chat/conversation-pane.tsx` -- tier-gate ONLY the offline `sending` caption wording: reduced-capability tier reads `Queued — sends when keeper is open and back online`; desktop keeps `Queued — sends when you're back online`. Resolve the tier where `offline` is already derived (`conversation-pane`, via `useIsReducedCapabilityPlatform()`) and thread it to `SendStateCaption` the same way `offline` flows — do NOT read a hook inside `SendStateCaption` if it is invoked as a plain function. Leave `failed`/`sent`/`Sending…`/`Read ✓` untouched; do not touch the undo-send countdown pill.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- extract the inline due-check into a small pure predicate `fn outbox_row_due(dispatch_at_ts: i64, now: i64) -> bool { dispatch_at_ts <= now }`, use it in the scheduler filter (l.425), and add a regression test (in the existing outbox test module) that: persists an outbox row whose `dispatch_at_ts` is already in the past (models a window that elapsed while the app was suspended), asserts `registry::list_outbox_rows_for_account` still returns it (durable — no silent loss across the paused period), and asserts `outbox_row_due(row.dispatch_at_ts, now_ms())` is `true` (the scheduler will dispatch it on its next tick after resume — no restart needed). The test must exercise the real predicate and the real registry, not re-implement the comparison.
- [x] `src/components/layout/phone-shell.test.tsx` (extend) + `src/components/chat/message-bubble.test.tsx` (extend) -- unit-test the I/O matrix: persistent offline pill appears when offline and clears on reconnect; precedence (offline pill wins over "Connecting…"; suppressed during pull/refresh); desktop tier shows no persistent phone pill; the caption reads the iOS wording on the reduced tier and the desktop wording on desktop.
- [x] `_bmad-output/implementation-artifacts/deferred-work.md` -- append the 14.6 SM-8 dogfooding bars: airplane-mode toggle and a real Wi-Fi↔cellular handover each recover unaided (no restart, no lost message), findings ledgered; note the automated suite covers the pill/caption/durability but on-device radio behavior is Simulator-unverifiable.

**Acceptance Criteria:**
- Given the reduced (iOS) tier with all accounts offline and no pull/refresh gesture, when connectivity is lost, then a persistent offline pill is visible, the UI keeps rendering from the local mirror, and no toast appears; when connectivity returns, the pill clears with no app restart and no blank webview.
- Given a stale foreground resume that is still offline, then the persistent offline pill (not the "Connecting…" pill) owns the header slot — at most one connectivity indicator shows at any time.
- Given an own message queued while offline, then on iOS it reads "Queued — sends when keeper is open and back online" and on desktop it reads "Queued — sends when you're back online"; the send is never silently lost.
- Given an Undo-Send row whose window elapsed while the app was suspended, then the row survives (durable) and dispatches on the next scheduler tick after resume without an app restart (automated portion); the on-device airplane-mode / Wi-Fi↔cellular recovery is recorded as an SM-8 dogfooding item, unaided recovery being the bar.

## Design Notes

**Most of this story already exists — 14.6 fills gaps and proves promises.** The flaky-network spine was built in Story 1.7 (matrix-sdk `.with_offline_mode()` + SDK-native reconnect backoff, the reconnect supervisor re-enabling the send queue on transition to `Running`), 14.1 (the single `sync_now()` kick that both foreground-resume and pull-to-refresh converge on), and 14.4 (the stale-resume restart guard and "Connecting…" pill). The Undo-Send outbox (Story 8.3) is durable in `keeper.db` and its scheduler already retries rows that elapsed while the app was closed (`account.rs:383`). So 14.6 adds **no** new networking: it adds the phone's missing persistent offline surface, the honesty-correct iOS caption, and regression tests — and ledgers the radio scenarios that only a device can prove.

**Why no explicit outbox "kick" on resume.** On iOS the whole process freezes while suspended, so the ~250 ms scheduler tick naturally pauses and resumes; on foreground the next tick drains every elapsed row within a tick interval. An explicit lifecycle→outbox call would add a second truth (fighting AD-30) for no correctness gain — dispatch-then-delete + the SDK send queue already guarantee at-least-once with no loss.

**Single-pill precedence.** The header slot is shared by the pull affordance, the refresh spinner / pull-offline pill, the new persistent offline pill, and the "Connecting…" pill. They must be mutually exclusive so the phone never stacks two connectivity indicators: gesture > refresh > offline > connecting. The connecting pill already gates on `!offline`, so adding the offline pill closes the gap the 14.4 comment explicitly left for this story.

## Verification

**Commands:**
- `bun run check` -- expected: biome + tsc + vitest green (phone-shell offline-pill + message-bubble caption tests pass).
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` clean.
- `bun run test:rust` -- expected: the new `outbox_row_due` / elapsed-while-suspended durability test passes.

**Manual checks (SM-8 / not story-blocking):**
- On device: toggle airplane mode and perform a real Wi-Fi↔cellular handover with keeper foreground; the offline pill appears and clears, sync recovers unaided (no restart), and any message composed while disconnected sends on reconnect. Ledgered as SM-8 dogfooding.

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (low 3)
- defer: 0
- reject: 5: (low 5)
- addressed_findings:
  - `[low]` `[patch]` The Rust test `outbox_row_elapsed_while_suspended_is_durable_and_due` comment/name oversold its scope — it claimed to prove dispatch-on-resume across a real suspend/resume, but only asserts row persistence + the due-predicate (never drives `run_outbox_scheduler`). Rewrote the doc-comment to state honestly what it pins (durability via a fresh `list_outbox_rows_for_account` re-open + due-selection by the real `outbox_row_due`), scoping the live-scheduler tick to the on-device SM-8 bar.
  - `[low]` `[patch]` The extracted `outbox_row_due` `<=` boundary (a window ending exactly on a tick) was unasserted — added `outbox_row_due_at_and_around_the_boundary` locking `<=` (already-elapsed due, `== now` due, future not-yet-due) against a silent `<=`→`<` refactor.
  - `[low]` `[patch]` The two appended deferred-work SM-8 entries were not blank-line separated (inconsistent with the ledger's entry spacing) — added the separator.
- rejected (by-design / not this story's problem): sub-reveal-drag transient no-indicator gap (spec-accepted precedence; test asserts it); spinner→pull-offline-pill swap mid-refresh (single-pill-correct, identical `OFFLINE_PILL_TEXT` copy); persistent-pill lag after a fully-offline refresh (the `pull-offline-pill` shows identical copy during that window — no real gap); `outbox_row_due` has no overflow clamp (unreachable via `hold_send`; behavior-identical to the pre-existing inline filter); phone pill lacks the sidebar's `aria-label`/`sr-only` (visible text yields the accessible name under `role="status"` — reviewer confirmed not a defect).

## Auto Run Result

Status: done

**Summary:** Story 14.6 makes keeper's flaky-network behavior honest and complete on the iOS/phone tier. The heavy lifting already existed — matrix-sdk Simplified Sliding Sync offline mode + SDK-native backoff (Story 1.7), the single `sync_now()` lifecycle kick shared by foreground-resume and pull-to-refresh (14.1), the stale-resume restart guard and "Connecting…" pill (14.4), and a durable Undo-Send outbox + persistent SDK send queue (8.3). This story adds the two phone surfaces that were missing and locks the resilience with regression tests: (1) a **persistent offline pill** on the reduced-capability tier (before this, the phone offline pill only appeared mid pull-refresh and the 14.4 "Connecting…" pill suppresses itself when offline, so a flapping connection was silent on the phone); (2) a **tier-gated queued-send caption** — iOS reads "Queued — sends when keeper is open and back online" (honest about foreground-only sync) while desktop keeps "…when you're back online"; (3) a **Rust regression test** proving an Undo-Send row whose window elapsed while suspended is durable (no silent loss) and selected as due by the scheduler's predicate. All connectivity indicators are mutually exclusive by construction (gesture > refresh > offline > connecting), and the on-device airplane-mode / Wi-Fi↔cellular scenarios are ledgered as SM-8 dogfooding.

**Files changed:**
- `src/components/layout/phone-shell.tsx` — new persistent offline pill (`data-testid="offline-pill"`, `role="status"`, `WifiOff` + reused `OFFLINE_PILL_TEXT`) rendered when `isReducedCapability && offline && !refreshing && pullDy === null`, with single-pill precedence; corrected the pull-indicator comments so neither claims the pull surface is the persistent one.
- `src/components/chat/message-bubble.tsx` — new `reducedCapability?: boolean` prop threaded to `SendStateCaption`; only the offline `sending` "Queued …" caption is reworded on the reduced tier; every other caption state untouched.
- `src/components/layout/conversation-pane.tsx` — resolves the tier via `useIsReducedCapabilityPlatform()` where `offline` is derived and passes `reducedCapability` to `MessageBubble`.
- `src-tauri/crates/keeper-core/src/account.rs` — extracted the pure predicate `outbox_row_due(dispatch_at_ts, now)` used in the scheduler filter; added `outbox_row_elapsed_while_suspended_is_durable_and_due` (durability + due-selection) and `outbox_row_due_at_and_around_the_boundary` (the `<=` boundary).
- `src/components/layout/phone-shell.test.tsx` — new 7-test suite for the persistent offline pill (shows/clears, precedence over Connecting…/pull/refresh, desktop + pre-hydration never render).
- `src/components/chat/message-bubble.test.tsx` — 3 tests: iOS wording on the reduced tier, desktop wording by default, other caption states untouched.
- `_bmad-output/implementation-artifacts/deferred-work.md` — two Story 14.6 SM-8 dogfooding entries (airplane-mode toggle; Wi-Fi↔cellular handover — unaided recovery, no restart, no lost message).

**Review findings breakdown:** 2 reviewers (Blind Hunter + Edge Case Hunter, Opus). 0 intent_gap, 0 bad_spec (implementation was faithful to the spec). 3 low patches applied (test-comment honesty; `<=` boundary test; ledger blank line). 5 low rejected (by-design precedence gaps, a non-gap, an unreachable/pre-existing overflow case, and a confirmed non-defect accessibility nit). No re-derivation loopback.

**Follow-up review recommended:** false — the final pass made only a few localized, low-consequence fixes (two test/comment honesty improvements, one added boundary assertion, one whitespace ledger fix); no production behavior, API, security, or data change.

**Verification:**
- `bun run check` — PASS (biome clean, tsc clean, 1245 vitest tests / 116 files, core-tauri-free check clean).
- `bun run check:rust` — PASS (`cargo fmt --check` clean, `clippy --all-targets -D warnings` clean).
- `bun run test:rust` — PASS (786 nextest, incl. both new outbox tests).

**Residual risks:**
- The end-to-end radio recovery (airplane-mode toggle, real Wi-Fi↔cellular handover) is device-only and unverifiable in the Simulator/jsdom — carried as SM-8 dogfooding; the automated suite covers the pill, caption, and outbox durability, and recovery rides the already-shipped matrix-sdk offline mode + reconnect supervisor + 14.1 sync kick with no new machinery.
- The Rust regression test proves durability (fresh-open re-read) and due-selection, not the live scheduler tick firing across a real freeze/resume — that last step is the SM-8 bar by design.
