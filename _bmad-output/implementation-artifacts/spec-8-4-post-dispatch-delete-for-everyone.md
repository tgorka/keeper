---
title: 'Post-Dispatch Delete for Everyone'
type: 'feature'
created: '2026-07-06'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: [oversized]
baseline_revision: '646cc9a4731dc8b6a7ecdc2ae6b570f1b5fd4278'
final_revision: 'f25c2c3eff1d0ac2831072e1a6aa078a1e0a3c64'
---

<intent-contract>

## Intent

**Problem:** keeper already has post-dispatch delete-for-everyone (Story 3.8: `send::redact`, the delete confirmation that names the bridged Network and states removal is best-effort, the honest "Message deleted" stub) and the Undo-Send hold (Story 8.3: `outbox` rows, `cancel_held_send`, the undo-send pill). But the two are unaware of each other. A message still inside its undo window has **no remote event to redact** — yet the timeline's Delete affordance (`onDelete` button, `⌫`/Delete key) resolves only against SDK timeline items, so a held bubble either silently no-ops or, worse, could be steered toward a redaction that cannot succeed. The user's "delete this" intent on a held message must resolve as an **undo**, not a Redaction.

**Approach:** Make the timeline Delete affordance held-aware. A held bubble becomes Delete-targetable (a Delete action on the bubble, and `⌫` when it is the selected row); that intent routes to the exact same effect as the undo-send pill's Undo — `cancelHeldSend` + restore the body to the composer as a Draft, with zero network activity and no redaction dialog. Both delete entry points branch on held-row identity **first**, so a held id can never reach `deleteMessage`/`send::redact`. A message that has actually dispatched (window elapsed or window = 0) keeps the existing Story 3.8 redaction path unchanged. On the Rust side no redaction or archive plumbing is added — an own post-dispatch redaction already flows through the source-agnostic sync redaction handler into the archive's mark-never-erase path — but a regression test pins that FR-36 contract for an own deletion.

## Boundaries & Constraints

**Always:**
- A Delete/`⌫` intent on a **held** (still-in-window) message resolves as an undo: `cancelHeldSend(accountId, roomId, id)` then, on a non-empty returned body, `composerStore.restore(...)` — identical to the undo-send pill. Zero network dispatch; no redaction dialog is opened.
- A Delete/`⌫` intent on a **dispatched own** message keeps the Story 3.8 behavior verbatim: open `DeleteMessageDialog` → `deleteMessage` → `send::redact` (bridged Chats name the Network and state removal is best-effort).
- Held-row identity is the sole discriminator and is collision-free: a held render key is `held:<id>`; SDK timeline keys are opaque `unique_id`s that never carry that prefix. The branch strips the `held:` prefix and matches against the live `heldSends` snapshot.
- The undo effect is shared code with the undo-send pill (one helper), so pill-undo and delete-undo cannot drift.
- Redaction dispatch still goes through exactly one SDK gate (`send::redact`, AD-13); this story adds no new dispatch or redaction path in Rust.
- Copy follows UX-DR10/UX-DR17 voice already established in `DeleteMessageDialog`; no new user-facing strings are required for the undo route (undo silently restores the Draft, as in Story 8.3).

**Block If:**
- The `held:` render-key convention or `cancelHeldSend`/`useHeldSends`/`composerStore.restore` contracts from Story 8.3 are absent or changed such that a held row cannot be identified at the delete seam.

**Never:**
- No new redaction, cancel, or archive IPC command; no change to `send::redact`, `redact_message`, `room_network_label`, `cancel_held_send`, `hold_send`, or the outbox scheduler.
- No new archive plumbing and no origin/`is_own` branch in the archive redaction handler — own and remote redactions stay source-agnostic (FR-36); this story only adds a regression test asserting that contract for an own deletion.
- No confirmation dialog for the held-undo route — an undo is reversible (returns text to the composer) and matches Story 8.3's dialog-free cancel.
- No change to the dispatched-message redaction UX, the bridged best-effort framing, or the received-redaction stub (all delivered by Story 3.8).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Delete a held message | Held bubble Delete button, or `⌫` on the selected held bubble | `cancelHeldSend` deletes the `outbox` row (zero network), body returns to the composer as a Draft; held bubble + pill vanish. No redaction dialog. | `cancelHeldSend` throws → swallow, no restore (mirrors pill) |
| Window elapses before undo lands | User fires held-delete but the scheduler already dispatched the row | `cancelHeldSend` finds no row → returns empty body → no-op; the now-dispatched message follows the normal sent path (Story 8.3 at-least-once) | none — honest, message left before undo |
| Delete a dispatched own message | Own message, `sendState === null`, Delete/`⌫` | Existing Story 3.8 path: `DeleteMessageDialog` → `deleteMessage` → redaction; bridged Chats name the Network + best-effort | dialog surfaces honest error + retry (3.8) |
| `⌫` on others' / unsent / no selection | Selected item not own, or an unsent echo, or nothing selected | No-op; `⌫` keeps its default (unchanged from 3.8) | — |
| Own redaction reaches the archive | User post-dispatch-deletes; the server echoes the redaction back via sync | Sync redaction handler marks the archive row (`redacted_ts` set, `content_json` retained); `retrievable_content` gates on the "honor remote deletions locally" setting | — |

</intent-contract>

## Code Map

- `src/lib/stores/outbox.ts` (or a small shared module) -- extract the undo-send pill's cancel-and-restore body into one reusable async helper `undoHeldSend(accountId, roomId, id)` (`cancelHeldSend` then `composerStore.restore` on non-empty body). Single source of truth for the undo effect.
- `src/components/chat/undo-send-pill.tsx` -- replace the inline `undo` body (`undo-send-pill.tsx:~53-63`) with a call to the shared `undoHeldSend` helper (behavior identical; no visual change).
- `src/components/layout/conversation-pane.tsx` -- (1) branch `onDelete` (`~1066-1077`) and the `⌫`/Delete keydown handler (`~1313-1330`): if the key/selection is a held row (`held:` prefix → matched in `heldSends`), call `undoHeldSend` and return **before** any redaction path; otherwise the existing dispatched-message dialog path. (2) Render held bubbles (`~1473-1482`) with a Delete affordance and make them selectable so `⌫` can target them (selected key = `held:<id>`), reusing the same amber styling. The dispatched-message redaction dialog can never receive a held id.
- `src-tauri/crates/keeper-core/tests/archive_durability.rs` (existing `redaction_marks_and_retrievable_content_honors_policy`) -- add/extend a test asserting an **own-initiated** redaction, delivered through the sync redaction handler, marks the archive row (`redacted_ts` set, `content_json` retained) and that `retrievable_content` honors the "honor remote deletions locally" setting — pinning FR-36 for own deletions. No production Rust change.

## Tasks & Acceptance

**Execution:**
- [x] `src/lib/stores/outbox.ts` -- add the shared `undoHeldSend(accountId, roomId, id)` helper (cancel + restore-on-non-empty). Unit-test: calls `cancelHeldSend`, restores composer on a non-empty body, no restore on empty/throw.
- [x] `src/components/chat/undo-send-pill.tsx` -- delegate the pill's undo to the shared helper; existing pill test stays green.
- [x] `src/components/layout/conversation-pane.tsx` -- held-aware Delete: branch `onDelete` and the `⌫` handler on held identity (held → `undoHeldSend`, dispatched own → existing dialog); render a Delete affordance on held bubbles + make them selectable for `⌫`. Tests: Delete/`⌫` on a held bubble calls `undoHeldSend` and never opens `DeleteMessageDialog`; Delete/`⌫` on a dispatched own message still opens the dialog; a held id is never passed to `deleteMessage`.
- [x] `src-tauri/crates/keeper-core/tests/archive_durability.rs` -- regression test: an own post-dispatch redaction marks the archive row (content retained) and `retrievable_content` honors the setting (FR-36), documenting that own deletions follow the source-agnostic path.

**Acceptance Criteria:**
- Given a message already dispatched (window elapsed or window = 0), when the user deletes it for everyone, then keeper issues a Matrix Redaction and — in bridged Chats — the confirmation names the Network and states removal there is best-effort (FR-47, unchanged from Story 3.8).
- Given a message still in its undo window, when the user expresses the delete intent (held-bubble Delete or `⌫` on the selected held bubble), then it resolves as an undo (Story 8.3): the `outbox` row is deleted with zero network activity, the text returns to the composer as a Draft, and no Redaction is attempted and no redaction dialog opens.
- Given the user's own post-dispatch deletion, when the archive processes the redaction (delivered via sync), then the Local Archive marks the row and keeps priors unless "honor remote deletions locally" is on (FR-36), via the existing source-agnostic handler.
- Given the whole change, then `bun run check`, `bun run check:rust`, and `bun run test:rust` are green, and the AD-13 single-gate redaction guard still proves exactly one `.redact(` call site.

## Design Notes

**Why the held-delete is an undo, not a redaction.** A redaction targets a Matrix event that exists on the server. A held message has never left the machine — there is no `event_id` to redact — so the honest resolution of "delete this" is to pull it back, which is exactly Story 8.3's cancel: drop the `outbox` row and hand the text back to the composer as a Draft. Routing it anywhere near `send::redact` would either no-op confusingly or error. The seam is deterministic because held render keys are `held:<id>` and SDK keys never are, so the branch is a prefix check plus a lookup in the live `heldSends` snapshot — no ambiguity, no collision.

**Shared undo effect.** The pill and the timeline Delete must produce byte-identical behavior, so both call one `undoHeldSend` helper. This prevents the two affordances from drifting (e.g. one restoring the Draft and the other not) and gives a single tested unit.

**Archive AC is satisfied by existing code.** An own post-dispatch redaction is dispatched via `send::redact`, echoed back by the server in the next sync, and delivered to the same `register_redaction_handler` that handles remote redactions — which calls `archive.redact(...)` → `mark_redacted` (sets `redacted_ts`, never erases `content_json`). `retrievable_content(..., honor)` gates read-time visibility on the "honor remote deletions locally" setting. The handler carries no origin metadata, so own and remote are treated identically per FR-36. This story therefore adds a regression test rather than new plumbing.

## Verification

**Commands:**
- `bun run check` -- biome + tsc + vitest green, including the `undoHeldSend` helper test, the held-aware `onDelete`/`⌫` routing tests (held → undo, dispatched → dialog), and the "held id never reaches `deleteMessage`" assertion.
- `bun run check:rust` -- rustfmt clean + clippy `-D warnings` (no `.unwrap()`/`.expect()` in any new test helper paths).
- `bun run test:rust` -- cargo-nextest green, including the own-redaction archive-mark + honor-gate regression test and the unchanged AD-13 single-`.redact(`-gate guard.

**Manual checks (real second session, test credentials in 1Password):**
- With Undo-Send window > 0, send a message; while the held bubble/pill is counting down, use the held-bubble Delete (and `⌫`): the message never leaves, its text returns to the composer, and no delete confirmation dialog appears.
- Let a message dispatch (or set window = 0), then Delete it: the Story 3.8 confirmation appears (naming the Network + best-effort on a bridged Chat) and a receiving client shows the redaction stub.

## Review Triage Log

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 0, low 3)
- defer: 0
- reject: 7: (high 0, medium 0, low 7)
- addressed_findings:
  - `[low]` `[patch]` A `held:` selection whose window elapsed mid-action fell through `tryUndoHeld` (row gone from the snapshot) toward the redaction branch and, on `⌫`, left `e.preventDefault()` uncalled (native key could act). Hardened `tryUndoHeld` so **any** `held:`-prefixed key is always consumed (returns `true`); the undo fires only when the row is live and room context is resolved, otherwise a clean no-op. Makes the "a held id never reaches `deleteMessage`" invariant explicit instead of relying on a double-miss.
  - `[low]` `[patch]` Added two coverage tests for the load-bearing branch ordering: a dispatched own message still opens the redaction dialog while a held row coexists, and a stale `held:` selection is consumed with no cancel and no redaction.
  - `[low]` `[patch]` The held bubble rendered a `<p>` (flow content) inside a `<button>` (invalid HTML) and its Delete affordance shared the bare "Delete" label with the destructive redaction Delete. Changed the body to a block `<span>` and gave the held Delete button an honest `aria-label` ("Discard held message and return it to the composer") distinguishing the non-destructive undo from a for-everyone redaction.
- rejected_notable: sent-and-restored duplicate from a Rust race (relies on violating Story 8.3's tested `cancel_held_send` idempotency — not this diff); composer overwrite on restore (Story 8.3's documented, accepted trade-off, reused verbatim); failed-cancel silence and the `void`-returning pill callback (identical to the pill's existing 8.3 behavior; gates green); keyboard arrow-nav cannot select a held bubble (mitigated — the undo-send pill `⌘⇧Z`/focusable Undo and the Tab-focusable held Delete button both give keyboard undo; only the redundant arrow path is unreached); `r`/`e` dead keystroke on a held selection (benign no-op — reply/edit on an unsent message is meaningless); the Rust test covering redaction parity rather than "held emits no redaction" (correct distribution — the held-routing invariant is a frontend decision with no new Rust code, covered by the frontend tests).

## Auto Run Result

Status: done

### Summary
Made keeper's post-dispatch delete honest about the Undo-Send window (Story 8.4). The Matrix Redaction path, bridged-Network best-effort framing (Story 3.8), the `outbox`/`cancel_held_send` machinery (Story 8.3), and the source-agnostic archive redaction-mark honoring the "honor remote deletions locally" setting (Epic 5) already existed. The new work: a Delete/`⌫` on a message still inside its undo window now resolves as an **undo** (`undoHeldSend` → `cancelHeldSend` + composer restore, zero network) instead of a Redaction, via a branch-first `tryUndoHeld` guard shared with the undo-send pill so the two affordances cannot drift. Held bubbles became Delete-targetable (a Delete button + selectable for `⌫`). A dispatched message keeps the Story 3.8 redaction dialog unchanged. A Rust regression test pins that an own post-dispatch redaction marks (never erases) the archive row and honors the setting (FR-36). No production Rust change.

### Files changed
- `src/lib/stores/outbox.ts` -- new shared `undoHeldSend(accountId, roomId, id)` helper (cancel + restore-on-non-empty), the single source of truth behind both the pill's Undo and the held-bubble Delete.
- `src/components/chat/undo-send-pill.tsx` -- the pill's Undo delegates to `undoHeldSend` (no behavior change).
- `src/components/layout/conversation-pane.tsx` -- `tryUndoHeld` guard branches `onDelete` and the `⌫` handler on held identity (held → undo, dispatched own → existing dialog); a `held:` key is always consumed so it can never reach `deleteMessage`; held bubbles gained a Delete affordance, are selectable, use valid phrasing content, and carry an honest `aria-label`.
- `src/lib/stores/outbox.test.ts` -- `undoHeldSend` unit tests (restore on non-empty, no restore on empty, swallow rejection).
- `src/components/layout/conversation-pane.test.tsx` -- held-aware routing tests: held Delete/`⌫` → undo and never the dialog; dispatched own → dialog (even with a held row present); stale `held:` selection consumed inertly.
- `src-tauri/crates/keeper-core/tests/archive_durability.rs` -- `own_post_dispatch_redaction_marks_archive_and_honors_policy` regression test (test-only, no production Rust change).

### Review findings breakdown
- Patches applied: 3 (all low) — invariant-hardening `tryUndoHeld` guard, branch-ordering + stale-selection tests, invalid-HTML fix + honest `aria-label`.
- Deferred: 0.
- Rejected: 7 (all low) — see Review Triage Log `rejected_notable`.

### Verification
- `bun run check` -- PASS (biome clean, tsc clean, vitest 799 passed / 81 files; +2 new coverage tests).
- `bun run check:rust` -- PASS (rustfmt `--check` clean, clippy `-D warnings` clean).
- `bun run test:rust` -- PASS (cargo-nextest 644 passed, incl. the new own-redaction archive test and the unchanged AD-13 single-`.redact(`-gate guard).

### Residual risks
- A held-delete's undo relies on Story 8.3's Rust-side `cancel_held_send` idempotency (an already-dispatched row returns `""`), so a delete fired in the microsecond the scheduler dispatches is inert rather than a duplicate — the accepted 8.3 boundary, not re-verified here.
- Held bubbles are keyboard-selectable for `⌫` only via a pointer or the Tab-focusable Delete button; arrow-key navigation still excludes them (the undo-send pill remains the primary keyboard undo). A pre-existing arrow-nav design choice, left as-is.
