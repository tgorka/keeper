---
title: 'Undo-Send Window'
type: 'feature'
created: '2026-07-06'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: [oversized]
baseline_revision: 'd4566f491a323ef4b92de5dea8062139405d2fed'
final_revision: 'b8e6f6122d09ed2310bcdc8d349445ee845b9782'
---

<intent-contract>

## Intent

**Problem:** Every approved send leaves the machine instantly, so a regretted message can only be chased with a post-dispatch delete that other people may already have seen. Beeper paywalls the pre-dispatch buffer; keeper ships it free.

**Approach:** Insert a user-controlled hold between approval and dispatch. When the Undo-Send window (Settings, 0–60 s, default 10 s; 0 disables) is > 0, an approved send (composer or Approval Pane) is written to a persistent `outbox` table with `dispatchAtMs = approvalTime + window` instead of going to the SDK send queue. A per-account scheduler dispatches rows whose window has elapsed; cancel deletes the row with zero network activity and returns the text to that Chat's composer as a Draft. Held rows stream to the frontend as a distinct `held` state (amber bubble in the timeline tail + a floating undo-send pill with countdown).

## Boundaries & Constraints

**Always:**
- The SDK enqueue (`timeline.send(content)`) stays a **single call site**. Refactor it into `send::dispatch(timeline, text)`; both `send::submit` (window == 0 path) and the outbox scheduler (elapsed-hold completion) route through `dispatch`. The AD-13 guard test is updated to assert exactly one `.send(content)` site (inside `dispatch`) and that `send::dispatch(` has exactly one non-`submit` caller (the scheduler). `SendTrigger` stays a closed 2-variant set; `send::submit`'s two user callers (`send_text`, `send_approval`) are unchanged in count. (epics.md: "AD-13 … completed 7.4/8.3".)
- Holding is durable: `outbox` lives in `keeper.db` (WAL). After crash/restart the scheduler dispatches rows already past `dispatchAtMs` and lets unelapsed rows finish their countdown; the frontend resumes the countdown from `dispatchAtMs` when the Chat is opened. **No held message may be silently lost** — dispatch-then-delete (accept a rare mid-crash duplicate over any loss).
- Cancel (`Undo` click or `⌘⇧Z` while a held send exists in the focused Chat) deletes the row, performs **zero** network dispatch, persists the body as that Chat's Draft (`registry::set_draft`) and restores it into the composer. Cancel of an already-dispatched/absent row is an idempotent no-op.
- Window == 0 preserves today's behavior exactly: `send_text` / `send_approval` call `send::submit` immediately, no `outbox` row.
- The undo-send window is read from the `settings` table (key `undo_send.window`, default 10, clamped 0..=60) on each send and at scheduler start.
- New VMs (`HeldSendVm`, `OutboxVm`) follow spine conventions: camelCase serde, `Vm` suffix, ms-epoch timestamps, ts-rs binding into `src/lib/ipc/gen/`, streamed snapshot into the `outbox` zustand mirror store. Held surfaces use the existing amber `--held` / `text-held` token; the pill's radial ring degrades to a numeric-only countdown under `motion-reduce` and announces its countdown to VoiceOver once (not per second).

**Block If:**
- The installed matrix-sdk forces `timeline.send` to be called from more than one site to complete a held dispatch (i.e. the single-`.send(content)`-call-site AD-13 guarantee cannot be preserved through the refactor). HALT — do not add a second SDK enqueue site.

**Never:**
- Do not add a third `SendTrigger` variant or a background/bulk send path; the scheduler completing an already-approved hold is not a new dispatch decision and must not mint a new trigger.
- Do not inject held rows into the SDK timeline stream or add a `Held` variant to the SDK-mapped `SendState`; held bubbles are rendered from the `outbox` VM at the timeline tail, distinct from SDK local echoes.
- Do not implement post-dispatch delete / Redaction (Story 8.4) or register `⌘⇧Z` into a global command/cheat-sheet registry (Epic 9) — the `⌘⇧Z` handler here is a local keydown scoped to the open Chat with a pending hold.
- Do not hold outbox/held state in a JS store as source of truth; the store mirrors the Rust `outbox` stream only.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Approve, window > 0 | window = 10, ComposerSend/ApprovalPaneApprove | `outbox` row inserted (`dispatchAtMs = now + 10_000`); held amber bubble at timeline tail + undo-send pill with countdown; zero network | insert failure → surfaced as a send error (not silently dropped) |
| Approve, window = 0 | window = 0 | immediate `send::submit` → `dispatch`; no `outbox` row (unchanged) | best-effort as today |
| Undo before elapse | Undo click or `⌘⇧Z`, row present | row deleted, zero network (verifiable at homeserver), body restored to composer + persisted as Draft | already-dispatched/missing row → idempotent no-op |
| Window elapses | `dispatchAtMs <= now` | scheduler dispatches oldest-first via `send::dispatch`, deletes row; SDK echo takes over (Sending → Sent) | dispatch error logged + swallowed (best-effort); row removed after SDK handoff |
| Crash/restart, elapsed row | restart, `dispatchAtMs` in past | scheduler dispatches on startup | — |
| Crash/restart, unelapsed row | restart, `dispatchAtMs` in future | scheduler waits; countdown resumes from `dispatchAtMs` on Chat open | — |
| Offline at elapse | offline when window passes | scheduler dispatches → SDK send queue persists → sends on reconnect | SDK owns offline durability (Story 1.7) |
| Multiple holds, one Chat | 3 sends held | 3 pills stack oldest-first; 3 held bubbles; dispatched oldest-first | — |
| Setting out of range | user enters 99 / -5 | clamped to 0..=60 before persist | — |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/registry.rs` -- `open()` creates tables (WAL). Add `outbox` table (mirror `drafts` DDL) + CRUD (`insert_outbox`, `delete_outbox(id)`, `list_outbox_rows_for_account`, `list_outbox_rows`); add `get_undo_send_window`/`set_undo_send_window` (settings key `undo_send.window`, default 10, clamp 0..=60) mirroring `get_incognito_global`/`set_incognito_global`.
- `src-tauri/crates/keeper-core/src/send.rs` -- factor the sole `timeline.send(content)` into `pub(crate) async fn dispatch(timeline, text)`; `submit` calls `dispatch`. Update the three AD-13 guard tests (`send.rs:~388-643`): `.send(content)` == 1 inside `dispatch`; `send::submit(` callers == 2; `send::dispatch(` non-submit callers == 1 (scheduler). Other verb gates (reply/edit/reaction/redact/attachment) untouched.
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `HeldSendVm { id, accountId, roomId, body, heldAtMs, dispatchAtMs }` and `OutboxVm { rows: Vec<HeldSendVm> }` (ts-rs export).
- `src-tauri/crates/keeper-core/src/account.rs` -- `send_text` (~2100) & `send_approval` (~2192): read window; > 0 → insert `outbox` row (`id = TransactionId::new()`, `dispatchAtMs = now_ms() + window*1000`), publish outbox snapshot; == 0 → existing `send::submit`. Add `cancel_held_send(account_id, room_id, id) -> Result<String, CoreError>` (delete row, `set_draft`, publish, return body). Add outbox broadcast + `subscribe_outbox(account_id, room_id, channel)` producer (snapshot-then-broadcast, filtered to room, mirror timeline subscription lifecycle). Add `run_outbox_scheduler(client, account_id, platform, tx)` spawned in `activate()` (~3710, tokio interval ~250 ms; dispatch elapsed rows for this account via reuse-or-transient timeline like `send_approval` → `send::dispatch`; delete + publish). Store its `JoinHandle` in `AccountHandle` (~218) and abort on shutdown.
- `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- commands `undo_send_window() -> u16`, `set_undo_send_window(seconds)`, `cancel_held_send(account_id, room_id, id) -> String`, `subscribe_outbox`/`unsubscribe_outbox`; register in `generate_handler!`.
- `src/lib/ipc/client.ts` -- wrappers `undoSendWindow`, `setUndoSendWindow`, `cancelHeldSend`, `subscribeOutbox`/`unsubscribeOutbox`; re-export `HeldSendVm`/`OutboxVm`.
- `src/lib/stores/outbox.ts` (new) -- zustand mirror of held rows keyed by `(accountId, roomId)`; selector `useHeldSends(accountId, roomId)`.
- `src/components/chat/undo-send-pill.tsx` (new) -- floating pill above composer: radial SVG countdown from `dispatchAtMs` (numeric-only under `motion-reduce`), aria-live announce-once, stack oldest-first; click / `⌘⇧Z` → `cancelHeldSend` then restore composer.
- `src/components/chat/message-bubble.tsx` + `src/components/layout/conversation-pane.tsx` -- subscribe to `subscribeOutbox` for the open Chat; append held rows to `toRenderedRows` at the tail with an amber `text-held` "Held" caption; render the pill; on cancel restore body via composer `setDraft` + `scheduleDraftSave`.
- `src/components/settings/settings-dialog.tsx` -- add the Undo-Send window control (range/number 0–60, default 10) to `PrivacySection` using the load-on-open + optimistic-write-with-revert pattern.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- add `outbox` table + CRUD and the `undo_send.window` settings helpers (default 10, clamp 0..=60); unit-test insert/list-for-account/delete round-trip and window default+clamp.
- [x] `src-tauri/crates/keeper-core/src/send.rs` -- extract `dispatch`; route `submit` through it; update the AD-13 guard tests to the new topology (one `.send(content)` site; scheduler as the sole non-submit `dispatch` caller).
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `HeldSendVm` + `OutboxVm` (ts-rs export).
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- hold-vs-submit branch in `send_text`/`send_approval`; `cancel_held_send`; outbox broadcast + `subscribe_outbox` producer; `run_outbox_scheduler` supervised task (spawn in `activate`, abort on shutdown). Tests: window>0 inserts a row & does not dispatch; window==0 dispatches with no row; elapsed row dispatches once then is deleted; cancel deletes row + writes draft; unelapsed row survives a simulated restart read.
- [x] `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- add & register the four commands via the `IpcError` path.
- [x] `src/lib/ipc/client.ts` -- add the wrappers and VM re-exports.
- [x] `src/lib/stores/outbox.ts` (+ test) -- held-rows mirror store keyed by `(accountId, roomId)`.
- [x] `src/components/chat/undo-send-pill.tsx` (+ test) -- countdown pill; numeric under reduced motion; single announce; click cancels and restores.
- [x] `src/components/chat/message-bubble.tsx` + `src/components/layout/conversation-pane.tsx` (+ tests) -- outbox subscription; held bubble at timeline tail (amber); pill mount; cancel → composer restore.
- [x] `src/components/settings/settings-dialog.tsx` (+ test) -- Undo-Send window control (0–60, default 10, 0 disables) with optimistic persist.

**Acceptance Criteria:**
- Given an approved send (composer or Approval Pane) with window > 0, when dispatch is requested, then an `outbox` row is inserted with `dispatchAtMs = approvalTime + window`, a distinct amber held bubble renders at the timeline tail, and the undo-send pill floats above the composer with a countdown (numeric-only under reduced motion; multiple pending sends stack oldest-first), with zero network activity until the window elapses (FR-46, AD-13, UX-DR6).
- Given the countdown running, when the user clicks Undo or presses `⌘⇧Z`, then the `outbox` row is deleted with zero network dispatch and the full text returns to that Chat's composer as a Draft (FR-46).
- Given the window elapses (including after a crash while offline), then the scheduler dispatches the row through the single `send::dispatch` gate into the account's send queue and normal send states take over; elapsed rows dispatch on startup/reconnect and unelapsed rows resume their countdown, with no held message lost (FR-46, NFR-8).
- Given Settings → Privacy, when the user sets the Undo-Send window (0–60 s; 0 disables holding), then subsequent sends honor it, and the AD-13 sole-gate tests still prove exactly one SDK enqueue site (AD-13).
- Given the whole change, then `bun run check`, `bun run check:rust`, and `bun run test:rust` are green.

## Design Notes

AD-13 evolution: the airlock invariant is "no message reaches the SDK send queue except by a legal path." Before 8.3 that path was `send::submit` (two user triggers). 8.3 keeps `send::submit` as the *immediate* user path but adds a *deferred* completion: a durable `outbox` row, written under one of the same two triggers, that the scheduler later completes. Both funnel through the single `send::dispatch` primitive, so the "one `.send(content)` call site" guarantee is preserved and the guard test is tightened (not weakened) to pin the scheduler as the only non-`submit` caller.

Held rows are deliberately *not* SDK timeline items — they have not been sent. Rendering them from a dedicated `outbox` VM at the tail keeps the SDK `EventSendState → SendState` mapping honest and avoids identity-matching a synthetic bubble to a later real echo. Brief visual: when a hold dispatches, its held bubble disappears (row deleted) and the SDK "Sending…" echo appears — an accepted, momentary hand-off rather than an in-place mutation.

Loss-vs-duplicate: the scheduler dispatches (hands to the SDK queue, which persists) then deletes the `outbox` row. A crash in the microsecond gap re-dispatches on restart (rare duplicate) rather than dropping the message — the epic's "no held message may be silently lost" makes at-least-once the correct choice.

Cancel restores the held body into the composer, replacing current composer content and persisting it as the room Draft. The common flow is an empty composer just after send; the rare "typed a new message during the window then undid" case yields to the restored text — a documented trade-off, not data corruption (the restored text is the user's own most recent intent).

## Verification

**Commands:**
- `bun run check:rust` -- rustfmt clean + clippy `-D warnings` (no `.unwrap()`/`.expect()` in new paths).
- `bun run test:rust` -- outbox CRUD + window default/clamp, hold-vs-submit branch, elapsed-dispatch-once, cancel-writes-draft, restart-read, and the updated AD-13 sole-gate tests all pass.
- `bun run check` -- biome + tsc + vitest green, including the outbox store, undo-send pill (countdown + reduced-motion + single-announce + cancel-restore), held-bubble rendering, and the Settings window control.

## Review Triage Log

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 6: (high 1, medium 4, low 1)
- defer: 1
- reject: 12
- addressed_findings:
  - `[high]` `[patch]` `send_approval` ran the Undo-Send window branch **before** `RoomId::parse`, so a malformed room id with a positive window was inserted into the `outbox` and then silently dropped by the scheduler's unparsable-id guard — an approved, held message lost, violating "never lose a held message". Moved the parse ahead of the window branch (symmetric with `send_text`, which was already safe); added a regression test (`send_approval_with_malformed_room_id_errors_and_holds_nothing`) (`src-tauri/crates/keeper-core/src/account.rs`).
  - `[medium]` `[patch]` The scheduler re-dispatched a row every 250 ms whenever the post-dispatch `delete_outbox` transiently failed (e.g. `SQLITE_BUSY` under WAL contention), turning the accepted "rare crash duplicate" into a duplicate storm. Added an in-memory `awaiting_delete` set (a dispatched-but-undeleted row retries only its delete, never re-dispatches) plus `MissedTickBehavior::Delay`, bounding duplicates to a genuine crash/abort while preserving at-least-once (`src-tauri/crates/keeper-core/src/account.rs`).
  - `[medium]` `[patch]` The Undo-Send composer restore was global, so a `cancelHeldSend` resolving after the user switched Chats could inject the held body into (and persist it as the draft of) the wrong room. Scoped `restore` to `(accountId, roomId)`; the composer applies it only when the target matches its own chat (`src/lib/stores/composer.ts`, `src/components/chat/composer.tsx`, `src/components/chat/undo-send-pill.tsx`; test asserts the scoped target).
  - `[medium]` `[patch]` The `⌘⇧Z` handler was a bare `window` keydown that fired even while the Settings dialog (a portal modal that leaves the pill mounted) held focus, silently cancelling the oldest held send. It now ignores the keystroke when a `[role="dialog"]` modal owns focus (`src/components/chat/undo-send-pill.tsx`).
  - `[medium]` `[patch]` `cancel_held_send` deleted the row and then returned `Err` if the draft write failed, losing the body with no restore (the frontend maps a rejection to `""`). Made `set_draft` best-effort so the body is always returned for live restore even if its durable persistence fails (`src-tauri/crates/keeper-core/src/account.rs`).
  - `[low]` `[patch]` A Settings state variable named `window` shadowed the browser global in a component that (via sibling Story 8.3 code) legitimately uses `window.*`; renamed to `undoWindow` (`src/components/settings/settings-dialog.tsx`).
- Notable rejects (verified not real / accepted design / out of scope): the `roomId={selectedRoomId}` "type error / null prop" is a false positive — the render sits inside a `selectedRoomId !== null` block so it is narrowed to `string` (`tsc` passes); a hold written while an account is not yet activated is **not** lost — the scheduler dispatches it on the next activation/startup tick; the dispatch-then-delete at-least-once trade-off and its abort-mid-tick duplicate are the epic's chosen never-lose semantics (an in-flight set cannot survive a process restart by design); the missing `outbox(account_id)` index and the countdown-ring clock-skew cosmetics are negligible for a tiny, seconds-lived table; a failed cancel leaves the pill visible (snapshot-driven), so it is not mistaken for a successful undo.

### 2026-07-06 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 2
- reject: 17
- addressed_findings:
  - `[low]` `[patch]` `cancel_held_send` persisted the restored Draft into the caller-supplied `room_id` parameter rather than the held row's own stored room. In practice the frontend always cancels from the row's open Chat so they match, but the row already carries the canonical room recorded at hold time; switched the `set_draft` (and its tracing) to `row.room_id` and added a mismatch-warning guard so a divergent caller can never restore a held body into the wrong room (`src-tauri/crates/keeper-core/src/account.rs`).
- Notable rejects (verified not real / documented trade-off / already tracked): the in-memory `awaiting_delete` set not surviving a restart is the spec's chosen at-least-once semantics ("no held message may be silently lost" → accept a rare crash-window duplicate), already in Residual risks; the composer-restore replacing typed content and the held-bubble→SDK-echo hand-off flicker are documented Design-Notes trade-offs; the parseable-but-unresolvable-room retry (and its "Sending in 0s" pill) is already in the deferred-work ledger; scheduler-bypasses-`submit`-trigger-logging is by design (the scheduler mints no `SendTrigger`); the cancel full-account scan / `notify_outbox` non-coalescing / double-clamp display are negligible for a tiny seconds-lived table; the `⌘⇧Z`-only-guards-`role="dialog"` and settings-field-cannot-be-blanked items are the prior pass's deliberate scoping / standard controlled-input behavior.

## Auto Run Result

Status: done

### Summary
Story 8.3 adds the Undo-Send Window: an approved send (composer or Approval Pane) with a positive window (Settings → Privacy, 0–60 s, default 10, 0 disables) is held in a durable WAL `outbox` table with `dispatchAtMs = approvalTime + window` instead of dispatching immediately. A per-account 250 ms scheduler dispatches elapsed rows through the single `send::dispatch` gate (dispatch-then-delete, at-least-once so no held message is lost); cancel (Undo click or `⌘⇧Z`) deletes the row with zero network activity and returns the body to the Chat's composer as a Draft. Held rows stream as a dedicated `outbox` VM — an amber "Held" bubble at the timeline tail plus a floating countdown pill (radial ring, numeric-only under reduced motion, announced once) — never injected into the SDK timeline. AD-13 is preserved and tightened: the sole `.send(content)` site is factored into `send::dispatch`, with `send::submit` keeping exactly two user callers and the scheduler the only other `dispatch` caller, pinned by the updated guard tests.

### Files changed
- `src-tauri/crates/keeper-core/src/registry.rs` -- `outbox` table (WAL) + CRUD; `undo_send.window` settings helpers (default 10, clamp 0..=60); outbox cleanup on account delete; tests.
- `src-tauri/crates/keeper-core/src/send.rs` -- extracted `pub(crate) dispatch` as the sole SDK-enqueue site; `submit` delegates to it; AD-13 guard tests tightened.
- `src-tauri/crates/keeper-core/src/vm.rs` -- `HeldSendVm` + `OutboxVm` (ts-rs export).
- `src-tauri/crates/keeper-core/src/account.rs` -- hold-vs-submit branch in `send_text`/`send_approval` (parse-before-hold); `hold_send`; `cancel_held_send` (best-effort draft); outbox broadcast + `subscribe_outbox`/`unsubscribe_outbox`; `run_outbox_scheduler` (in-flight `awaiting_delete` guard, `MissedTickBehavior::Delay`) supervised in `activate`, aborted on shutdown; tests incl. the malformed-room-id regression.
- `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- `undo_send_window`, `set_undo_send_window`, `cancel_held_send`, `subscribe_outbox`, `unsubscribe_outbox` commands, registered.
- `src/lib/ipc/client.ts` (+ `gen/HeldSendVm.ts`, `gen/OutboxVm.ts`) -- wrappers + VM re-exports/bindings.
- `src/lib/stores/outbox.ts` (+ test) -- held-rows mirror keyed by `(accountId, roomId)`.
- `src/lib/stores/composer.ts` + `src/components/chat/composer.tsx` -- room-scoped `restore` for cancel→composer restore.
- `src/components/chat/undo-send-pill.tsx` (+ test) -- countdown pills, reduced-motion + announce-once, `⌘⇧Z` (modal-scoped) cancel + restore.
- `src/components/layout/conversation-pane.tsx` (+ test) -- outbox subscription, amber held bubbles at the tail, pill mount.
- `src/components/settings/settings-dialog.tsx` (+ test) -- Undo-Send window control (0–60, default 10) with optimistic persist.

### Review findings breakdown
- Patches applied: 6 (1 high — `send_approval` parse-before-hold data loss; 4 medium — scheduler duplicate-storm guard, room-scoped restore, `⌘⇧Z` modal scoping, cancel best-effort draft; 1 low — Settings `window` shadow rename).
- Deferred: 1 (held row for a parseable-but-unresolvable room retries every tick with no age bound).
- Rejected: 12 (incl. the false-positive null-`roomId` "type error" — the render is inside a `selectedRoomId !== null` narrowing block; not-lost hold on a non-activated account; accepted at-least-once trade-off; negligible index/clock-skew cosmetics).

### Follow-up review pass (2026-07-06)
An independent follow-up review (Blind Hunter + Edge Case Hunter) re-swept the full diff.
- Patches applied: 1 low — `cancel_held_send` now persists the restored Draft into the held row's own canonical room (`row.room_id`) instead of the caller-supplied `room_id`, with a mismatch-warning guard (defensive; no user-facing behavior change since the frontend always cancels from the row's open Chat).
- Deferred: 2 NEW ledger entries — (a) no automated test drives the scheduler's elapsed-dispatch / delete-retry path (needs a mock-Matrix sync harness the crate lacks); (b) the scheduler's ~250 ms idle polling and per-send fresh `registry::open()` cost (wake-on-hold gate + cached window would remove it).
- Rejected: 17 — documented trade-offs (at-least-once crash-window duplicate, composer-restore replacement, held→echo hand-off flicker), the already-tracked unresolvable-room retry, by-design scheduler-no-trigger telemetry, and negligible perf/cosmetic nitpicks on a tiny seconds-lived table.
- `followup_review_recommended` set to `false`: this pass produced only one localized, low-consequence defensive fix.
- Re-verification after the patch: `bun run check:rust` PASS; `bun run test:rust` PASS (643/643); `bun run check` PASS (biome clean, tsc clean, vitest 791/791).

### Verification
- `bun run check:rust` -- PASS (rustfmt clean + clippy `-D warnings`).
- `bun run test:rust` -- PASS (643 tests, 0 skipped; AD-13 sole-gate guards + new outbox/hold/cancel/malformed-room tests green).
- `bun run check` -- PASS (biome clean, tsc clean, vitest 791 passed).
- AD-13 counts re-verified in production `account.rs`: `send::submit(` = 2, `send::dispatch(` = 1; single `.send(content)` site inside `dispatch`.

### Residual risks
- At-least-once dispatch: a crash/abort in the microsecond gap between SDK hand-off and outbox-row delete re-dispatches on restart (a rare duplicate) — the deliberate choice over ever losing a held message; the in-memory `awaiting_delete` guard bounds within-process duplicates but cannot survive a process restart.
- Cancel restores the held body into the composer, replacing current composer content; the rare "typed a new message during the window then undid" case yields to the restored text (documented trade-off).
- A held row for a room that parses but never resolves on the live Client retries indefinitely (deferred — see deferred-work.md); the held pill/bubble lingers until the room resolves.
- Follow-up review recommended: the review pass applied a high-severity data-loss fix plus concurrency/data-path changes across Rust and TS that would benefit from an independent look.
