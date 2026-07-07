---
title: 'IPC subscribe() clears Channel handler on invoke rejection'
type: 'bugfix'
created: '2026-07-06'
status: 'blocked'
review_loop_iteration: 0
baseline_revision: '429a8f7275400ca6b401a74ddec7d93e75292ed5'
followup_review_recommended: false
context: []
warnings: []
---

<intent-contract>

## Intent

**Problem:** The shared `subscribe()` IPC helper (`src/lib/ipc/client.ts`) arms `channel.onmessage` *before* awaiting `invoke`, but nothing clears it when `invoke` rejects (e.g. `timelineUnavailable`/`syncUnavailable`). Every failed subscribe leaves a live `Channel` handler and its Tauri callback registration dangling; repeated retries of a failing room accumulate them. `subscribeInbox()` has the same shape across its six channels.

**Approach:** Wrap the `invoke` in `subscribe()` in a `try/catch` that nulls `channel.onmessage` before rethrowing the rejection unchanged. Apply the same cleanup to all six channels in `subscribeInbox()`. Preserve the load-bearing "arm before invoke" ordering (AD-8) — only the failure path changes.

## Boundaries & Constraints

**Always:** Keep `channel.onmessage = handler` set *before* `invoke` (AD-8 ordering is load-bearing). On rejection, null every armed channel's `onmessage` then rethrow the original error unchanged (still the {@link IpcError} envelope). Happy path stays byte-for-byte identical in behavior. Obey the TS/Biome rules (no `any`, 2-space, 100-char, double quotes).

**Block If:** (none — this is a self-contained, low-risk cleanup with no unresolved decisions)

**Never:** Do not swallow or transform the rejection. Do not change the return type, arguments, or resolve value of either function. Do not touch the Rust backend or any other IPC wrapper. Do not add channel teardown to the happy path.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| subscribe succeeds | `invoke` resolves with id | `onmessage` stays armed; returns the id; batches still forwarded | No error expected |
| subscribe rejects | `invoke` rejects with IpcError | `channel.onmessage` is null after the throw; original error propagates | Rethrow unchanged |
| subscribeInbox rejects | `invoke` rejects | all six channels' `onmessage` are null after the throw; original error propagates | Rethrow unchanged |

</intent-contract>

## Code Map

- `src/lib/ipc/client.ts` -- `subscribe<TBatch>()` (single channel) and `subscribeInbox()` (six channels) — the two helpers to harden.
- `src/lib/ipc/client.test.ts` -- Vitest suite; `MockChannel` records instances and exposes `onmessage`. Existing `subscribe` / `subscribeInbox` describe blocks to extend with rejection cases.

## Tasks & Acceptance

**Execution:**
- [ ] `src/lib/ipc/client.ts` -- In `subscribe()`, wrap `await invoke(...)` in `try/catch`; in the catch set `channel.onmessage = null` then `throw` the caught error. -- Drops the dangling handler on a failed subscribe while preserving arm-before-invoke ordering.
- [ ] `src/lib/ipc/client.ts` -- In `subscribeInbox()`, wrap the `await invoke(...)` in `try/catch`; in the catch null all six channels' `onmessage` (`channel`, `archive`, `pins`, `favourites`, `spaces`, `networks`) then rethrow. -- Same cleanup for the multi-channel subscribe.
- [ ] `src/lib/ipc/client.test.ts` -- Add a test that `subscribe()` rejects with the injected envelope AND the created channel's `onmessage` is `null` afterward; add the equivalent for `subscribeInbox()` asserting all six channels are cleared. -- Locks in the I/O matrix rejection rows.

**Acceptance Criteria:**
- Given `invoke` rejects, when `subscribe()` is awaited, then it rejects with the same error and the channel's `onmessage` is `null`.
- Given `invoke` rejects, when `subscribeInbox()` is awaited, then it rejects with the same error and every one of the six channels' `onmessage` is `null`.
- Given `invoke` resolves, when either helper is awaited, then behavior is unchanged: the id resolves and armed handlers still forward batches.

## Review Triage Log

### 2026-07-06 — Review pass
- intent_gap: 1: (high 0, medium 1, low 0)
- bad_spec: 0
- patch: 0
- defer: 0
- reject: 5
- addressed_findings:
  - none

**Intent-gap finding (root cause inside `<intent-contract>`):** Both reviewers (Blind Hunter + Edge Case Hunter) and independent verification against `node_modules/@tauri-apps/api/core.js` confirm the prescribed mechanism cannot achieve the intent's goal. The Tauri `Channel` registers its global callback in the **constructor** (`this.id = transformCallback(...)`, core.js:82); that registration is freed **only** by `unregisterCallback(this.id)` via `cleanupCallback()` (core.js:117-118). The `set onmessage` accessor (core.js:120-122) merely swaps a private handler field — it never touches the registration. Therefore nulling `channel.onmessage` on a rejected `invoke` does NOT "drop the channel" or stop "its Tauri callback registration [from dangling]" as DW-4 and this spec's intent-contract require; it is close to a no-op (at best releasing the `onBatch` reference on a Channel that persists in the Tauri internals map anyway). Fixing it correctly requires a human decision among materially different options: (a) call the undocumented internal `channel.cleanupCallback()` / `unregisterCallback` — using non-public Tauri API; (b) add a real backend unsubscribe/teardown for the failed-subscribe case (backend change, out of this spec's scope); or (c) close DW-4 as won't-fix, since on the failure path the backend spawns no producer and delivers no batch, making the residual a negligible per-failed-retry leak. There is not exactly one reading, so intent cannot be inferred unattended.

**Rejected (noise / moot under the intent_gap cascade):** `null as unknown as (...)` cast smell; comment-accuracy (a facet of the intent gap); "tests assert the mock not the real contract" (moot — code re-derived); "subscribeInbox is bespoke vs a shared helper" (by design — it is the only multi-channel path and cannot route through single-channel `subscribe()`); theoretical "catch block could throw and mask the original error" (the `set onmessage` accessor cannot throw here).

## Design Notes

The `MockChannel` in the test file defaults `onmessage` to `null`, so asserting `channel.onmessage === null` after a rejection cleanly proves the arm→clear round-trip. Prefer `try/catch` over `finally`: `finally` would also run on success and we must NOT clear the handler on the happy path.

## Verification

**Commands:**
- `bun run check` -- expected: biome lint + tsc + vitest all pass, including the new rejection tests.

## Auto Run Result

Status: blocked
Blocking condition: intent gap in intent contract

**Bundle:** ipc-subscribe-handler-cleanup (DW-4). Ledger NOT edited — the orchestrator records resolution.

**Outcome:** Planned → implemented → adversarial review → **reverted to baseline**. The working tree is clean except for this spec file. No code was committed.

**What was attempted:** In `src/lib/ipc/client.ts`, wrap the `invoke` in `subscribe()` and `subscribeInbox()` in `try/catch` that nulls `channel.onmessage` (all six channels for the inbox helper) before rethrowing, plus two rejection unit tests. `bun run check` passed clean on that draft.

**Why blocked (intent_gap):** The intent's prescribed mechanism — "null `channel.onmessage` on rejection to drop the channel / stop its Tauri callback registration from dangling" — is factually incapable of achieving that goal. Verified against `node_modules/@tauri-apps/api/core.js`: the Tauri `Channel` registers its global callback in the **constructor** (`this.id = transformCallback(...)`, line 82) and frees it **only** via `unregisterCallback(this.id)` through `cleanupCallback()` (lines 117-118). The `set onmessage` accessor (lines 120-122) only swaps a private handler field; it never unregisters. So nulling `onmessage` leaves the Tauri callback registration exactly as dangling as before (close to a no-op). Both independent reviewers reached the same conclusion.

**Decision required from a human (materially different options — cannot be inferred unattended):**
1. Call the undocumented internal `channel.cleanupCallback()` / `unregisterCallback` — the only path that truly frees the registration, but it is non-public Tauri API (version-fragile; against this project's conservative dependency posture).
2. Add a real backend unsubscribe/teardown for the failed-subscribe case — a Rust change, out of this spec's single-file scope.
3. Close DW-4 as won't-fix — on the failure path the backend spawns no producer and delivers no batch (per the DW-4 ledger's own "no functional bug" note), so the residual is a negligible per-failed-retry leak of one internals-map entry.

**Recommendation:** Option 3 (close as won't-fix) or option 1 if the leak is deemed worth an undocumented-API call. Route via `/bmad-loop-resolve dw-4-ipc-subscribe-handler-cleanup` for a human to disambiguate the frozen intent.

**Verification:** `git status --porcelain` shows only the untracked spec file; `git diff <baseline> -- src/lib/ipc/` is empty (code fully reverted to `429a8f7`).
