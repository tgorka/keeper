---
title: 'Reactions'
type: 'feature'
created: '2026-07-04'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: 'e2c64396a3cfaacbf61230ffdcaaa18941e28c5a'
final_revision: '580d9c469bfee40db7f1a675888d84fa8df47c62'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** keeper's timeline (Stories 1.5/1.6/3.4) renders and dispatches text, replies, and edits, but has no way to **react** to a message with an emoji. Incoming reactions from other clients/bridges are invisible, and there is no aggregated pill row, so a lightweight cross-network signal Matrix users expect (FR-12) is missing.

**Approach:** Add emoji reactions confined to `keeper-core` on matrix-sdk-ui 0.18's `Timeline::toggle_reaction(item_id, key)` (dispatch) and `TimelineItemContent::reactions()` (read). Aggregate per-emoji counts with the user's own reaction flagged into a new minimal `ReactionGroupVm` on the timeline VM, surface a `toggle_reaction` Tauri command, and render a click-to-toggle pill row under each bubble plus a curated emoji Popover in the action bar. Incoming reactions re-render through Story 1.5's existing diff stream — no new producer path.

## Boundaries & Constraints

**Always:**
- All reaction relation logic, event IDs, and `TimelineEventItemId` resolution stay in `keeper-core`. The VM carries only `{ emoji, count, is_own }` per group — **no per-sender user IDs, no event IDs cross IPC** (NFR-9 minimalism; keep the existing "no event-id material on the VM" assertion green).
- Reaction dispatch routes **solely** through `keeper-core::send`: `.toggle_reaction(` appears exactly once in the crate, inside the new dispatch fn (extend the FR-41 single-gate source-scan guard to cover it alongside `.send(`/`.send_reply(`/`.edit(`).
- Key resolution mirrors `submit_edit`: resolve the opaque render `key` (`unique_id`) to the SDK `TimelineEventItemId` via `timeline.items()` scan — the frontend never sends event IDs.
- Incoming reactions render via the existing `VectorDiff`/`Set` diff stream (SDK re-emits the item with updated reactions) — **no full timeline re-render, no new producer loop**.
- Own-reaction toggle is symmetric: clicking to add then clicking the own pill retracts it remotely.
- Aggregation preserves the SDK's per-key insertion order; `count` = number of distinct reactors for that emoji; `is_own` = the account's own user ID (or an SDK local reaction status) is present for that emoji.
- Typed VM crosses IPC via `keeper-core::vm` (serde camelCase + `#[ts(export)]`); bindings regenerate without drift.

**Block If:**
- matrix-sdk-ui 0.18 does not expose `Timeline::toggle_reaction(&TimelineEventItemId, &str)` or `TimelineItemContent::reactions() -> Option<&ReactionsByKeyBySender>` as assumed at planning — HALT with an API-mismatch blocking condition rather than inventing a reaction path.

**Never:**
- No emoji-picker dependency — use a static curated reaction set (~8–12 common emoji) in the Popover.
- No optimistic/local reaction state in a JS store and no reaction storage in TypeScript — reactions are diff-driven only (fire IPC, re-render from the stream).
- No per-sender identity list, reactor avatars/tooltips, or free-form emoji input on the VM for this MVP (counts + own-highlight only).
- No changes to the composer store or send-content gate for messages.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Add reaction | User picks 👍 from the action-bar Popover on a message with no own 👍 | `toggle_reaction` dispatches; SDK roundtrips; pill row shows `👍 1` own-highlighted after the diff | SDK dispatch failure → `SendFailed` (retriable), no crash/blank |
| Remove own reaction | User clicks their own highlighted pill | `toggle_reaction` retracts remotely; pill count decrements or the pill disappears via the diff | Same as above |
| Aggregate multiple reactors | 3 users reacted 👍, 1 reacted ❤️, own reaction is ❤️ | Pills render in SDK order: `👍 3` (not highlighted), `❤️ 1` (own-highlighted) | No error expected |
| Incoming reaction | Remote reaction event arrives for a visible message | `Set` diff re-emits the item with updated reactions; pill row updates without full re-render | No error expected |
| Target unresolvable | `item_key` not found in `timeline.items()` | `SendError::TargetNotFound` → `SendFailed` (non-retriable) | Inline, no crash |
| No reactions | Message item has an empty reaction set | `reactions: []`; no pill row rendered | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- Add `ReactionGroupVm { emoji: String, count: u32 (`#[ts(type = "number")]`), is_own: bool }` (serde camelCase + `#[ts(export)]`). Extend `TimelineItemVm::Message` with `reactions: Vec<ReactionGroupVm>` (empty when none). Update `sample_message`/round-trip tests for the new field; keep the "no event-id/user-id material on the VM" assertion.
- `src-tauri/crates/keeper-core/src/timeline.rs` -- Thread the account's own `OwnedUserId` from `open_timeline`/`OpenTimeline` through `forward_timeline` into `item_to_vm(item, index, own_user_id)`. Add a pure `reaction_groups(content, own_user_id) -> Vec<ReactionGroupVm>`: iterate `content.reactions()` (`ReactionsByKeyBySender`, i.e. `IndexMap<emoji, IndexMap<OwnedUserId, ReactionInfo>>`), emitting one group per key with `count = inner.len()` and `is_own = inner.contains_key(own_user_id)`. Fill `reactions` in `item_to_vm` (empty vec for non-message / no reactions). No new diff path — reactions ride the existing Set/Append mapping.
- `src-tauri/crates/keeper-core/src/send.rs` -- Add `toggle_reaction(timeline, item_key, emoji)`: resolve `item_key`→`TimelineEventItemId` via the same items scan as `submit_edit` (`event.identifier()`), then the sole `timeline.toggle_reaction(&item_id, emoji)` call site; ignore the returned added/removed bool (or log). `TargetNotFound` when unresolved; map SDK error to `SendError::Dispatch`. Extend the FR-41 guard test: `.toggle_reaction(` appears exactly once, inside `toggle_reaction`; existing `.send(`/`.send_reply(`/`.edit(` counts unchanged.
- `src-tauri/crates/keeper-core/src/account.rs` -- Add `toggle_reaction(account_id, room_id, item_key, emoji)` mirroring `send_reply` (`open_timeline_for` → delegate to `send::toggle_reaction`; log room id only).
- `src-tauri/crates/keeper/src/ipc.rs` -- Command `toggle_reaction(state, account_id, room_id, item_key, emoji)` via existing `to_ipc_error` (SendError arms already cover `TargetNotFound`→non-retriable `SendFailed` and `Dispatch`→retriable). Add a test asserting the resolve-failure path maps as expected if not already covered.
- `src-tauri/crates/keeper/src/lib.rs` -- Register `ipc::toggle_reaction` in `invoke_handler!`.
- `src/lib/ipc/client.ts` -- Add `toggleReaction(accountId, roomId, itemKey, emoji): Promise<void>` (`invoke("toggle_reaction", …)`); re-export regenerated `ReactionGroupVm` + updated `TimelineItemVm`.
- `src/components/chat/reaction-popover.tsx` -- **NEW** curated-emoji Popover (reuse `@/components/ui/popover` + `Button`, `Smile` from lucide-react): trigger button labeled "Add reaction"; grid/flex of ~8–12 static emoji; clicking one calls `onPick(emoji)`. Purely presentational.
- `src/components/chat/message-actions.tsx` -- Add the `ReactionPopover` alongside Reply/Edit; new props `messageKey` + `onReact(key, emoji)`; wire the Popover pick to `onReact`.
- `src/components/chat/message-bubble.tsx` -- Render a reaction pill row **under** the bubble from `item.reactions` (skip when empty): each pill = emoji + count, own-reaction pills visually highlighted; clicking a pill calls `onToggleReaction(item.key, emoji)`. Pass `onToggleReaction`/`onReact` down to `MessageActions`. New prop `onToggleReaction?`.
- `src/components/layout/conversation-pane.tsx` -- Add `onToggleReaction(key, emoji)` callback → `toggleReaction(accountId, selectedRoomId, key, emoji).catch(() => {})` (guarded on non-null account/room); pass to bubbles (used by both the Popover pick and pill clicks). No new keyboard shortcut.
- `src/**` colocated tests -- `reaction-popover.test.tsx` (opens, pick fires `onPick`); `message-actions.test.tsx` (React button present, fires `onReact`); `message-bubble.test.tsx` (pill row renders counts, own-highlight, empty→no row, pill click toggles).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- Add `ReactionGroupVm` (camelCase + `#[ts(export)]`); extend `TimelineItemVm::Message` with `reactions: Vec<ReactionGroupVm>`. -- Typed aggregated reaction data crossing IPC (counts + own-flag only).
- [x] `src-tauri/crates/keeper-core/src/timeline.rs` -- Thread own `OwnedUserId` through `forward_timeline`→`item_to_vm`; add pure `reaction_groups(content, own_user_id)`; fill `reactions`. -- Derive per-emoji aggregation + own-highlight; keep the diff-stream path (no re-render).
- [x] `src-tauri/crates/keeper-core/src/send.rs` -- Add `toggle_reaction` (sole `Timeline::toggle_reaction` call site; resolve key→`TimelineEventItemId` like `submit_edit`; `TargetNotFound` when unresolved). -- Reaction dispatch inside the single send gate (AD-13/FR-41).
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- Add `toggle_reaction` (mirror `send_reply`; delegate to `send::toggle_reaction`). -- Per-account action dispatch.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- Add `toggle_reaction` command via `to_ipc_error`. -- IPC surface + honest error mapping.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- Register `toggle_reaction` in `invoke_handler!`. -- Wire command.
- [x] `src/lib/ipc/client.ts` -- Add `toggleReaction`; re-export `ReactionGroupVm` + updated `TimelineItemVm`. -- Typed frontend IPC.
- [x] `src/components/chat/reaction-popover.tsx` -- New curated-emoji Popover firing `onPick(emoji)`. -- Discoverable add-reaction entry point.
- [x] `src/components/chat/message-actions.tsx` -- Mount `ReactionPopover`; add `messageKey`/`onReact`. -- Wire the action-bar React affordance (AC1).
- [x] `src/components/chat/message-bubble.tsx` -- Reaction pill row under the bubble (counts, own-highlight, click-to-toggle); pass reaction callbacks down. -- Renders aggregation + toggle (AC1, AC2).
- [x] `src/components/layout/conversation-pane.tsx` -- Add `onToggleReaction` callback → `toggleReaction` IPC; thread to bubbles. -- Wires the feature end to end.
- [x] `src-tauri/crates/keeper-core/src/{vm.rs,timeline.rs,send.rs}` (tests) -- vm serde round-trip for `ReactionGroupVm` + Message with reactions (assert no user-id/event-id material); `reaction_groups` aggregation (count per emoji, `is_own` true/false, empty→`[]`); extended FR-41 guard (`.toggle_reaction(` exactly once, in `toggle_reaction`). -- Lock the aggregation + single-gate contract.
- [x] `src/**` (tests) -- reaction-popover open + pick; message-actions React button fires `onReact`; bubble pill row counts + own-highlight + empty→no row + pill click toggles. -- Cover the I/O matrix + AC behaviors.

**Acceptance Criteria:**
- Given a message, when the user adds an emoji reaction from the action-bar Popover, then it appears in a pill row under the bubble (own-highlighted), round-trips in Matrix-native and bridged Chats, and clicking the own pill retracts it remotely (FR-12).
- Given multiple reactors on one message, when reactions render, then counts aggregate per emoji with the user's own reaction visually highlighted, and clicking a pill toggles it.
- Given incoming reaction events, then they render within the normal diff stream without a full timeline re-render.
- Given the reaction path, then all event IDs, `TimelineEventItemId`s, per-sender user IDs, and relation logic stay in `keeper-core` (only `{emoji, count, is_own}` crosses IPC via the opaque render `key`), and reaction dispatch routes only through `keeper-core::send` (FR-41 single-gate guard passes).
- Given `bun run check:all`, then Biome, tsc, vitest, rustfmt, clippy (`-D warnings`), cargo-nextest, and `cargo deny check` all pass and the ts-rs bindings (`ReactionGroupVm`, updated `TimelineItemVm`) regenerate without drift.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 0, low 3)
- defer: 1: (high 0, medium 0, low 1)
- reject: 11: (high 0, medium 0, low 11)
- addressed_findings:
  - `[low]` `[patch]` Reaction `count` used a lossy `count as u32` cast in `aggregate_reactions`, silently wrapping an absurd (≥2³²) reactor count. Switched to `u32::try_from(count).unwrap_or(u32::MAX)` so the count saturates honestly (project no-silent-loss posture). Practically unreachable, but the pure helper is now correct at the boundary.
  - `[low]` `[patch]` A reaction pill exposed `aria-pressed={group.isOwn}` even when non-interactive (`onToggle == null` ⇒ `disabled`), announcing a toggle state on a non-actionable control. Now `aria-pressed={onToggle != null ? group.isOwn : undefined}`; the two pre-existing pressed-state tests now render with `onToggleReaction` to exercise the interactive path.
  - `[low]` `[patch]` The "no reaction pill row" test asserted absence via a hardcoded emoji regex (`/you reacted|👍|❤️/`), which would false-pass if a regression rendered a pill for a *different* emoji. Rewrote it to assert no `aria-pressed` toggle button exists in either state (pills are the only such buttons), catching a stray pill of any emoji.

## Design Notes

**SDK API (matrix-sdk-ui 0.18, verified at planning).** Dispatch: `Timeline::toggle_reaction(item_id: &TimelineEventItemId, reaction_key: &str) -> Result<bool, Error>` (adds or retracts; returns added/removed — ignored). Read: `TimelineItemContent::reactions() -> Option<&ReactionsByKeyBySender>` where `ReactionsByKeyBySender` wraps `IndexMap<String /*emoji*/, IndexMap<OwnedUserId, ReactionInfo>>` — per-key insertion order and per-sender uniqueness are guaranteed by the SDK, so `count = inner.len()`.

**Aggregation & own-highlight.** `is_own` = the account's own `user_id()` is a key in the emoji's inner sender map. This catches both a confirmed remote reaction (`RemoteToRemote`) and a pending local one (`LocalToLocal`/`LocalToRemote`), since the local reaction is inserted under the own user ID immediately. We thread the own `OwnedUserId` from the client that opened the timeline into `item_to_vm` — mirroring how `is_own` is already derived for messages.

**Why counts-only crosses IPC (NFR-9).** The AC needs aggregated counts + own-highlight, nothing more. Emitting only `{emoji, count, is_own}` keeps per-sender user IDs and reaction event IDs inside `keeper-core`, preserving the timeline VM's "no identity/event-id material" minimalism. Reactor tooltips/avatars are explicitly out of scope for this MVP.

**Single dispatch gate (AD-13/FR-41).** A reaction is outbound event dispatch that needs the same opaque-key→`TimelineEventItemId` resolution as edits, so it belongs in `keeper-core::send` next to `submit_edit`, not scattered. The FR-41 source-scan guard gains a fourth invariant: `.toggle_reaction(` appears exactly once, inside `toggle_reaction`.

**Re-render is free (as in 3.1/3.4).** Received reactions arrive as `Set` diffs re-emitting the item with updated reactions; Story 1.5's producer already streams `Set`/`Append`, so no new re-render code — only the enriched VM mapping. Reactions are stateless on the frontend: fire `toggleReaction`, let the diff update the pills.

**Curated emoji set.** No picker dependency; a static ~8–12 emoji set (e.g. 👍 ❤️ 😂 😮 😢 🎉 🙏 🔥) in a shadcn `Popover`. Reaction keys are arbitrary Matrix strings, so the curated emoji strings pass through unchanged.

## Verification

**Commands:**
- `bun run check` -- Biome + tsc + vitest all green (incl. new reaction-popover/bubble/action tests).
- `bun run check:rust` -- rustfmt clean + clippy `-D warnings` clean.
- `bun run test:rust` -- cargo-nextest green; ts-rs regenerates `ReactionGroupVm` + updated `TimelineItemVm` with no git drift.
- `cargo deny check` (from `src-tauri/`) -- still green; no new crate deps.

**Manual checks (real second session, test credentials in 1Password):**
- React to a message from keeper; confirm the pill appears own-highlighted and the reaction shows in Element (and, where a bridge supports it, on the remote network); click the own pill and confirm it retracts in both.
- Have Element add reactions from multiple users; confirm keeper aggregates counts per emoji and updates live without a full timeline flicker.

## Auto Run Result

Status: done

**Summary:** Added Matrix message **reactions** to the timeline, confined to `keeper-core` on matrix-sdk-ui 0.18's `Timeline::toggle_reaction(item_id, key)` (dispatch) and `TimelineItemContent::reactions()` (read). Dispatch routes through the existing single content-gate module (`keeper-core::send`) as `toggle_reaction`, resolving the frontend's opaque render `key` (`unique_id`) to the SDK's `TimelineEventItemId` by scanning `timeline.items()` (mirroring `submit_edit`) — so **no event IDs, per-sender user IDs, or raw event JSON ever cross IPC**. `TimelineItemVm::Message` gained `reactions: Vec<ReactionGroupVm>`, where each `ReactionGroupVm { emoji, count, is_own }` is aggregated in the producer from `content.reactions()` (per-key insertion order; `count` = distinct reactors; `is_own` = the account's own user id, threaded from `client.user_id()` through `open_timeline`/`forward_timeline`/`item_to_vm`, is present for that emoji). A new `toggle_reaction` Tauri command surfaces it, reusing `SendError::TargetNotFound` (unresolvable key → non-retriable `SendFailed`) and `Dispatch` (SDK failure → retriable). React adds a curated-emoji `ReactionPopover` in the per-message action bar and a click-to-toggle pill row under the bubble (own-reaction pills highlighted via `aria-pressed`); reactions are stateless on the frontend — the toggle IPC fires and the existing Story 1.5 diff stream re-renders the pills, no optimistic state. The FR-41 single-gate source-scan guard was extended so `.toggle_reaction(` appears exactly once, inside `toggle_reaction`.

**Files changed:**
- `src-tauri/crates/keeper-core/src/vm.rs` — `ReactionGroupVm` + `TimelineItemVm::Message.reactions`; serde round-trip + no-identity tests.
- `src-tauri/crates/keeper-core/src/timeline.rs` — own `OwnedUserId` threaded through `open_timeline`/`OpenTimeline`/`forward_timeline`/`map_diff_indexing` into `item_to_vm`; pure `reaction_groups`/`aggregate_reactions` (saturating count cast); aggregation tests.
- `src-tauri/crates/keeper-core/src/send.rs` — `toggle_reaction` (sole `Timeline::toggle_reaction` gate); extended FR-41 source-scan guard.
- `src-tauri/crates/keeper-core/src/account.rs` — `toggle_reaction` account method (mirrors `send_reply`).
- `src-tauri/crates/keeper/src/{ipc.rs,lib.rs}` — `toggle_reaction` command + registration.
- `src/lib/ipc/client.ts` (+ `gen/ReactionGroupVm.ts`, `gen/TimelineItemVm.ts`) — `toggleReaction` wrapper + regenerated bindings.
- `src/components/chat/reaction-popover.tsx` (+ `.test.tsx`) — NEW curated-emoji Popover.
- `src/components/chat/{message-actions.tsx,message-bubble.tsx}` (+ tests) — action-bar React affordance; reaction pill row + `onToggleReaction`.
- `src/components/layout/conversation-pane.tsx` (+ test) — `onToggleReaction` → `toggleReaction` IPC wiring.
- `src/lib/stores/timeline.test.ts` — fixture gained `reactions: []`.

**Review findings breakdown:** intent_gap 0, bad_spec 0, patch 3 (all low, applied), defer 1 (low), reject 11 (all low).
- **Patches applied:** saturating `u32::try_from(count).unwrap_or(u32::MAX)` count cast (was lossy `as u32`); `aria-pressed` on reaction pills only when interactive (`onToggle != null ? group.isOwn : undefined`); rewrote the "no pill row" test to assert no `aria-pressed` toggle button (was a hardcoded emoji-subset regex that could false-pass).
- **Deferred (1):** no behavioral error-path tests for `send::toggle_reaction`/`account::toggle_reaction` (unresolvable key → `TargetNotFound`, unparsable room → `RoomNotFound`) — mirrors the pre-existing send-method coverage gap; needs shared `Timeline` test-harness infra, logged in `deferred-work.md`.
- **Rejected (11, all low):** reactions only on rendered text messages (UTD/media are a coherent MVP boundary owned by 3.1/3.6/3.7); swallowed toggle-failure `catch` (matches the spec's documented "no crash/blank" — reactions are intentionally stateless); `is_own` confirmed-vs-pending (the SDK keys a local echo under the own user id, so `contains_key` is correct); inert Popover when `onToggleReaction` is unwired (all three callbacks are always wired together in the pane); a11y label wording; latent React-key fragility; the source-scan guard only scanning `send.rs` (pre-existing, already deferred by 3.4); a somewhat tautological containment test (containment is structurally correct); a blank pill from a pathological empty remote reaction key; a guarded null-account/room mid-toggle no-op; and picking an already-own emoji retracting it (correct symmetric-toggle semantics).

**Verification performed:**
- `bun run check` — Biome clean, tsc clean, **358** vitest tests pass.
- `bun run check:rust` — `cargo fmt --check` clean, clippy `-D warnings` clean.
- `bun run test:rust` — cargo-nextest **218** tests pass; ts-rs bindings regenerated with drift limited to the intended `ReactionGroupVm.ts` (new) + `TimelineItemVm.ts` (updated).
- `cargo deny check` — licenses ok; no new crate dependencies (`Cargo.lock` unchanged). Pre-existing unrelated GTK3-bindings advisory persists (documented in 3.4), not introduced here.

**Residual risks:**
- Reactions render only on decrypted text messages; a reaction on a still-UTD or media message won't show a pill until that story/decrypt lands (intended MVP boundary).
- A toggle that fails to dispatch (network/`Dispatch`) is silent (no optimistic echo, no error surface) — acceptable per the stateless spec design, but a future reaction-failure indicator would improve honesty.
- Reaction error paths in `keeper-core` are covered only by the pure aggregation + source-scan tests (behavioral error-path tests deferred).
