---
title: 'Delete for Everyone — Redaction'
type: 'feature'
created: '2026-07-04'
status: 'done'
baseline_revision: '30da88a1b83175491ea884e5f7112a6a436dfb0c'
final_revision: '8ef6207e2d6bc109d2d2b5c42984d6d0ccacae55'
review_loop_iteration: 0
followup_review_recommended: false
context: ['{project-root}/docs/project-context.md']
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** A user cannot delete their own messages for everyone. keeper has no Delete affordance, no redaction dispatch, and received redactions render as a blank gap (they map to `TimelineItemVm::Other`, which the frontend skips) — dishonest, since a removed message silently vanishes instead of showing a stub. In bridged Chats, deletion is only best-effort, and the UI must say so and name the Network rather than imply a guaranteed remote delete.

**Approach:** Add a redaction dispatch gate (`Timeline::redact`) reached from a Delete action (action-bar button + `⌫` on a selected own message) confirmed in an AlertDialog. Render received/own redactions as an explicit honest stub via a new `TimelineItemVm::Redacted` variant (mirroring the `Utd` stub). Make the confirmation honest per Chat kind: a native Matrix Room states removal for everyone; a bridged Chat names the Network (derived from the standard MSC2346 `m.bridge` state event) and states remote removal is best-effort.

## Boundaries & Constraints

**Always:**
- Redaction dispatch goes through exactly one SDK call site — `Timeline::redact` in `send::redact` — enforced by the compile-time single-gate guard test (mirrors the send/edit/reaction gates, FR-41/AD-13).
- The Delete affordance is offered only on the user's own messages (`Message.is_own`); `⌫` opens the dialog only when the selected item is own.
- Received and own redactions render an explicit, never-blank stub (honesty invariant, same principle as the UTD stub) — never a silent gap.
- No Matrix/crypto/protocol logic or event raw JSON crosses into TypeScript; the redaction path is a one-shot Tauri command; the bridge-network label crosses IPC only as a resolved display string (AD-1, NFR-9).
- Copy follows UX-DR10 voice: sentence case, no exclamation marks, honest consequence-naming; Glossary nouns (Network, Chat, Room) capitalized.

**Block If:**
- The installed `matrix-sdk-ui` `Timeline` exposes no `redact(item_id, reason)` method (contract assumed present — confirmed for 0.18).

**Never:**
- No local-only "delete for me" / archive-retention behavior — that is Story 5.2 (`Delete ▸` submenu, "Honor remote deletions locally"). MVP has one action: delete for everyone.
- No redaction-reason input UI — dispatch passes `reason: None`.
- No room-list network attribution, network badges, or network filter — that is Story 4.6. This story derives a network label only for the delete confirmation, on demand, and does not touch `RoomVm`.
- Do not remove or re-index redacted items; the SDK emits a `Set` diff turning the item into the Redacted VM in place.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Delete own (native Room) | Own message selected, confirm in dialog | `send::redact` dispatched; SDK emits redaction; item becomes a Redacted stub for all Matrix clients | none expected |
| Delete own (bridged Chat) | Bridged Room, delete dialog opens | Dialog names the Network ("Removal on Telegram is best-effort") + local framing; confirm dispatches redaction | label probe failure → fall back to honest generic bridged framing |
| `⌫` on own selection | Own message selected, focus in timeline (not textarea) | Opens the delete confirmation dialog | — |
| `⌫` on others' / no selection | Selected item not own, or none selected | No-op (Delete is own-only) | — |
| Received redaction | A redaction event arrives (any sender) for a timeline message | Item renders the honest "Message deleted" stub, not blank, not removed | — |
| Redaction dispatch fails | SDK/network error on `redact` | Dialog stays open, shows honest error, action re-enabled for retry | `SendError::Dispatch` → retriable |
| Target vanished | `item_key` no longer in timeline | Honest "message no longer available"; dialog closes | `SendError::TargetNotFound` → non-retriable |
| Bridge label parse | `m.bridge` / `uk.half-shot.bridge` state present or absent | `protocol.displayname` (→ `protocol.id`) when present, else `None` (native framing) | malformed content → `None` |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/send.rs` -- Add `pub async fn redact(timeline: &Timeline, item_key: &str, reason: Option<&str>) -> Result<(), SendError>`: resolve the item by `unique_id().0 == item_key` → `event.identifier()` (`TimelineEventItemId`), then `timeline.redact(&item_id, reason)` — the **sole** `.redact(` gate. Missing item → `TargetNotFound`; SDK error → `Dispatch`. Extend the single-dispatch-gate guard test to assert exactly one `.redact(` inside `redact`.
- `src-tauri/crates/keeper-core/src/bridge.rs` (new) -- `pub fn parse_bridge_network_name(content: &serde_json::Value) -> Option<String>` (MSC2346: `protocol.displayname`, fallback `protocol.id`; trimmed, non-empty). `pub async fn room_bridge_network(room: &Room) -> Option<String>`: read the Room's `m.bridge` then legacy `uk.half-shot.bridge` state events via the SDK state accessor, parse the first with a protocol name. Pure `parse_*` is unit-tested; the room read is a thin wrapper.
- `src-tauri/crates/keeper-core/src/account.rs` -- `pub async fn redact_message(account_id, room_id, item_key, reason: Option<&str>) -> Result<(), CoreError>`: `open_timeline_for` → `send::redact`; log room id + kind only. `pub async fn room_network_label(account_id, room_id) -> Result<Option<String>, CoreError>`: resolve the Room, delegate to `bridge::room_bridge_network`.
- `src-tauri/crates/keeper-core/src/vm.rs` -- Add `TimelineItemVm::Redacted { key, sender, sender_display_name, timestamp }` (mirrors `Utd`; carries only non-secret render data). Update the enum doc: redacted is now its own stub variant, not `Other`.
- `src-tauri/crates/keeper-core/src/timeline.rs` -- In `item_to_vm`, add a `MsgLikeKind::Redacted =>` arm producing `TimelineItemVm::Redacted { key, sender, sender_display_name, timestamp }` before the `_ => Other` fallback; update the mapping doc comment.
- `src-tauri/crates/keeper/src/ipc.rs` -- Commands `delete_message(state, account_id, room_id, item_key)` → `accounts.redact_message(..., None)`; `room_network_label(state, account_id, room_id) -> Result<Option<String>, IpcError>` → `accounts.room_network_label`. Map errors via `to_ipc_error`.
- `src-tauri/crates/keeper/src/lib.rs` -- Register `delete_message` and `room_network_label` in `invoke_handler`.
- `src/lib/ipc/client.ts` -- `deleteMessage(accountId, roomId, itemKey): Promise<void>` → `invoke("delete_message", …)`; `roomNetworkLabel(accountId, roomId): Promise<string | null>` → `invoke("room_network_label", …)`.
- `src/components/chat/message-actions.tsx` + `message-bubble.tsx` -- Add a Delete (`Trash2`) button rendered only when the message is own (`canDelete`), wired to `onDelete(key)`; thread `canDelete`/`onDelete` through the bubble.
- `src/components/chat/redacted-stub.tsx` (new) -- Honest "Message deleted" stub (mirror `utd-stub.tsx`; muted, `role="status"`), exported copy constant.
- `src/components/chat/delete-message-dialog.tsx` (new) -- Controlled AlertDialog (sign-out pattern). On open, fetch `roomNetworkLabel`; render native framing when `null`, else bridged framing naming the Network + best-effort. Destructive confirm → `deleteMessage`; keep open + honest error + re-enabled action on failure.
- `src/components/layout/conversation-pane.tsx` -- Render `kind: "redacted"` as `RedactedStub` in the render-list builder (breaks the same-sender run, like `utd`); add an `⌫`/`Delete` key branch opening the dialog for the selected own message; hold delete-target state and render `DeleteMessageDialog`; thread `onDelete` to bubbles.
- Tests: Rust `#[cfg(test)]` + TS `*.test.tsx` — see test tasks.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/send.rs` -- Add `redact` (sole `Timeline::redact` gate; `TargetNotFound`/`Dispatch`); extend the single-gate guard test with `.redact(`. -- One redaction dispatch path.
- [x] `src-tauri/crates/keeper-core/src/bridge.rs` -- `parse_bridge_network_name` + `room_bridge_network` (MSC2346 `m.bridge`, legacy `uk.half-shot.bridge`). -- Honest bridged-Network label, no fabrication.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- `redact_message` + `room_network_label`. -- Per-account redaction + label resolution.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` + `timeline.rs` -- `TimelineItemVm::Redacted` variant + `MsgLikeKind::Redacted` mapping. -- Received redactions become an honest stub (AC3), not blank.
- [x] `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- `delete_message` + `room_network_label` commands, registered; error mapping. -- IPC surface.
- [x] `src/lib/ipc/client.ts` -- `deleteMessage`, `roomNetworkLabel` wrappers. -- Typed IPC access.
- [x] `src/components/chat/message-actions.tsx` + `message-bubble.tsx` -- Delete button (own-only) + `onDelete` threading. -- Action-bar affordance (AC1).
- [x] `src/components/chat/redacted-stub.tsx` -- Honest "Message deleted" stub. -- Never-blank redaction rendering (AC3).
- [x] `src/components/chat/delete-message-dialog.tsx` -- Confirmation with native vs bridged (Network-naming, best-effort) framing; confirm → `deleteMessage`; error+retry. -- AC1/AC2.
- [x] `src/components/layout/conversation-pane.tsx` -- Render redacted stub; `⌫` opens dialog for own selection; delete-target state + dialog; thread `onDelete`. -- Wiring + keyboard (AC1/AC3).
- [x] `src-tauri/**` tests -- `redact` resolves + gates (guard sees exactly one `.redact(`); `TargetNotFound` on missing item; `parse_bridge_network_name` over `m.bridge`/`uk.half-shot.bridge`/missing/malformed; `item_to_vm` maps `MsgLikeKind::Redacted` → `Redacted` VM; `to_ipc_error` maps redaction arms (Dispatch retriable, TargetNotFound not). -- Lock the contract.
- [x] `src/**` tests -- message-actions: Delete shows only when own, click → `onDelete`; dialog: native copy (label `null`), bridged copy names the Network + best-effort (mocked label), confirm calls `deleteMessage`, dispatch error keeps dialog open + retry; conversation-pane: `kind:"redacted"` renders the stub, `⌫` on own selection opens the dialog, on non-own does nothing. -- Cover the I/O matrix + ACs.

**Acceptance Criteria:**
- Given the user's own message, when they choose Delete (action-bar button or `⌫` with the message selected) and confirm in the AlertDialog, then keeper issues a Matrix Redaction and the timeline shows a redaction stub for all Matrix clients in the Room (FR-15).
- Given a bridged Chat, when the delete confirmation renders, then it names the Network and states removal there is best-effort ("Deletes your copy on this Mac. … Removal on <Network> is best-effort.") (FR-15, UX-DR17); when the Network cannot be identified, it falls back to honest generic best-effort framing rather than a fabricated name.
- Given a received redaction (any sender), then the affected message renders as an explicit "Message deleted" stub in the timeline — never blank and never silently removed (local archive retention is Story 5.2).
- Given a redaction dispatch failure, then the dialog surfaces an honest error and allows retry (retriable), consistent with the send-failure honesty rule.
- Given `bun run check:all`, then Biome, tsc, vitest, rustfmt, clippy (`-D warnings`), cargo-nextest, and `cargo deny check` all pass; ts-rs bindings regenerate to include the new `Redacted` timeline variant with no drift.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 1, low 1)
- defer: 0
- reject: 15: (high 0, medium 0, low 15)
- addressed_findings:
  - `[medium]` `[patch]` The delete confirmation dialog re-enabled the destructive action on **every** failure, never reading `retriable` — so a vanished target (`SendError::TargetNotFound` → non-retriable `SendFailed`) offered a retry that can never succeed, contradicting the I/O matrix ("Target vanished → honest 'message no longer available'; dialog closes") and AC4 ("allows retry (**retriable**)"). `handleConfirm` now branches on `retriable`: a dispatch failure keeps the honest error with a live retry; a non-retriable failure surfaces an honest "This message is no longer available." and withdraws the destructive action (only Cancel remains) so no futile retry loop is offered. Added a non-retriable dialog test (400 vitest total).
  - `[low]` `[patch]` The untrusted, server-controlled bridge Network label was capped at 40 chars with no indication, so a clipped name was presented as the whole Network in the honesty-focused delete copy. `parse_bridge_network_name` now appends an ellipsis only when it actually clips. Updated the cap test to assert the ellipsis and added an at-cap (no-ellipsis) test (274 nextest total).
- rejected_notable: H1/M1 Rust `is_own()` lets a local echo through to a silent send-abort — UI-unreachable (Delete is gated on `sendState === null`) and the comment's literal claim (cannot redact **someone else's** message) still holds; H2 no unit test for the `is_own()` branch — the SDK `EventTimelineItem` constructor is `pub(super)`, the same acknowledged constraint as the UTD arm; M2 native-vs-undetected-bridge conflation — the prior pass's deliberate honest fallback; M3 label-probe copy flicker — the transient copy is itself honest; ECH1/ECH5 stale-target / double-redact — modal-blocked / UI-unreachable (rejected in the prior pass).

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 5: (high 0, medium 2, low 3)
- defer: 1: (high 0, medium 0, low 1)
- reject: 12: (high 0, medium 0, low 12)
- addressed_findings:
  - `[medium]` `[patch]` The delete confirmation's no-label branch promised a guaranteed remote delete ("removes it for everyone in this Chat") — dishonest for a bridged Chat keeper couldn't detect (a bridge that doesn't expose MSC2346 `m.bridge` state, or a swallowed state-store read error), deviating from AC2. Reworded the null case to append an honest conditional best-effort caveat ("If this Chat is bridged to another network, removal there is best-effort") that names no Network; the detected-bridge branch still names it. Also gated the dialog's Esc/backdrop dismiss on `!deleting` so it can't close over an in-flight dispatch. Test updated to assert the conditional caveat and the absence of a fabricated Network.
  - `[medium]` `[patch]` Delete was offered on own **unsent/failed** local echoes (`canDelete = isOwn`), which have no remote event to redact → a redaction attempt that errors instead of the Cancel/Retry path (Story 3.7). Gated `canDelete` (message-bubble) and the pane's `onDelete` + `⌫` handler on `sendState === null`. Added a bubble test (sent → Delete offered; sending/failed → not).
  - `[low]` `[patch]` `send::redact` performed no ownership check while a frontend comment claimed "Rust also gates redaction dispatch" — the own-message rule for a destructive, network-visible action lived only in TypeScript. Added a defensive `event.is_own()` gate in `send::redact` (mirroring `submit_edit`'s `is_editable()`), returning non-retriable `TargetNotFound` for a non-own target, so the invariant holds in Rust and the comment is true.
  - `[low]` `[patch]` The bridge-provided Network label (untrusted, server-controlled) was rendered verbatim and unbounded in the delete confirmation. Capped it to 40 characters in `parse_bridge_network_name` (length, not injection — React already escapes). Added a cap test.
  - `[low]` `[patch]` The `⌫`/Delete key handler called `preventDefault()` and fired for any selection — swallowing the key on non-own messages and triggering on modifier chords (⌘/Ctrl/Alt+⌫). It now ignores modifier chords and only intercepts + opens when the selected target is own and sent; otherwise the key keeps its default. Added a modifier-chord no-op assertion.

## Design Notes

**Redacted is a stub variant, not `Other`.** Redacted items currently hit the `_ => Other` arm and the frontend renders nothing — a silent gap that violates keeper's honesty rule. Adding `TimelineItemVm::Redacted` (a direct mirror of the existing `Utd` stub — `{ key, sender, sender_display_name, timestamp }`, no secret data) lets the frontend render an explicit "Message deleted" row while diff indices stay aligned (the SDK turns the live item into a redacted one via a `Set` diff). This is the same pattern already proven for UTD.

**Bridge Network label is MSC2346, scoped to the confirmation.** mautrix/Beeper bridges publish an `m.bridge` (and legacy `uk.half-shot.bridge`) room state event whose `content.protocol.displayname` names the Network ("Telegram", "WhatsApp"). `room_network_label` reads that on demand when the delete dialog opens; a native Matrix Room has no such event → `None` → the confirmation uses the "removes for everyone in this Chat" framing (redaction is honored by all Matrix clients) with no best-effort caveat. This is deliberately smaller than Story 4.6 (room-list attribution + network filter): no `RoomVm` field, no inbox badges — just one on-demand label for honest delete copy, with an honest fallback so an unrecognized bridge never yields a fabricated name.

**Redaction reuses the proven send gate.** `send::redact` copies the edit/reaction shape exactly: scan `timeline.items()` for `unique_id == item_key`, take `event.identifier()`, call the single SDK method. The compile-time guard test that already pins `.send(`/`.edit(`/`.toggle_reaction(`/`.send_attachment(` to one call site each gains a `.redact(` assertion, so redaction can never grow a second dispatch path.

## Verification

**Commands:**
- `bun run check` -- Biome + tsc + vitest green (incl. new message-actions/dialog/pane/stub tests).
- `bun run check:rust` -- rustfmt clean + clippy `-D warnings` clean.
- `bun run test:rust` -- cargo-nextest green; ts-rs bindings regenerate with the new `Redacted` variant and no git drift.
- `cargo deny check` (from `src-tauri/`) -- green; no new dependency (advisories-only pre-existing gtk-rs failures excepted, as in prior stories).

**Manual checks (real second session, test credentials in 1Password):**
- In an encrypted native Matrix Room, delete an own message via the action-bar button and via `⌫`; confirm the receiving Element client shows a redaction stub and keeper shows "Message deleted".
- Have the second session redact a message; confirm keeper renders the stub in place (not blank, not removed).
- In a Beeper bridged Chat, open Delete; confirm the dialog names the Network and states removal is best-effort.
- Force a dispatch failure (kill network mid-delete); confirm the dialog shows an honest error and retry succeeds after reconnect.

## Auto Run Result

Status: done

### 2026-07-04 — Follow-up review pass

A second independent review (Blind Hunter + Edge Case Hunter, run without prior context) surfaced 17 findings; two were patched, the rest rejected (all no/negligible consequence) and none deferred (the one live-region item was already logged from the prior pass).

**Patches applied (2):**
- **[medium] Dialog now honors `retriable`.** The delete confirmation's `handleConfirm` treated every failure identically — kept the dialog open and re-enabled the destructive action — never reading `IpcError.retriable`. That contradicted the spec I/O matrix (Target vanished → non-retriable, "message no longer available") and offered a retry that can never succeed for a `TargetNotFound`. It now branches: a retriable dispatch failure keeps the honest error with a live retry; a non-retriable failure shows an honest "This message is no longer available." and withdraws the destructive action (only Cancel remains). New dialog test covers the non-retriable path.
- **[low] Honest truncation of the untrusted Network label.** `parse_bridge_network_name` capped the server-controlled label at 40 chars silently; a clipped name was presented as the whole Network in an honesty-focused confirmation. It now appends an ellipsis only when it actually clips. Cap test updated + at-cap (no-ellipsis) test added.

**Files changed in this pass:**
- `src/components/chat/delete-message-dialog.tsx` — retriable-aware error handling (terminal vs retryable), `errorRetriable` state, destructive action withdrawn on a non-retriable failure.
- `src/components/chat/delete-message-dialog.test.tsx` — non-retriable terminal-error test.
- `src-tauri/crates/keeper-core/src/bridge.rs` — ellipsis on truncation; cap test updated + at-cap test added.

**Notable rejections:** H1/M1 (Rust `is_own()` lets a local echo through to a silent send-abort) — UI-unreachable (Delete is gated on `sendState === null`) and the comment's literal "cannot redact someone else's message" claim still holds; H2 (no unit test for the `is_own()` branch) — the SDK `EventTimelineItem` constructor is `pub(super)`, the same acknowledged limit as the UTD arm; M2 (native-vs-undetected-bridge conflation), M3 (probe copy flicker), ECH1/ECH5 (stale-target / double-redact, modal-blocked / UI-unreachable) — all previously considered and by-design or unreachable.

**Verification (re-run after patches):**
- `bun run check` — Biome clean, tsc clean, **400** vitest tests pass (43 files).
- `bun run check:rust` — `cargo fmt --check` clean, clippy `-D warnings` clean.
- `bun run test:rust` — cargo-nextest **274** tests pass; no ts-rs binding drift (no `src/lib/ipc/gen/` change).
- `cargo deny check` — bans/licenses/sources clean; `advisories` FAILED is the pre-existing gtk-rs/unicode/`proc-macro-error2` unmaintained baseline transitive from Tauri's Linux backend — no new dependency (`Cargo.toml`/`Cargo.lock` unchanged).

**Follow-up review:** not recommended (`followup_review_recommended: false`). This pass made two localized fixes — one medium spec-conformance correction with a test, one low honesty-cosmetic — not a broad or behavior-sweeping change.

---

### Original run

**Summary:** Implemented FR-15 delete-for-everyone via Matrix redaction, end to end. Own messages gain a Delete affordance (action-bar `Trash2` button + `⌫`/Delete on a selected own, sent message) that opens an AlertDialog and, on confirm, dispatches through a single new gate `send::redact` → matrix-sdk-ui `Timeline::redact` (mirroring the edit/reaction gates, with a defensive `is_own()` check and the compile-time single-`.redact(`-gate guard test). Received/own redactions now render an explicit honest "Message deleted" stub instead of a silent gap: a new `TimelineItemVm::Redacted` variant (mirroring `Utd`) is produced from `MsgLikeKind::Redacted`, and the frontend renders `RedactedStub` in place (never blank, never re-indexed). The delete confirmation is honest per Chat kind: a native/undetected Chat gets a conditional best-effort caveat that names no Network; a detected bridged Chat names the Network (derived on demand from the standard MSC2346 `m.bridge`/legacy `uk.half-shot.bridge` room state event, capped and read as opaque `content` only) and states remote removal is best-effort. Scoped smaller than Story 4.6 (no `RoomVm` field, no inbox attribution/filter).

**Files changed:**
- `src-tauri/crates/keeper-core/src/send.rs` — `redact` (sole `Timeline::redact` gate; `is_own()` defense-in-depth; `TargetNotFound`/`Dispatch`); single-gate guard test extended with `.redact(`.
- `src-tauri/crates/keeper-core/src/bridge.rs` (new) — pure `parse_bridge_network_name` (MSC2346, 40-char cap) + `room_bridge_network` (reads only opaque `content`); 9 unit tests.
- `src-tauri/crates/keeper-core/src/account.rs` — `redact_message` + `room_network_label`.
- `src-tauri/crates/keeper-core/src/vm.rs` + `timeline.rs` — `TimelineItemVm::Redacted` variant + `MsgLikeKind::Redacted` mapping + VM serde-shape test.
- `src-tauri/crates/keeper-core/src/lib.rs` — `pub mod bridge`.
- `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` — `delete_message` + `room_network_label` commands, registered.
- `src/lib/ipc/gen/TimelineItemVm.ts` — regenerated (`redacted` variant).
- `src/lib/ipc/client.ts` — `deleteMessage` + `roomNetworkLabel` wrappers.
- `src/components/chat/message-actions.tsx` + `message-bubble.tsx` — own-and-sent-only Delete (`Trash2`); `canDelete = isOwn && sendState === null`.
- `src/components/chat/redacted-stub.tsx` (new) — "Message deleted" stub.
- `src/components/chat/delete-message-dialog.tsx` (new) — on-demand Network-label probe; native/undetected vs detected-bridge framing (honest conditional best-effort caveat, no fabricated name); confirm → `deleteMessage`; error keeps open + retry; dismiss guarded while dispatching.
- `src/components/layout/conversation-pane.tsx` — redacted render row; delete-target state + dialog; `⌫` opens for own+sent selection, ignores modifier chords, no silent swallow.
- Tests: `message-actions.test.tsx`, `message-bubble.test.tsx`, `delete-message-dialog.test.tsx`, `redacted-stub.test.tsx`, `conversation-pane.test.tsx`.

**Review findings breakdown:** intent_gap 0, bad_spec 0, patch 5 (medium 2, low 3 — all applied + tested), defer 1 (low), reject 12 (all low).
- **Patches applied:** (1) [med] null-label confirmation over-promised a guaranteed remote delete on undetected bridges → honest conditional best-effort caveat + dismiss guarded while deleting; (2) [med] Delete was offered on unsent/failed own echoes → gated on `sendState === null`; (3) [low] `send::redact` had no ownership gate (comment claimed otherwise) → added `is_own()` gate; (4) [low] untrusted bridge label rendered unbounded → capped to 40 chars; (5) [low] `⌫` swallowed keys / fired on modifier chords → own+sent-only, modifier-aware.
- **Deferred (1):** timeline stub Alerts (`RedactedStub` + pre-existing `UtdStub`) use `role="status"` (aria-live) → repeated SR announcements on reset batches — logged in `deferred-work.md`.
- **Rejected (12, all low):** stale-label (handled by effect reset), SDK `Redacted` mapping untestable end-to-end (harness limit, matches UTD precedent), unused stub fields (non-secret, UTD precedent), confirm double-click (disabled + Radix), redaction eventual-consistency copy (matches edit/send), catch coercion (client normalizes to `IpcError`), grouping-break/heavy-Alert (cosmetic, UTD precedent), missing action-bar e2e test (transitively covered), setState-after-close (benign, parent-controlled), itemKey-switch-mid-confirm (modal blocks), double-redact-already-redacted (UI-unreachable), multiple-`m.bridge` non-determinism (rare, still honest).

**Verification performed (independently re-run after patches):**
- `bun run check` — Biome clean, tsc clean, **399** vitest tests pass (43 files).
- `bun run check:rust` — `cargo fmt --check` clean, clippy `-D warnings` clean.
- `bun run test:rust` — cargo-nextest **273** tests pass; ts-rs bindings regenerated with drift limited to the intended `TimelineItemVm.ts` (`redacted` variant).
- `cargo deny check` — licenses/bans/sources clean; no new dependency (`Cargo.toml`/`Cargo.lock` unchanged). The pre-existing `advisories` gtk-rs/GTK3 unmaintained set from Tauri's Linux backend is on the baseline — no new advisory introduced.

**Residual risks:** Live E2EE redaction against a real second Matrix session (Element showing the redaction stub, real Beeper bridged-room `m.bridge` state shape, network-kill mid-delete) was not exercised here — see Manual checks. The MSC2346 probe only detects bridges that publish `m.bridge`/`uk.half-shot.bridge`; undetected bridges fall to the honest conditional best-effort caveat (they are never told deletion is guaranteed), but are not named — full per-network attribution is Story 4.6. `item_to_vm`'s `Redacted` arm is verified by compilation + a VM serde-shape test, not a constructed SDK `EventTimelineItem` (same `pub(super)` constructor constraint as the existing UTD arm). Follow-up review recommended given the destructive, network-visible nature of the flow and the honesty-copy correction.
