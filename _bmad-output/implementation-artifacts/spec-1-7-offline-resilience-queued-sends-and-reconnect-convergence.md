---
title: 'Story 1.7 — Offline Resilience: Queued Sends and Reconnect Convergence'
type: 'feature'
created: '2026-07-04'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: 'cd8fb126aca7b9854bb110463b2a31da13072a22'
final_revision: '5d531f448ca1d6c7703d839e0926d4df8e917522'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** After Story 1.6 a user can send text and see honest `Sending…`/`Sent`/`Failed — Retry` states, but only while online. On flaky Wi-Fi a message composed offline has no honest home: there is no "queued" state, no auto-dispatch on reconnect, and no persistent signal that the app is disconnected. Epic 1's vertical slice is incomplete until offline-composed messages queue visibly, dispatch themselves on reconnect, and survive a force-quit — and the room list reconverges cleanly after a long gap.

**Approach:** Lean entirely on the SDK's native, persistent send queue rather than building our own. In `keeper-core`, build the per-account `SyncService` with `.with_offline_mode()` so it exposes a real `Offline` state, stream that connectivity as a new **connection-status** channel (`ConnectionStatus ∈ { Online, Offline }`), and on every transition back to `Running` call `client.send_queue().set_enabled(true)` (idempotent — also respawns persisted unsent requests) so queued sends dispatch automatically. On the frontend, a persistent sidebar-footer **offline pill** and an amber **"Queued — sends when you're back online"** caption are rendered as *pure projections* of two Rust-authoritative streams: the existing per-item `sendState` and the new connection status. Reconnect convergence and send-queue persistence across force-quit are already provided by matrix-sdk (sliding-sync recovery + SQLite-backed queue) — this story wires the seams and proves them, it does not reimplement them.

## Boundaries & Constraints

**Always:**
- All connectivity/queue/persistence logic stays in `keeper-core`; the `keeper` shell stays IPC/platform glue only, gains no new business logic, no `tauri` dep leaks into core. (AD-6)
- **Rely on SDK-native queueing (no custom queue/persistence).** Queued sends, retry/backoff, and cross-restart persistence are owned by `matrix_sdk::send_queue::SendQueue` + its SQLite store. This story adds exactly: `.with_offline_mode()` on the `SyncService`, a connectivity observer, and a reconnect handler that calls `client.send_queue().set_enabled(true)`. It does **not** add a keeper-side outbox, txn tracking, or persistence.
- **Connection status is a Rust-authoritative stream (AD-8).** New command `connection_status_subscribe(account_id, Channel<ConnectionStatusBatch>) -> subscription_id` and `connection_status_unsubscribe(...)`. The stream opens with a full snapshot (current mapped status), then emits on change (dedupe consecutive-equal). Reuse the exact lazy account-activation + supervised-task + self-reap lifecycle as `subscribe_room_list` (register the `JoinHandle` in the subscriptions map; abort on unsubscribe and on `shutdown`). `ConnectionStatusBatch { status: ConnectionStatus }`; both derive `serde` + `ts_rs::TS` with `#[ts(export)]`, camelCase (`"online"|"offline"`).
- **Connectivity mapping.** Pure `map_connection_status(&sync_service::State) -> ConnectionStatus`: `Running → Online`; `Idle | Terminated | Error(_) | Offline → Offline`. The pill and queued caption both derive from this one signal.
- **Auto-dispatch on reconnect.** Per account, a lifetime-of-account supervised task observes `sync.state()`; on any transition **into** `Running` it calls `client.send_queue().set_enabled(true).await` (idempotent; re-enables any room queue a recoverable error disabled and respawns unsent tasks). At activation, after `sync.start()`, call `client.send_queue().set_enabled(true).await` once so persisted queued sends from a prior process reload and dispatch (force-quit resilience). Store the supervisor's `JoinHandle` on the `AccountHandle`; abort it in `shutdown` and on partial activation teardown.
- **"Queued" is a pure presentation projection, not invented state (AD-8/AD-9/AD-20).** The `SendState` Rust enum stays `{ Sending, Sent, Failed }` — **no** new Rust variant and **no** connectivity coupling in `item_to_vm` (the SDK timeline does not re-emit items on a global connectivity flip, so a Rust `Queued` variant would require synthetic re-emits — explicitly avoided). Instead the frontend renders the amber "Queued — sends when you're back online" caption when `connectionStatus === "offline" && item.sendState === "sending" && item.isOwn`, in place of "Sending…". No timeline item is invented, mutated, re-ordered, or removed; both inputs are Rust-streamed truth. This mirrors how Story 1.6's captions are already a pure projection of `sendState` + `groupTail`.
- **Offline pill (UX-DR18).** Persistent sidebar-footer element: exact text `Offline — showing your local archive. Messages queue until you're back.`, rendered only while `status === "offline"`, using the `held` amber tokens (`text-held`/`bg-held`), `role="status"`, keyboard-irrelevant (non-interactive). No toasts for connectivity, ever — flapping just shows/hides the persistent pill (no spam by construction).
- **Queued caption (UX-DR10).** Amber `text-held`, sentence case, no error codes/emoji; renders on the group tail like `Sending…`/`Sent`. On reconnect the item's `sendState` advances to `Sent` via the existing timeline stream and the caption follows automatically.
- TS: no `any`, `import type`, `@/` alias, 2-space/100-col/double-quote Biome, `cn()` for classes, reuse installed shadcn primitives — never hand-write in `src/components/ui/`. New store is a vanilla zustand store created outside React (AD-9), holding only the Rust-streamed status. Rust: no `.unwrap()`/bare `.expect()` in production paths, `?` + `thiserror`, clippy `-D warnings` clean, `tracing` (account id / subscription id only — never message body, token, txn id, or event id).
- Regenerate ts-rs bindings for the new VMs (`ConnectionStatus`, `ConnectionStatusBatch`) into `src/lib/ipc/gen/` and commit them to match cargo output. No change to `SendState.ts`, `TimelineItemVm.ts`, or `IpcErrorCode.ts` (no new error code — connection-subscribe activation failures reuse `SyncUnavailable`).

**Block If:**
- matrix-sdk-ui 0.18 lacks `SyncServiceBuilder::with_offline_mode()`, `SyncService::state() -> Subscriber<State>` with the `{ Idle, Running, Terminated, Error, Offline }` shape, or matrix-sdk 0.18 lacks `Client::send_queue().set_enabled(bool)` / `respawn_tasks_for_rooms_with_unsent_requests()` — a stack-anchor conflict. (Verified present in the vendored 0.18.0 source during planning; only block if implementation proves otherwise.)

**Never:**
- No keeper-side outbox, message-persistence layer, txn-id tracking, or reconnect/convergence reimplementation — matrix-sdk owns queueing, retry, persistence, and sliding-sync reconvergence. No new `SendState` variant; no connectivity read inside `item_to_vm`.
- No session-restore-on-launch UX, sign-out, or `AccountManager::shutdown` wiring — that is Story 1.8 (this story only ensures the reconnect supervisor is torn down by an existing `shutdown` call, it does not add a new sign-out path). No multi-account. No typing indicators, read receipts, edits/redactions, media send, or approval pane. No changes to the `send::submit` single-dispatch gate (FR-41/AD-13) or to Story 1.5's receive path beyond what is listed. No `matrix-js-sdk`; no crypto/token logic in TS.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Send while offline | `status === offline`, user sends | message dispatches through the unchanged `send::submit` gate; local echo appears with `sendState: "sending"` (SDK `NotSentYet`); the frontend renders amber `Queued — sends when you're back online` (not "Sending…"); pill already visible | none (queued by SDK) |
| Reconnect with queued sends | connectivity returns → `sync.state()` → `Running` | reconnect supervisor calls `send_queue().set_enabled(true)`; SDK dispatches queued sends; each echo's `sendState` advances to `sent` over the timeline stream; captions flip `Queued → Sent`; pill hides | none on happy path |
| Offline pill lifecycle | `sync.state()` flips `Running`→`Offline`→`Running` | connection stream emits `offline` then `online`; footer pill shows then hides; no toast; deduped snapshots | none |
| Connection subscribe, account not yet active | first `connection_status_subscribe` before room-list subscribe | lazily activates the account (same path as room-list), then streams status | activation failure → `SyncUnavailable` (retriable) |
| Force-quit while queued | queued sends persisted in SDK store, app relaunched, account re-activates | `set_enabled(true)` at activation reloads persisted unsent requests; their local echoes reappear in the timeline as `sending`, render as `Queued` while offline, dispatch on reconnect | none (SDK persistence) |
| 24 h offline gap then reconnect | sliding-sync session stale, `RoomListService` enters `Recovering` | room list reconverges to server state with no duplicate/missing chats automatically; connection stream reports `online` once `Running` | none (SDK sliding-sync recovery) |
| Online send (regression) | `status === online`, user sends | unchanged Story-1.6 behavior: `Sending…` → `Sent`; no "Queued" caption | as Story 1.6 |
| Send permanently fails (regression) | SDK `SendingFailed { is_recoverable: false }` | persistent `Failed — Retry` (unchanged); "Queued" never overrides `failed` | as Story 1.6 |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- add `ConnectionStatus` unit enum (`Online|Offline`, `#[serde(rename_all="camelCase")]`, `#[ts(export)]`) and `ConnectionStatusBatch { status: ConnectionStatus }` (`#[ts(export)]`); serde round-trip tests. No change to `SendState`/`TimelineItemVm`/`IpcErrorCode`.
- `src-tauri/crates/keeper-core/src/account.rs` -- (1) `activate()`: build `SyncService::builder(client).with_offline_mode().build()`; after `sync.start()`, `client.send_queue().set_enabled(true).await` (reload + enable). (2) spawn a lifetime **reconnect supervisor** task observing `sync.state()`, calling `send_queue().set_enabled(true)` on transitions into `Running`; store its `JoinHandle` on `AccountHandle` (new field), abort in `shutdown` + partial-teardown. (3) `subscribe_connection_status(&self, platform, account_id, sink) -> Result<u64, CoreError>` mirroring `subscribe_room_list` (lazy activate/reuse handle, spawn producer over `sync.state()` that emits an initial snapshot then deduped diffs, register/self-reap `JoinHandle`). (4) pure `map_connection_status(&sync_service::State) -> ConnectionStatus` + unit test for the constructible variants (`Idle`,`Running`,`Terminated`,`Offline`).
- `src-tauri/crates/keeper/src/ipc.rs` -- `#[tauri::command] async fn connection_status_subscribe(state, account_id, channel: Channel<ConnectionStatusBatch>) -> Result<u64, IpcError>` and `connection_status_unsubscribe(state, account_id, subscription_id) -> Result<(), IpcError>`; route through the existing `to_ipc_error` (no new code).
- `src-tauri/crates/keeper/src/lib.rs` -- register both commands in `generate_handler!`.
- `src/lib/ipc/gen/` -- regenerated: NEW `ConnectionStatus.ts`, `ConnectionStatusBatch.ts`.
- `src/lib/ipc/client.ts` -- `subscribeConnectionStatus(accountId, onBatch): Promise<number>` (via shared `subscribe`), `unsubscribeConnectionStatus(accountId, id): Promise<void>`; re-export `ConnectionStatus`.
- `src/lib/stores/connection.ts` -- NEW vanilla zustand store `{ status: ConnectionStatus }` (default `"online"` — no false-offline flash before the first snapshot), `applyBatch(b)`, `reset()`, `useConnectionStore` selector hook.
- `src/hooks/use-connection-status.ts` -- NEW effect hook: reads `accountId` from the accounts store, subscribes on mount/account change, applies batches to the connection store, unsubscribes + `reset()` on cleanup/account clear (mirror `chat-list-pane`'s subscribe lifecycle).
- `src/components/layout/app-shell.tsx` -- call `useConnectionStatus()` once (the always-mounted signed-in root).
- `src/components/layout/sidebar-pane.tsx` -- render the persistent offline pill in a footer (`mt-auto` + `border-t`), shown only when `useConnectionStore(status)==="offline"`, amber `held` tokens, `role="status"`, exact text.
- `src/components/chat/message-bubble.tsx` -- add optional `offline?: boolean` prop (default `false`); in the send-state caption, when `offline && sendState === "sending"` render amber `Queued — sends when you're back online` (`text-held`, group-tail) instead of `Sending…`. `sent`/`failed` unaffected.
- `src/components/layout/conversation-pane.tsx` -- read `useConnectionStore(status)`, pass `offline={status === "offline"}` to each `MessageBubble`.
- Tests: Rust unit (`vm.rs` serde round-trip for `ConnectionStatus`/`ConnectionStatusBatch`; `account.rs` `map_connection_status`); frontend (`connection.test.ts` store applyBatch/reset; `use-connection-status.test.ts` subscribe/unsubscribe lifecycle with mocked client; `sidebar-pane.test.tsx` pill shown offline / hidden online + exact text; `message-bubble.test.tsx` queued caption when offline+sending, `Sending…` when online, `sent`/`failed` unaffected; `conversation-pane.test.tsx` `offline` prop wired). Keep `offline` optional so existing `MessageBubble` fixtures need no churn.

## Tasks & Acceptance

**Execution:**
- [x] `keeper-core/src/vm.rs` -- `ConnectionStatus` enum + `ConnectionStatusBatch`; serde round-trip tests.
- [x] `keeper-core/src/account.rs` -- `.with_offline_mode()`; activation `set_enabled(true)`; reconnect supervisor task (+ `AccountHandle` field, aborted on shutdown/teardown); `subscribe_connection_status` (room-list-style lifecycle); pure `map_connection_status` + unit test.
- [x] `keeper/src/ipc.rs` -- `connection_status_subscribe`/`connection_status_unsubscribe` commands via `to_ipc_error`.
- [x] `keeper/src/lib.rs` -- register both commands.
- [x] regenerate ts-rs bindings; commit NEW `ConnectionStatus.ts` + `ConnectionStatusBatch.ts`.
- [x] `src/lib/ipc/client.ts` -- `subscribeConnectionStatus`/`unsubscribeConnectionStatus` wrappers + `ConnectionStatus` re-export.
- [x] `src/lib/stores/connection.ts` (+ test) -- store, `applyBatch`, `reset`, selector hook.
- [x] `src/hooks/use-connection-status.ts` (+ test) -- subscribe/unsubscribe lifecycle keyed on `accountId`.
- [x] `src/components/layout/app-shell.tsx` -- mount `useConnectionStatus()`.
- [x] `src/components/layout/sidebar-pane.tsx` (+ test) -- persistent offline pill (exact text, `held` tokens, `role="status"`).
- [x] `src/components/chat/message-bubble.tsx` (+ test) -- `offline` prop → amber `Queued` caption in place of `Sending…`.
- [x] `src/components/layout/conversation-pane.tsx` (+ test) -- wire `offline` from the connection store to bubbles.

**Acceptance Criteria:**
- Given the machine is offline, when the user sends a message, then it renders with the amber `Queued — sends when you're back online` caption and dispatches automatically on reconnect, resolving to `Sent`; and the sidebar footer shows a persistent `Offline — showing your local archive. Messages queue until you're back.` pill while disconnected, with no toasts on connection flapping (FR-9, UX-DR10, UX-DR18).
- Given a 24 h offline gap, when the app reconnects, then the room list converges to server state with no duplicate and no missing chats (FR-8) — provided by matrix-sdk sliding-sync recovery, not reimplemented; the connection stream returns to `online` once sync reaches `Running`.
- Given a force-quit while messages are queued, when the app relaunches, then queued messages are still visible in their queued state and dispatch on connectivity (NFR-5, NFR-8) — provided by the SDK's SQLite-backed send queue plus `set_enabled(true)`/respawn at activation.
- Given AD-8/AD-9/AD-20, then connection status is a Rust-authoritative snapshot-then-diff stream, the offline pill and "Queued" caption are pure projections of Rust-streamed signals, the TS store never invents/mutates/re-orders/removes timeline items, and no token, txn id, event id, or message plaintext crosses IPC or reaches `tracing` (NFR-9).
- Given the FR-41 gate, then `send::submit` remains the sole content-dispatch entry point — this story adds no new `Timeline::send`/`send_queue().send` call site.
- Given the quality gates, when `bun run check`, `bun run check:rust`, `bun run test:rust`, and `cargo deny check` (from `src-tauri/`) run, then all pass.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 0, low 2)
- defer: 0
- reject: 6
- addressed_findings:
  - `[low]` `[patch]` The "Queued" caption gated only on `offline && sendState === "sending"`, but the spec's Always rule names three conjuncts incl. `item.isOwn`. Harmless today (only own local echoes carry `sending`), but a direct deviation from the stated rule and a fragile coupling to an SDK invariant. Threaded `isOwn` into `SendStateCaption` and gated the amber branch on `offline && isOwn`; added a test that a non-own `sending` message never shows "Queued". `message-bubble.tsx`, `message-bubble.test.tsx`.
  - `[low]` `[patch]` The collapsed-rail offline pill was a `role="status"` live region with only an `aria-label` and an `aria-hidden` icon — no text content. Screen readers that announce a live region's *content* (not its label) could announce nothing on connectivity change. Added an `sr-only` text child alongside the `aria-label` so both content-announcing and label-announcing ATs read the message. `sidebar-pane.tsx`.
  - Rejected (6, verified correct / pre-existing / documented residual): reconnect-supervisor `was_running` seed "missing a Running edge" (Blind Hunter verified via eyeball/sync_service source that `Running` is set synchronously before `start().await` returns and `get()` doesn't mark observed, and `set_enabled(true)` is idempotent → correct and self-healing); producer self-reap racing the register insert (pattern copied verbatim from shipped `subscribe_room_list`/`subscribe_timeline`; worst case is a finished `JoinHandle` entry cleared on unsubscribe/shutdown, cannot grow unbounded — pre-existing); dead-account spawn→register gap returning `Ok(id)` when `!did_activate` (mirrors siblings; requires a concurrent `shutdown`, which has no caller until Story 1.8 — unreachable this epic); producer initial snapshot via `get()` (benign; the `if status == last` dedup absorbs any replay); account never deactivating while only connection-subscribed (by design — deactivation is Story 1.8's `shutdown`); `map_connection_status` `Error(_)` arm untested (documented accepted residual — `State::Error(Arc<Error>)` has no public constructor).

## Design Notes

**Grounded matrix-sdk / matrix-sdk-ui 0.18.0 API (verified against vendored source):**
```rust
use matrix_sdk_ui::sync_service::{SyncService, State};

// activation: offline mode gives a real Offline state + auto-resume via /_matrix/client/versions
let sync = SyncService::builder(client.clone()).with_offline_mode().build().await?;
sync.start().await;
client.send_queue().set_enabled(true).await; // enable + respawn persisted unsent requests (force-quit resilience)

// connectivity observer (both the status producer and the reconnect supervisor call this independently;
// `state()` returns a fresh Subscriber each call):
let mut states = sync.state();                 // Subscriber<State>
// State: Idle | Running | Terminated | Error(Arc<Error>) | Offline
// map: Running => Online; else => Offline

// reconnect supervisor: on transition INTO Running, resume any room queue a recoverable error disabled:
client.send_queue().set_enabled(true).await;   // idempotent; iterates rooms + respawns
```
`send_queue().set_enabled(true)` is idempotent and internally calls `respawn_tasks_for_rooms_with_unsent_requests()`, so one call covers both "re-enable after a recoverable error" and "reload persisted queued sends." A recoverable send error *disables the room's queue* (it does not self-retry), so the reconnect supervisor's `set_enabled(true)` on return to `Running` is the load-bearing mechanism for "dispatches automatically on reconnect."

**Why "Queued" is presentation, not a Rust VM state.** `EventSendState::NotSentYet` maps to `sendState: "sending"` regardless of connectivity; the SDK does not re-emit timeline items when the `SyncService` flips offline/online. Introducing a Rust `SendState::Queued` would therefore require synthesizing timeline re-emits on every connectivity change — the exact truth-invention the architecture forbids. Composing the caption in the renderer from two authoritative streams (per-item `sendState` + global connection status) is strictly simpler and honest: it is a display rule (`offline ∧ sending ∧ own → "Queued"`) analogous to the existing `groupTail` rule, and touches no store state.

**Connection stream shape.** Scalar status, so each batch carries the full current `ConnectionStatus` (snapshot semantics — inherently idempotent, safe to re-subscribe per AD-8). Producer emits the initial snapshot immediately, then on each `state()` change, deduping consecutive-equal statuses to avoid redundant emits during internal SDK churn.

**Residual (documented, not a gap):** the live behaviors — actual `Offline` detection on network loss, auto-dispatch on reconnect, send-queue persistence across a real force-quit, and 24 h sliding-sync reconvergence — are exercised only against a real Synapse ≥ 1.114 (the epic exit gate), consistent with Stories 1.5/1.6's live-only residuals. Unit tests cover the pure seams (`map_connection_status`, VM serde, the store/hook/caption/pill rendering with a mocked IPC client). `map_connection_status`'s `Error(Arc<Error>)` arm has no test (no public `Error` constructor) — covered by reasoning, like Story 1.6's `SendingFailed` arm.

## Verification

**Commands:**
- `bun run check` -- expected: biome + tsc strict + vitest green (new `connection` store, `use-connection-status` hook, `sidebar-pane` pill, `message-bubble` queued caption, `conversation-pane` wiring).
- `bun run check:rust` -- expected: rustfmt + clippy `-D warnings` clean; core stays tauri-free (workspace guard); no `.unwrap()`.
- `bun run test:rust` -- expected: cargo-nextest green; ts-rs bindings regenerate to match committed `src/lib/ipc/gen/` (only new `ConnectionStatus`/`ConnectionStatusBatch`).
- `cd src-tauri && cargo deny check` -- expected: license firewall passes (no new crates — matrix-sdk send-queue/sync-service already dependencies).

**Manual checks (require a real Synapse ≥ 1.114 — automated tests can't exercise live connectivity):**
- `op run --env-file=.env.1p -- bun run tauri dev`: sign in → open a chat → disable network → the sidebar pill appears; send a message → amber `Queued — sends when you're back online`. Re-enable network → the pill hides, the queued message dispatches and resolves to `Sent` with no user action.
- Queue a message offline, force-quit, relaunch (still offline) → the message is still visible queued; restore network → it dispatches.
- Simulate a long offline gap → on reconnect the chat list reconverges with no duplicate/missing chats.

## Auto Run Result

Status: **done**

### Summary
Delivered offline resilience for Epic 1's send/receive slice by leaning entirely on the SDK-native persistent send queue rather than building a keeper-side outbox. Each per-account `SyncService` is now built `.with_offline_mode()`, exposing a real `Offline` state that auto-resumes via `/_matrix/client/versions`. `keeper-core` streams a new Rust-authoritative **connection-status** signal (`ConnectionStatus ∈ { Online, Offline }`, mapped from `SyncService::state()`: `Running → Online`, all else → `Offline`) over a `connection_status_subscribe` command with the AD-8 snapshot-then-deduped-diff contract. A lifetime-of-account **reconnect supervisor** observes `sync.state()` and, on every transition into `Running`, calls `client.send_queue().set_enabled(true)` — the load-bearing mechanism that resumes any room queue a recoverable error disabled (and respawns persisted unsent tasks). Activation also calls `set_enabled(true)` once so queued sends persisted by a prior process reload and dispatch (force-quit resilience). On the frontend, a persistent sidebar-footer **offline pill** and an amber **"Queued — sends when you're back online"** caption are rendered as *pure projections* of two Rust streams (per-item `sendState` + connection status) — no new `SendState` Rust variant, no connectivity read in `item_to_vm`, no timeline item invented/mutated. Reconnect convergence (24 h gap) and cross-restart queue persistence are provided by matrix-sdk (sliding-sync recovery + SQLite-backed queue); this story wires and proves the seams.

### Files changed
- `crates/keeper-core/src/vm.rs` — `ConnectionStatus` enum + `ConnectionStatusBatch` (`#[ts(export)]` camelCase) + serde round-trip tests.
- `crates/keeper-core/src/account.rs` — `.with_offline_mode()`; activation `send_queue().set_enabled(true)`; `run_reconnect_supervisor` (+ `AccountHandle.reconnect_supervisor` field, aborted on shutdown + both partial-teardown paths); `subscribe_connection_status`/`unsubscribe_connection_status`; `run_connection_producer` (snapshot-then-deduped-diff); pure `map_connection_status` + unit tests.
- `crates/keeper/src/ipc.rs` — `connection_status_subscribe`/`connection_status_unsubscribe` commands funnelling through `to_ipc_error` (activation failure → existing `SyncUnavailable`).
- `crates/keeper/src/lib.rs` — registered both commands.
- `src/lib/ipc/gen/{ConnectionStatus.ts,ConnectionStatusBatch.ts}` (NEW) — regenerated bindings; no existing binding changed.
- `src/lib/ipc/client.ts` — `subscribeConnectionStatus`/`unsubscribeConnectionStatus` wrappers + `ConnectionStatus`/`ConnectionStatusBatch` re-exports.
- `src/lib/stores/connection.ts` (NEW) + test — vanilla zustand store (default `online`), `applyBatch`/`reset`, `useConnectionStore`.
- `src/hooks/use-connection-status.ts` (NEW) + test — subscribe/reset lifecycle keyed on `accountId`, cleanup-safe.
- `src/components/layout/app-shell.tsx` — mounts `useConnectionStatus()` once.
- `src/components/layout/sidebar-pane.tsx` (+ test) — persistent footer offline pill (exact copy, `held` tokens, `role="status"`, collapsed-rail a11y hardened with `sr-only` text).
- `src/components/chat/message-bubble.tsx` (+ test) — `offline` prop → amber `Queued` caption in place of `Sending…`, gated on `offline && isOwn`.
- `src/components/layout/conversation-pane.tsx` (+ test) — wires `offline` from the connection store to bubbles.

### Review findings
- Two fresh-context reviewers (adversarial-general Blind Hunter + edge-case-hunter). Triage: **0 intent_gap, 0 bad_spec, 2 patch (both low), 0 defer, 6 reject**. See Review Triage Log.
- **Patches (both applied):** added the `isOwn` conjunct to the "Queued" caption to match the spec's three-part rule and remove a fragile SDK-invariant coupling (+ a guard test); hardened the collapsed offline pill with `sr-only` text so the `role="status"` live region is announced by content-reading screen readers too.
- **Rejected (6):** reconnect-supervisor seed edge (Blind Hunter source-verified correct + idempotent-safe); producer reap/register race and dead-account `Ok(id)` (pre-existing, copied verbatim from shipped room-list/timeline, negligible/unreachable this epic); producer `get()` snapshot (dedup absorbs); no-deactivation (Story 1.8 by design); `Error(_)` arm untested (documented residual).

### Verification
- `bun run check` ✅ — biome clean, tsc strict clean, vitest **150 passed (18 files)**, core-tauri-free guard passes.
- `bun run check:rust` ✅ — rustfmt `--check` + clippy `--all-targets -D warnings` clean.
- `bun run test:rust` ✅ — cargo-nextest **101 passed, 0 skipped**; ts-rs bindings regenerate idempotently (only the two new files, byte-matching committed output).
- `cd src-tauri && cargo deny check licenses bans sources` ✅ (`bans ok, licenses ok, sources ok`). No new crate — `Cargo.lock` byte-identical to baseline `cd8fb12`. The pre-existing OpenSSL unmatched-allowance warning and the transitive Tauri/GTK3 `unmaintained` RUSTSEC advisories are unchanged and out of scope (identical to stories 1.1–1.6).
- Not run: live connectivity behaviors against a real Synapse ≥ 1.114 (the epic exit gate) — reasoned-about and unit-tested only at the pure seams. See Manual checks.

### Residual risks
- The live path — real `Offline` detection on network loss, auto-dispatch on reconnect (`set_enabled(true)` re-enable), send-queue persistence across a real force-quit, and 24 h sliding-sync reconvergence — runs only against a real homeserver, consistent with Stories 1.5/1.6's live-only residuals.
- `map_connection_status`'s `Error(Arc<Error>)` arm has no executable test (no public constructor) — covered by reasoning, maps to `Offline`.
- The reconnect supervisor's correctness leans on two eyeball/`SyncService` internals (synchronous `Running` set at start; `get()` not marking observed); `set_enabled(true)` idempotence makes any mis-seed harmless.
- `followup_review_recommended: false` — the two review-driven changes are localized, low-consequence frontend refinements (a spec-conformance conjunct + an a11y hardening), not significant enough to warrant an independent follow-up.
