---
title: 'Replies and Edits'
type: 'feature'
created: '2026-07-04'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '267516c555832290ea277a3614b6b9e2de98be05'
final_revision: 'ba1d3904f426c7b7d6d50d3a24ec89a85c70e640'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** keeper's timeline (Story 1.5/1.6) renders and sends only flat text. There is no way to **reply** to a specific message (with the quoted original shown inline and a jump-to-original affordance) nor to **edit** one's own message in place with an "Edited" caption. Received replies and edits from other clients/bridges also render as plain or invisible rows, losing conversation structure across Matrix and Bridges (FR-10, FR-11).

**Approach:** Add reply + edit on matrix-sdk-ui 0.18's `Timeline::send_reply(content, in_reply_to)` and `Timeline::edit(item_id, EditedContent)` — confined to `keeper-core`, dispatched through the existing single content-gate module (`keeper-core::send`, AD-13/FR-41). Extend `TimelineItemVm::Message` with a `reply` preview (quoted sender + body + an opaque jump key) and an `is_edited` flag, both derived in the timeline producer from `content.in_reply_to()` / `message.is_edited()`. The producer keeps a Rust-side `event_id → render-key` index so a reply's jump target is expressed only as the same opaque `key` (unique_id) used everywhere — **no event IDs cross IPC**. React adds a per-message action bar (Reply; Edit on own), a composer reply/edit context banner (Esc cancels without losing composer text), an inline reply quote (click → scroll to original), and the keyboard affordances (`r`, `e`, `↑`-in-empty-composer-edits-last-own).

## Boundaries & Constraints

**Always:**
- All reply/edit SDK calls, event IDs, `TimelineEventItemId`, and relation inspection live in `keeper-core`; the webview receives only the existing opaque `key` (unique_id), a resolved display name, and decoded plain-text (AD-1, AD-7, NFR-9). The quoted-reply preview and its jump target are expressed only as non-secret render data — **no event IDs, txn IDs, or raw event JSON cross IPC** (upholds the vm.rs containment posture that `TimelineItemVm` carries none of these).
- Reply and edit are **content dispatches**, so they route through the single-gate module `keeper-core::send` alongside `submit` (AD-13/FR-41) — new `submit_reply`/`submit_edit` functions there; no `Timeline::send_reply`/`Timeline::edit`/`Timeline::send` call sites anywhere else in the crate, asserted by a source-scan guard.
- Reply/edit targets are addressed by the opaque render `key` (unique_id); resolve it to the SDK identifier by scanning `timeline.items()` for `unique_id().0 == key` (mirror `send::retry`) — reply → `event_id()` (`OwnedEventId` for `send_reply`), edit → `identifier()` (`TimelineEventItemId` for `edit`).
- Received edits render the **latest** content with an "Edited" caption (`message.is_edited()` → true); the SDK delivers the update as a `Set` diff, so no new re-render code is needed — reuse Story 1.5's streaming producer.
- Received replies render the quoted original inline from `content.in_reply_to()`; when the original is loaded, the preview carries its render `key` so the frontend jump scrolls to it; when the original is not loaded, the quote still renders honestly but is not clickable (`in_reply_to_key: null`).
- Pending edit/reply context cancels on `Esc` without losing composer text: entering **edit** stashes the current draft and prefills the message body; `Esc` restores the stashed draft. **Reply** leaves the typed draft untouched and only clears the reply target.
- Only own messages are editable (gate on `is_own` + `is_editable()`); the Edit action and `e`/`↑`-edit shortcuts are offered only for own text messages.

**Block If:**
- matrix-sdk-ui 0.18 does not expose `Timeline::send_reply` / `Timeline::edit` / `EditedContent` / `content.in_reply_to()` / `message.is_edited()` as researched (verified present at planning) — the reply or edit leg is then unbuildable as specified.

**Never:**
- No threads/thread-root UX (`content.thread_root()` is ignored this story), no rich-text/markdown/formatted-body editing, no reply-to-media captions or `EditedContent::MediaCaption`/`PollStart` paths (text only — media is Story 3.7).
- No archive/pre-edit history retention (Epic 5 / Story 5.2 owns edit history and durability); keeper shows only the latest content here.
- No new crate dependencies; no `matrix-js-sdk`; no reply/edit relation logic, event IDs, or plaintext in TypeScript.
- Do not synthesize local echo or re-render logic — replies/edits appear via the existing timeline diff stream, same as text sends.
- No React/Delete/Copy actions in the action bar this story (React → 3.5, Delete → 3.8); add Reply and Edit only.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Send reply | User replies to a loaded message (action bar or `r`); composer has text | `send_reply(account, room, in_reply_to_key, body)` → resolve key→`OwnedEventId` → `submit_reply` → `timeline.send_reply(text_plain(body), event_id)`; local echo appears via stream with its own `reply` preview; composer clears reply target | Empty body → no-op (trim guard); key not resolvable / no event_id → `SendError::TargetNotFound` → `SendFailed` (non-retriable) |
| Render received reply | Incoming message with `content.in_reply_to()` Some, original loaded | `Message.reply = ReplyPreviewVm { in_reply_to_key: Some(<original key>), sender, sender_display_name, body }`; bubble shows quote; click quote → scroll original into view + brief highlight | Original not loaded (details Unavailable) → `in_reply_to_key: null`, quote renders from available data, not clickable |
| Send edit | User edits own message (action bar or `e`/`↑`-empty); confirms new text | `edit_message(account, room, item_key, body)` → resolve key→`TimelineEventItemId` → `submit_edit` → `timeline.edit(id, EditedContent::RoomMessage(text_plain(body)))`; timeline `Set` diff updates in place, `is_edited` → true, "Edited" caption; composer restores stashed draft | Empty body → no-op; not own / not text → `SendError::NotEditable` → `SendFailed` (non-retriable); key not found → `TargetNotFound` |
| Render received edit | Incoming edit (`m.replace`) to a message | `message.is_edited()` → true; `Message.is_edited: true`, body = latest content; "Edited" caption renders; arrives as a `Set` diff (no full re-render) | — |
| Cancel pending (Esc) | Composer has a pending reply or edit | Esc clears the pending context; reply keeps typed draft; edit restores the pre-edit stashed draft | — |
| Edit last own via `↑` | Composer empty, caret at start, `↑` pressed | Opens edit on the last own text message (stash empty draft, prefill its body) | No own text message in view → no-op |
| Dispatch failure | SDK rejects the reply/edit enqueue | `SendError::Dispatch(reason)` → `SendFailed` (retriable); non-secret reason only | Never leak body/event id to `tracing` |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- Add `ReplyPreviewVm { in_reply_to_key: Option<String>, sender: String, sender_display_name: Option<String>, body: String }` (serde camelCase + `#[ts(export)]`). Extend `TimelineItemVm::Message` with `is_edited: bool` and `reply: Option<ReplyPreviewVm>`. Update the existing `sample_message`/round-trip tests for the new fields; keep the "no event-id material on the VM" assertion.
- `src-tauri/crates/keeper-core/src/timeline.rs` -- Change `item_to_vm(item, index: &HashMap<OwnedEventId, String>)` to fill `is_edited` (`message.is_edited()`) and `reply` (pure `reply_preview(content, index)` helper: `content.in_reply_to()` → sender/display-name/`text_body`-of-embedded, and `in_reply_to_key` = `index.get(&details.event_id)`). In `forward_timeline`, maintain the `event_id → unique_id` index across the snapshot + diffs (insert each event item's `event_id()→unique_id().0` as it is mapped; clear on `Clear`/reset) so a reply resolves an already-mapped earlier original. Reuse `text_body` for the embedded body (empty string when the original is non-text).
- `src-tauri/crates/keeper-core/src/send.rs` -- Add `submit_reply(timeline, in_reply_to_key, text)` (resolve key→`OwnedEventId` via items scan like `retry`; `RoomMessageEventContentWithoutRelation::text_plain`; sole `Timeline::send_reply` call site) and `submit_edit(timeline, item_key, new_text)` (resolve key→item, gate `is_editable()`, `identifier()`→`TimelineEventItemId`; `EditedContent::RoomMessage(text_plain)`; sole `Timeline::edit` call site). Trim-guard empties. Extend the FR-41 guard test: the crate's single `.send(content)` gate is unchanged, and `.send_reply(`/`.edit(` each appear exactly once, inside `submit_reply`/`submit_edit` respectively.
- `src-tauri/crates/keeper-core/src/error.rs` -- Add `SendError::TargetNotFound` ("referenced message not found") and `SendError::NotEditable` ("message can't be edited").
- `src-tauri/crates/keeper-core/src/account.rs` -- Add `send_reply(account_id, room_id, in_reply_to_key, body)` and `edit_message(account_id, room_id, item_key, body)` mirroring `send_text` (`open_timeline_for` → delegate to `send::submit_reply`/`submit_edit`; log room id only).
- `src-tauri/crates/keeper/src/ipc.rs` -- Commands `send_reply(state, account_id, room_id, in_reply_to_key, body)` and `edit_message(state, account_id, room_id, item_key, body)` via `to_ipc_error`. Add `to_ipc_error` arms: `SendError::TargetNotFound | NotEditable` → `(SendFailed, false)`; existing arm unchanged. Test the new arms.
- `src-tauri/crates/keeper/src/lib.rs` -- Register `ipc::send_reply` and `ipc::edit_message` in `invoke_handler!`.
- `src/lib/ipc/client.ts` -- Add `sendReply(accountId, roomId, inReplyToKey, body): Promise<void>` (`invoke("send_reply", …)`) and `editMessage(accountId, roomId, itemKey, body): Promise<void>` (`invoke("edit_message", …)`); re-export regenerated `ReplyPreviewVm` + updated `TimelineItemVm`.
- `src/lib/stores/composer.ts` -- **NEW** zustand vanilla store (mirror `stores/rooms.ts`): `pending: { mode: 'reply'; targetKey; sender; bodyPreview } | { mode: 'edit'; targetKey } | null`, `stashedDraft: string | null`, `selectedKey: string | null`; actions `startReply(target)`, `startEdit(target, currentDraft)` (stash + return body to prefill), `cancel()` (returns draft to restore for edit), `clear()`, `select(key)`, `clearSelection()`.
- `src/components/chat/message-actions.tsx` -- **NEW** hover/focus action bar over a bubble: Reply (always) → `startReply`; Edit (own only) → `startEdit`. Labeled, keyboard-focusable buttons.
- `src/components/chat/message-bubble.tsx` -- Render an inline reply quote block (sender + body preview) above the body when `msg.reply` is set; clickable when `reply.inReplyToKey` (→ `onJumpTo(key)`). Render an "Edited" caption when `msg.isEdited`. Mount `<MessageActions>` on hover/focus; add `data-msg-key={key}` and a selected-ring when `selectedKey === key`.
- `src/components/chat/composer.tsx` -- Render a reply/edit context banner above the textarea when `pending` (quoted sender/preview for reply, "Editing your message" for edit) with a cancel (×) control; `Esc` cancels (restore stashed draft on edit). On send: route to `sendReply` (pending reply), `editMessage` (pending edit), else `sendText`; clear pending on success. On entering edit, prefill the textarea with the message body.
- `src/components/layout/conversation-pane.tsx` -- Wire `pending`/`onSend` to route reply/edit; pass `onReply`/`onEdit`/`onJumpTo`/`selectedKey` to bubbles; `onJumpTo(key)` scrolls `[data-msg-key]` into view + brief highlight; add a keydown handler (↑/↓ select message, `r` reply selected, `e` edit selected-if-own, `↑` in empty composer edits last own message, `Esc` clears pending/selection).
- `src/**` colocated tests -- `composer.ts` store transitions; `composer.test.tsx` banner + Esc restore + send routing; `message-bubble.test.tsx` reply quote + jump + Edited caption; `message-actions.test.tsx` Reply always / Edit own-only; keyboard `r`/`e`/`↑` behavior.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- Add `ReplyPreviewVm` (camelCase + `#[ts(export)]`); extend `TimelineItemVm::Message` with `is_edited: bool` + `reply: Option<ReplyPreviewVm>`. -- Typed reply/edit render data crossing IPC.
- [x] `src-tauri/crates/keeper-core/src/error.rs` -- Add `SendError::TargetNotFound` + `SendError::NotEditable`. -- Named non-retriable failures.
- [x] `src-tauri/crates/keeper-core/src/send.rs` -- Add `submit_reply` (sole `send_reply` call site) and `submit_edit` (sole `edit` call site; gate `is_editable()`); resolve keys via items scan; text-only content. -- Reply/edit dispatch inside the single content-gate module (AD-13/FR-41).
- [x] `src-tauri/crates/keeper-core/src/timeline.rs` -- Thread an `event_id → unique_id` index through `forward_timeline`; add pure `reply_preview(content, index)`; fill `is_edited`/`reply` in `item_to_vm`. -- Derive reply preview + edited state; keep IPC event-id-free.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- Add `send_reply`/`edit_message` (mirror `send_text`, delegate to `send::submit_reply`/`submit_edit`). -- Per-account action dispatch.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- Add `send_reply`/`edit_message` commands + `to_ipc_error` arms for the two new variants (`SendFailed`, non-retriable). -- IPC surface + honest error mapping.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- Register both commands in `invoke_handler!`. -- Wire commands.
- [x] `src/lib/ipc/client.ts` -- Add `sendReply`/`editMessage`; re-export `ReplyPreviewVm` + updated `TimelineItemVm`. -- Typed frontend IPC.
- [x] `src/lib/stores/composer.ts` -- New store: `pending`, `stashedDraft`, `selectedKey` + `startReply`/`startEdit`/`cancel`/`clear`/`select`/`clearSelection`. -- Drives banner, action routing, keyboard selection.
- [x] `src/components/chat/message-actions.tsx` -- New hover/focus action bar: Reply (all) + Edit (own). -- Discoverable reply/edit entry points.
- [x] `src/components/chat/message-bubble.tsx` -- Inline reply quote (clickable when key present) + "Edited" caption + action bar mount + `data-msg-key` + selected ring. -- Renders reply structure + edited state (AC1, AC2).
- [x] `src/components/chat/composer.tsx` -- Reply/edit banner, Esc-cancel with draft preservation, send routing to reply/edit/text, edit prefill. -- Composition + cancel semantics (AC1–AC3).
- [x] `src/components/layout/conversation-pane.tsx` -- Route pending state + callbacks; jump-to-original scroll; keyboard handler (↑/↓, `r`, `e`, `↑`-empty-edit, `Esc`). -- Wires the feature end to end.
- [x] `src-tauri/crates/keeper-core/src/{vm.rs,timeline.rs,send.rs}` (tests) -- vm serde round-trip for `ReplyPreviewVm` + Message with reply/`is_edited`; `item_to_vm` reply-resolved / reply-unresolved (`null` key) / `is_edited: true` with a built index; extended FR-41 guard (`.send_reply(`/`.edit(` each once, in the right fn). -- Lock the mapping + single-gate contract.
- [x] `src-tauri/crates/keeper/src/ipc.rs` (tests) -- Assert `to_ipc_error` maps `TargetNotFound`/`NotEditable` to `SendFailed` (non-retriable). -- Lock the error contract.
- [x] `src/**` (tests) -- composer store transitions (startReply/startEdit stash, cancel restores edit draft, reply keeps draft); composer banner + Esc + send routing; bubble reply quote + jump + Edited caption; action bar Reply-always/Edit-own-only; keyboard `r`/`e`/`↑`. -- Cover the I/O matrix + AC behaviors.

**Acceptance Criteria:**
- Given any message in the timeline, when the user replies (action bar or `r` with the message selected) and sends, then the sent reply renders with the quoted original inline, and clicking a received reply's quote jumps to (scrolls to) the original message in the timeline (FR-10).
- Given the user's own message, when they edit it (action bar, or `↑` in an empty composer for the last own message) and confirm, then the timeline updates in place with an "Edited" caption; and received edits render the latest content with the "Edited" caption (FR-11).
- Given edit or reply composition, when the user presses Esc, then the pending edit/reply context cancels without losing composer text (reply keeps the typed draft; edit restores the pre-edit draft).
- Given the reply/edit path, then all event IDs, `TimelineEventItemId`s, and relation logic stay in `keeper-core` (no event IDs or raw event JSON cross IPC; the jump target is the existing opaque `key`), and reply/edit content dispatch routes only through `keeper-core::send` (AD-13/FR-41 single-gate guard passes).
- Given `bun run check:all`, then Biome, tsc, vitest, rustfmt, clippy (`-D warnings`), cargo-nextest, and `cargo deny check` all pass and the ts-rs bindings (`ReplyPreviewVm`, updated `TimelineItemVm`) regenerate without drift.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 1, low 1)
- defer: 3: (high 0, medium 0, low 3)
- reject: 12: (high 0, medium 0, low 12)
- addressed_findings:
  - `[low]` `[patch]` Entering edit stashed a hardcoded `""` (the parent `startEdit(..., "")` couldn't see the composer's local draft), so Esc-after-edit restored an empty composer, silently discarding any pre-edit typed draft — a direct gap vs the AC "edit restores the pre-edit draft". Moved draft preservation into `composer.tsx` (which owns the draft): a `preEditDraft` ref captures the outgoing draft in the prefill effect and `cancelPending` restores it on edit-cancel; the store's now-vestigial stash is left as belt-and-suspenders. Test rewritten to the realistic flow (type → enter edit → Esc → draft restored).
  - `[medium]` `[patch]` `onSend` cleared the pending context unconditionally after the enqueue await, so a reply/edit the user started *during* the in-flight enqueue was wiped on the prior send's success. Scoped the clear to only fire when the still-current pending is the same `(mode, targetKey)` that was dispatched.

## Design Notes

**SDK API (matrix-sdk-ui 0.18, verified at planning).** `Timeline::send_reply(content: RoomMessageEventContentWithoutRelation, in_reply_to: OwnedEventId)` builds the reply relation and enqueues it. `Timeline::edit(item_id: &TimelineEventItemId, new_content: EditedContent)` replaces content (only when `EventTimelineItem::is_editable()` — own + text/media/etc). Read side: `TimelineItemContent::in_reply_to() -> Option<InReplyToDetails>` gives `event_id` + `event: TimelineDetails<Box<EmbeddedEvent>>` (the embedded original's `content`/`sender`/`sender_profile` when `Ready`); `message.is_edited() -> bool`. `RoomMessageEventContentWithoutRelation::text_plain(body)` and `EditedContent::RoomMessage(...)` are the text constructors.

**Why key resolution, not event IDs over IPC.** `vm.rs` documents that no event/txn ids cross IPC on the timeline VM (secret-containment minimalism, NFR-9). We keep that: the frontend passes the opaque render `key` (unique_id) for reply/edit targets, and `keeper-core` resolves it to `event_id()` (reply) or `identifier()` (edit) by scanning `timeline.items()` — exactly how `send::retry` resolves its wedged echo. For jump-to-original, the producer holds an `event_id → unique_id` index built while mapping items, so the reply preview carries the *original's* opaque `key` (never its event id). A reply whose original isn't loaded yields `in_reply_to_key: null` — the quote still renders, just isn't clickable.

**Single content-gate (AD-13/FR-41).** Replies and edits feed new content to the send queue, so they belong in `keeper-core::send` next to `submit`, not scattered. The existing guard scans for exactly one `.send(content)` (unchanged). Extend it so `.send_reply(` and `.edit(` each appear exactly once, inside `submit_reply`/`submit_edit` — the content-dispatch surface stays one module.

**Esc / draft preservation.** The composer's local draft is the user's reply text in reply mode (untouched on cancel). Entering edit stashes that draft and loads the message body into the textarea; Esc restores the stashed draft — satisfying "cancels without losing composer text" for both modes.

**Re-render is free (as in 3.1/3.3).** Received edits arrive as a `Set` diff and received replies as ordinary event items with `in_reply_to()` populated; Story 1.5's producer already streams `Set`/`Append`, so no new re-render code — only the enriched VM mapping.

## Verification

**Commands:**
- `bun run check` -- Biome + tsc + vitest all green (incl. new composer/bubble/action tests).
- `bun run check:rust` -- rustfmt clean + clippy `-D warnings` clean.
- `bun run test:rust` -- cargo-nextest green; ts-rs regenerates `ReplyPreviewVm` + updated `TimelineItemVm` with no git drift.
- `cargo deny check` (from `src-tauri/`) -- still green; no new crate deps.

**Manual checks (real second session, test credentials in 1Password):**
- Reply to a message from keeper; confirm the quote renders inline and the reply arrives as a reply in Element (and, where a bridge supports it, on the remote network); click a received reply's quote and confirm it scrolls to the original.
- Edit an own message from keeper (action bar and `↑`-in-empty-composer); confirm it updates in place with "Edited" both in keeper and Element, and a received edit shows the latest content + "Edited".
- Start a reply and an edit, type/adjust text, press Esc; confirm the draft is preserved (reply) / restored (edit).

## Auto Run Result

Status: done

**Summary:** Added Matrix message **replies** and **edits** to the timeline, confined to `keeper-core` on matrix-sdk-ui 0.18's `Timeline::send_reply(content, in_reply_to)` and `Timeline::edit(item_id, EditedContent)`. Both dispatch through the existing single content-gate module (`keeper-core::send`) as `submit_reply`/`submit_edit`, resolving the frontend's opaque render `key` (`unique_id`) to the SDK's `OwnedEventId` / `TimelineEventItemId` by scanning `timeline.items()` (mirroring `send::retry`) — so **no event IDs, txn IDs, or raw event JSON ever cross IPC**. `TimelineItemVm::Message` gained `is_edited` (from `message.is_edited()`) and a `reply: Option<ReplyPreviewVm>` quoted-original preview (from `content.in_reply_to()`), the latter carrying the original's opaque jump `key` resolved via a producer-owned `event_id → unique_id` index. Two new Tauri commands (`send_reply`, `edit_message`) surface it, with `SendError::TargetNotFound`/`NotEditable` mapping to a non-retriable `SendFailed`. React adds a per-message hover/focus action bar (Reply always; Edit on own), an inline reply quote (click → scroll to original + brief highlight), an "Edited" caption, a composer reply/edit context banner with Esc-cancel that preserves composer text (reply keeps the typed draft; edit restores the pre-edit draft), and the keyboard affordances (`↑`/`↓` select, `r` reply, `e` edit own, `↑`-in-empty-composer edits last own). Received edits/replies re-render via Story 1.5's existing diff stream — no new re-render code.

**Files changed:**
- `src-tauri/crates/keeper-core/src/send.rs` — `submit_reply` (sole `Timeline::send_reply` gate) + `submit_edit` (sole `Timeline::edit` gate, `is_editable()`-gated); FR-41 source-scan guard extended for both.
- `src-tauri/crates/keeper-core/src/timeline.rs` — `ReplyIndex` (`event_id → unique_id`) threaded through `forward_timeline`; pure `reply_preview`/`reply_preview_from_details`; `item_to_vm` fills `is_edited`/`reply`.
- `src-tauri/crates/keeper-core/src/vm.rs` — `ReplyPreviewVm` + `TimelineItemVm::Message.{is_edited,reply}`; serde round-trip tests (assert no event-id material).
- `src-tauri/crates/keeper-core/src/{error.rs,account.rs}` — `SendError::{TargetNotFound,NotEditable}`; `send_reply`/`edit_message` account methods.
- `src-tauri/crates/keeper/src/{ipc.rs,lib.rs}` — 2 commands + `to_ipc_error` non-retriable arm + tests; command registration.
- `src/lib/ipc/client.ts` (+ `gen/ReplyPreviewVm.ts`, `gen/TimelineItemVm.ts`) — `sendReply`/`editMessage` wrappers + regenerated bindings.
- `src/lib/stores/composer.ts` — NEW reply/edit-context + selection store.
- `src/components/chat/{message-actions.tsx,message-bubble.tsx,composer.tsx}` — action bar, reply quote + Edited caption + selection ring, reply/edit banner + Esc-preserve + edit prefill.
- `src/components/layout/conversation-pane.tsx` — routing, jump-to-original scroll, keyboard handler.

**Review findings:** 2 patches applied (low: edit-cancel now restores the real pre-edit draft — the parent could not stash the composer-local draft, so preservation moved into `composer.tsx`; medium: the post-await pending-`clear()` is scoped to the dispatched context so a concurrently-started reply/edit survives). 3 deferred (all low, see deferred-work.md): the FR-41 guard is single-file not crate-wide (pre-existing mechanism limit); the composer shows retry-implying copy for non-retriable failures (needs `IpcError` threading); the `ReplyIndex` isn't pruned on removals (dead-but-harmless clickable quote for a removed original — no wrong-jump, `unique_id`s aren't reused). ~12 rejected — notably the jump-highlight/selection-ring "collision" (they sit on different DOM nodes — outer row vs inner bubble), the edit-prefill "race" (no timeline virtualization + synchronous store ⇒ the target is always present at `startEdit`), and the composer-Esc "double-handling" (the composer is a sibling of, not a child of, the scroll region, so its Esc never reaches the list handler). No intent_gap, no bad_spec — the spec held up.

**Verification (all re-run green after the review patches):** `bun run check` (Biome + tsc + vitest: 348 passed), `bun run check:rust` (rustfmt + clippy `-D warnings`: clean), `bun run test:rust` (cargo-nextest: 213 passed; ts-rs regenerates `ReplyPreviewVm` + `TimelineItemVm` with no unexpected drift), `cargo deny check licenses` (ok — no new crate deps). The pre-existing `cargo deny check advisories` finding (unmaintained gtk-rs GTK3 bindings via Tauri/wry) is unchanged and unrelated; `Cargo.lock` is untouched, and the license firewall the spec gates on is green.

**Residual risks:** The end-to-end round-trips are real-second-session manual checks, intentionally not run unattended: a reply arriving as a reply in Element / on a bridged network, jump-to-original scroll, and a received edit re-rendering with "Edited". The `Ready`-embedded reply-body mapping in Rust (sender/display-name/body from a loaded original) is covered only end-to-end — the SDK `Message`/`TimelineItemContent` fields are crate-private, so the pure `reply_preview_from_details` unit tests exercise only the key-resolution and details-unavailable branches (the same constructibility limit that left `item_to_vm` untested before). `followup_review_recommended: false` — the final pass made two localized low/medium UI-state fixes with no security/API/data-model change; the meaningful remaining gate is human manual verification against a real second session (Element).
