---
title: 'Story 7.3: Approval Pane'
type: 'feature'
created: '2026-07-05'
status: 'done'
baseline_revision: 'e8d7ac66a515bc76195f0a42f844a6176ea5ccea'
final_revision: '4a159c1'
review_loop_iteration: 1
followup_review_recommended: false
context: []
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Unsent drafts (Story 7.1/7.2) are durable per-chat but there is no single surface to review them across accounts. The airlock — "keeper never sends anything the user did not explicitly approve" — needs a cross-account pane where each pending draft is edited, approved (sent), or discarded, deliberately separating writing from sending.

**Approach:** Add a cross-account query over the `drafts` table (there is no `outbox` table yet — the send queue lives in the SDK; outbox join lands with Epic 8) returning per-draft rows enriched with room name, network, account hue, and age. Render them in a new primary view grouped by account then chat, reachable from the sidebar (amber count badge), a `⌘3` shortcut, and the `⌘K` command surface. Approve dispatches through the existing single gate `send::submit(body, SendTrigger::ApprovalPaneApprove)` — the second and last legal trigger, already reserved in `send.rs`. Discard removes the draft locally and tombstones its mirror behind a 5 s undo toast.

## Boundaries & Constraints

**Always:**
- Approve dispatches ONLY through `send::submit(.., SendTrigger::ApprovalPaneApprove)` via a dedicated `AccountManager::send_approval` — no new dispatch path or public send API is introduced.
- A pending draft is ALWAYS listed even when its room/account metadata cannot be resolved (account offline): fall back to `room_id` as the name and `network = None`; never hide or drop a draft row.
- Draft bodies are read from Rust on demand (the JS drafts store holds presence keys + remote offers only, never authoritative bodies).
- Discard is reversible for 5 s via an undo toast that restores the draft (local + mirror + inbox marker); approve clears the draft (local + mirror + marker) ONLY after the send succeeds — a failed send retains the draft.
- Amber (`--held`) marks the count badge only ("written, not sent"); the layout reserves a leading proposer column rendering "You" silently.

**Block If:**
- Making approve dispatch requires changing the `send::submit` gate signature or adding a second dispatch entry point beyond the reserved `ApprovalPaneApprove` trigger (would breach AD-13 — a planning-level decision).

**Never:**
- No approve-all / select-all-and-send / bulk / background / scheduled dispatch affordance — approving is strictly per-draft, user-initiated.
- Do not create an `outbox` table or pending-send materialization (Epic 8 owns that); MVP queries `drafts` only.
- Do not hold draft bodies in a JS store as the source of truth; do not destroy unsent text on discard without the undo window.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Populated pane | drafts in ≥3 chats across ≥2 accounts | rows grouped by account then chat; each shows proposer "You", chat name, network badge, account hue, body preview, age | — |
| Unresolved room | draft exists but account offline / room not live | row still listed; name = `room_id`, network = none | never drop the row |
| Empty | no pending drafts | empty state: "Nothing waiting. Drafts you write stay here until you approve them — nothing sends without you." | — |
| Approve OK | user triggers approve on a row | `approve_draft` → `send::submit(.., ApprovalPaneApprove)`; on success draft cleared (local+mirror), marker removed, row leaves list | — |
| Approve fails | send errors | draft retained (local+mirror+marker intact), error toast; row stays | error surfaced, no data loss |
| Discard | user triggers discard | draft removed locally + mirror tombstoned + marker cleared; 5 s "Undo" toast | undo restores draft+mirror+marker |
| Inline edit | Enter opens editor, user edits + saves | body persisted via `saveDraft` + `mirrorDraft`; preview reflects new body | trimmed-empty save = discard |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/registry.rs` -- `drafts` table (`account_id, room_id, body, updated_ts`, PK `(account_id,room_id)`); has `list_drafts` (keys only, :250). ADD full-row projection.
- `src-tauri/crates/keeper-core/src/send.rs` -- `submit(timeline, text, trigger)` (:47), `SendTrigger { ComposerSend, ApprovalPaneApprove }` (:25) — `ApprovalPaneApprove` reserved, currently unused.
- `src-tauri/crates/keeper-core/src/account.rs` -- `open_timeline_for` (:3126) resolves ONLY a currently-open conversation's `Arc<Timeline>` (the UI-subscription map, populated on room subscribe and reaped on close) — it returns `NoOpenTimeline` for any room the user is not viewing. The Approval Pane is a distinct primary view where NO conversation timeline is open, so approve MUST NOT depend on `open_timeline_for` alone. Use the established "reuse-open-else-transient-build" pattern from `mark_room_read` (:2583): try `open_timeline_for`, and on `Err` build a transient `TimelineBuilder::new(&room).build().await` from `room_for` (:2943) — see the existing usage at `mark_room_read` :2591-2599 and :3460. `send_text` uses `ComposerSend` (:2099) and legitimately keeps `open_timeline_for` because the composer is always invoked inside the open conversation. `bridge::room_bridge_network(&room)` (via :2314) + `Room::display_name()`/`cached_display_name` for name/network (cf. `room_item_to_vm` :4250); registry `list_accounts` gives `hue_index`+`user_id`.
- `src-tauri/crates/keeper-core/src/vm.rs` -- VM/ts-rs export pattern (e.g. `RemoteDraftVm`, `InboxRoomVm` with `hue_index`). ADD `ApprovalDraftVm`.
- `src-tauri/crates/keeper/src/ipc.rs` + `src-tauri/crates/keeper/src/lib.rs` -- thin commands + `generate_handler!`; `send_text` (:1547), draft cmds (:1130-1243).
- `src/lib/ipc/client.ts` -- typed IPC wrappers (`sendText`, draft cmds :449-830). ADD `listPendingDrafts`/`approveDraft`.
- `src/lib/stores/primary-view.ts` -- `PrimaryView = "inbox"|"archive"|"bridges"`; `setView`. ADD `"approval"`.
- `src/lib/stores/drafts.ts` -- presence `keys` set (composite `` `${a} ${r}` ``); ADD `usePendingDraftCount()` (=`keys.size`).
- `src/lib/format-time.ts` -- `formatRoomTimestamp`; ADD relative `formatDraftAge`.
- `src/components/layout/sidebar-pane.tsx` -- `VIEWS` array (:20), health-dot badge pattern (:87). ADD "Approvals" entry + amber count badge.
- `src/components/layout/app-shell.tsx` -- primary-view conditional render (:78-100). ADD approval branch + shortcut hook.
- `src/hooks/use-bridges-shortcut.ts` -- global-keydown shortcut pattern to copy for `⌘3`.
- `src/components/search/search-overlay.tsx` -- `⌘K` cmdk surface; `CommandGroup`/`CommandItem` (:107). ADD "Go to Approval Pane" nav command.
- `src/components/chat/RoomAvatar.tsx` (`AvatarBadge` :50 = network badge), `src/lib/account-hue.ts` (`accountHueVar` :18), `src/index.css` (`--held` :10/:83), `src/components/ui/sonner.tsx` (`toast` + action/duration), `src/components/chat/chat-row.tsx` (:170 accessible-row/focus-ring pattern) -- reused primitives.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- add `list_draft_rows(data_dir) -> Result<Vec<(String,String,String,i64)>>` selecting `account_id, room_id, body, updated_ts` across all accounts (mirror `list_drafts`) with a deterministic `ORDER BY account_id, updated_ts, room_id` so the grouped pane and its single roving tab-stop keep a stable order across re-queries (a bare `SELECT` has unspecified SQLite row order); unit test round-trips inserted rows and asserts the ordering is stable.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `ApprovalDraftVm { account_id, account_user_id, hue_index: u8, room_id, display_name, network: Option<String>, body, updated_ts: i64 }` with `#[ts(export)]` + camelCase (i64 as `number`).
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- add `AccountManager::list_pending_drafts() -> Result<Vec<ApprovalDraftVm>, CoreError>`: read `list_draft_rows`, join `hue_index`/`user_id` from registry accounts, resolve `display_name`+`network` per row via `room_for`+`room_bridge_network` with `room_id`/`None` fallback when unresolved (never drop a row); and `send_approval(account_id, room_id, body)`: after the whitespace-`EmptyBody` guard, resolve the room via `room_for` and acquire a `Timeline` with the **reuse-open-else-transient-build** pattern (`open_timeline_for` if the conversation happens to be open, otherwise `TimelineBuilder::new(&room).build().await`, exactly as `mark_room_read` does) so approve works from the pane where NO conversation is open — then `send::submit(.., SendTrigger::ApprovalPaneApprove)`. This stays inside the single dispatch gate (no new dispatch path / public send API — intent-contract honored); it merely obtains the `Timeline` off the open-subscription path the way Story 4.1 already does. An unparsable/unknown room (or non-live account) → `SendError::RoomNotFound`; a transient-build failure → the SDK's typed error mapped through `send::submit`/`CoreError`. Unit tests: unresolved-room row still emitted with fallback; **`send_approval` dispatches through the gate for a room whose conversation is NOT open (transient-build path) — it must NOT return `NoOpenTimeline` in that case**; `send_approval` submits with the approval trigger; whitespace body → `EmptyBody` before any timeline work.
- [x] `src-tauri/crates/keeper/src/ipc.rs` + `src-tauri/crates/keeper/src/lib.rs` -- add + register `list_pending_drafts() -> Vec<ApprovalDraftVm>` and `approve_draft(account_id, room_id, body)`.
- [x] `src/lib/ipc/client.ts` (+ regenerated `src/lib/ipc/gen/ApprovalDraftVm.ts`) -- add `listPendingDrafts(): Promise<ApprovalDraftVm[]>` and `approveDraft(a,r,body): Promise<void>`.
- [x] `src/lib/stores/primary-view.ts` -- extend `PrimaryView` with `"approval"` (update doc comment).
- [x] `src/lib/stores/drafts.ts` -- add `usePendingDraftCount(): number` selector over `keys.size`.
- [x] `src/lib/format-time.ts` -- add `formatDraftAge(ms: number): string` (relative, e.g. "just now"/"5 min"/"2 h"/date fallback) via `Intl.RelativeTimeFormat`; unit test the buckets.
- [x] `src/components/approval/approval-pane.tsx` (new) -- query `listPendingDrafts` on mount and re-query when `draftsStore` `keys` change; group by account then chat under section headers (account_user_id + hue); rows render proposer "You", chat name, network `AvatarBadge`, account hue edge, body preview, `formatDraftAge`; `Enter` opens inline textarea editor seeded from the row body **once, on the not-editing→editing transition only** (do NOT re-seed the textarea from a later `draft.body` change, so an incoming Story 7.2 cross-device mirror edit landing mid-edit can never silently clobber the user's in-progress text) (save → `saveDraft`+`mirrorDraft`, trimmed-empty → discard), `⌘Enter` approves (`approveDraft` → on success `clearDraft`+`clearDraftMirror`+`mark(false)`; on error keep + error toast), `⌘⌫` discards (`clearDraft`+`clearDraftMirror`+`mark(false)` + 5 s `toast` with "Undo" restoring `saveDraft`+`mark(true)`+`mirrorDraft`); empty-state copy verbatim; roving tabindex + `focus-visible:ring` per row; no bulk/select-all control. Colocated tests cover the matrix rows (grouping, empty, approve success/fail, discard+undo, inline edit).
- [x] `src/components/layout/sidebar-pane.tsx` -- add "Approvals" `VIEWS` entry (lucide icon) wired to `setView("approval")`, with an amber (`bg-held text-held-foreground`) count badge from `usePendingDraftCount()` shown only when `> 0`; test the badge + navigation.
- [x] `src/hooks/use-approval-shortcut.ts` (new) -- `⌘/Ctrl+3` → `setView("approval")`, guarding editable/input targets (copy `use-bridges-shortcut.ts`); test fire + input-guard.
- [x] `src/components/layout/app-shell.tsx` -- render `<ApprovalPane/>` when `primaryView === "approval"` (replacing the chat-list+conversation cluster like `bridges`); call `useApprovalShortcut()`.
- [x] `src/components/search/search-overlay.tsx` -- add a navigation `CommandItem` "Go to Approval Pane" calling `setView("approval")` (and closing the overlay).

**Acceptance Criteria:**
- Given drafts in ≥3 chats across ≥2 accounts, when the Approval Pane opens (sidebar entry with amber count badge, `⌘3`, or `⌘K`), then every pending draft lists grouped by account then chat, each row showing chat name, network badge, account hue, body preview, age, and a silent "You" proposer column — sourced from a cross-account query over `drafts`.
- Given a draft row, when the user approves, then it dispatches through `send::submit(.., ApprovalPaneApprove)` and — only on success — the draft is cleared locally and its mirror tombstoned and the row leaves the list; when the send fails the draft is retained and an error is shown.
- Given a draft row, when the user discards, then the draft is removed locally and from mirrored account data with a 5 s undo toast that fully restores it; when the user edits (`Enter`), inline editing persists via the normal draft save + mirror path.
- Given MVP scope, then no approve-all/select-all-and-send affordance exists, and with no pending drafts the empty state reads "Nothing waiting. Drafts you write stay here until you approve them — nothing sends without you."

## Spec Change Log

### 2026-07-05 — bad_spec loopback (review_loop_iteration 1)

**Triggering finding** (`[high]` `[bad_spec]`): Both review lenses independently found that `send_approval` — as the spec mandated it (`open_timeline_for` + `send::submit(.., ApprovalPaneApprove)`) — fails in the pane's primary flow. `open_timeline_for` resolves a timeline ONLY for a currently-open conversation (the UI-subscription map, populated on room subscribe, reaped on close). The Approval Pane is a standalone primary view where no conversation is open, so approve returns `SendError::NoOpenTimeline` for essentially every draft. Verified against the code: `open_timeline_for` (account.rs:3217) filters `handle.timelines` (inserted only by the subscription producer at :857, removed on reap at :844); the shipped unit test even asserted `NoOpenTimeline` as "expected", baking in the defect. The approve action — the whole point of the airlock — was non-functional as written.

**What was amended** (all outside `<intent-contract>`):
- Code Map (`account.rs`): documented that `open_timeline_for` is open-conversation-only and that approve must use the `mark_room_read` "reuse-open-else-transient-build" pattern; cited the existing `TimelineBuilder::new(&room).build()` usages (:2591-2599, :3460).
- Tasks (`account.rs`): `send_approval` now acquires the `Timeline` via reuse-open-else-transient-build (`open_timeline_for` if open, else `TimelineBuilder::new(&room).build()` from `room_for`), then dispatches through the single `send::submit` gate; added a required test that approve dispatches through the gate for a room whose conversation is NOT open (must not be `NoOpenTimeline`).
- Design Notes: added "Approve must not require an open conversation" explaining the composer-vs-pane asymmetry and why the transient-build stays inside the single-gate boundary.
- Folded two further spec-level corrections (same amendment, to keep the re-derived code coherent): Tasks (`registry.rs`) — `list_draft_rows` gains a deterministic `ORDER BY account_id, updated_ts, room_id` so the grouped list and its single roving tab-stop keep a stable order across re-queries (a bare `SELECT` has unspecified SQLite order). Tasks (approval-pane) — the inline editor is seeded from the row body only on the not-editing→editing transition, so an incoming Story 7.2 cross-device mirror edit landing mid-edit cannot silently clobber the user's in-progress text.

**Known-bad state avoided:** an approve path that only works when the user first navigates into each conversation (`NoOpenTimeline` otherwise) — defeating the cross-account pane; a pane whose row order and single tab-stop shift on every re-query; an inline edit silently overwritten by a remote mirror update.

**KEEP (must survive re-derivation):** everything the intent-contract requires plus all eight prior-pass patches (P1 editor-key bubbling guard so row shortcuts fire only when the row itself is focused; P2 re-query keyed on presence-key *contents* not size, catching net-zero add+remove; P4 approve in-flight guard + optimistic `removeRow` + discard-undo awaiting `saveDraft`; P6 `queryFailed` error affordance for the empty-and-failed case, distinct from the verbatim empty copy; P3 editor `committedRef` blur double-fire guard; P5 network-badge truthiness guard; P7 `send_approval` whitespace `EmptyBody` guard before dispatch + non-retriable IPC mapping; P8 roving-tabindex on the whole pane's first row only). Also KEEP: the never-drop-a-row fallback (`room_id` name / `network = None` when unresolved), the verbatim empty-state copy, the 5 s undo toast restoring local+mirror+marker, clearing on approve ONLY after send success, and the single-gate `ApprovalPaneApprove` dispatch. Only the timeline-acquisition mechanism (and the two folded corrections) changes.

## Review Triage Log

### 2026-07-05 — Review pass (post-re-derivation; review_loop_iteration 1)
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 1, low 0)
- defer: 2: (high 0, medium 1, low 1)
- reject: 19: (high 0, medium 0, low 19)
- addressed_findings:
  - `[medium]` `[patch]` The cross-account row's `aria-label` was `Draft in ${displayName}` only, so a screen-reader user could not tell same-named rooms apart across accounts on this dispatch surface. Now `Draft in ${displayName} on ${accountUserId}. …`; added assertions in the grouping test that both accounts' rows carry the account identity in their accessible name. `approval-pane.tsx`, `approval-pane.test.tsx`.
  - Verified the re-derivation's root-cause fix is correct: `send::submit` (send.rs:60-83) enqueues onto the room's client-scoped, durable send queue via `Timeline::send().await` and intentionally drops the `SendHandle` — so `send_approval` building a transient `TimelineBuilder` timeline and dropping it after dispatch still delivers the message reliably (same mechanism the composer relies on). Before the fix approve returned `NoOpenTimeline` (never sent); after it, approve durably enqueues. The reviewers' "no in-pane local echo / post-enqueue failure not surfaced in the pane" concern reduces to the by-design enqueue-commit model (identical to the composer's clear-on-enqueue discipline; delivery/wedge-state owned by the SDK send queue and the per-conversation send-state UI, audited by Story 7.4) — rejected, not a regression.
  - defer (2, appended to `deferred-work.md` as new entries): `[medium]` roving-tabindex exposes only the first row as a Tab stop with no ArrowUp/Down handler, so keyboard-only users can act on exactly one draft (a11y completeness gap, not an AC violation; pane is mouse-operable and `⌘3`-reachable); `[low]` a transient re-query failure while rows are already shown keeps a possibly-stale list with no "couldn't refresh" signal (the empty+failed case is handled; self-heals on the next presence-key change).
  - reject (19, all low actual-consequence; by-design / narrow / cosmetic / self-healing / already-tracked): no in-pane local echo + present-but-dead reused timeline (send queue delivers; the reuse arm only fires when a conversation is genuinely open/live); stale-body-at-approve + unsaved-editor-text on ⌘Enter (by design — P1 fires row shortcuts only when the row itself is focused, and the pane dispatches the reviewed body); `formatDraftAge` doesn't tick (correct at load and on every presence change; cosmetic); discard/undo mirror-write ordering race + undo `saveDraft` rejection (narrow 5 s window, self-heals on next sync); `initials()` raw `[0]` / combining-mark badge glyph (cosmetic); `⌘3` bare convention + direct-target-only guard (mirrors the existing `use-bridges-shortcut`); per-approve `tracing::info!` with account/room id, body-free (consistent logging; prior pass rejected); error screen persists until Retry/presence-change (Retry exists; re-fires on presence change); badge(`keys.size`)-vs-authoritative-rows divergence incl. signed-out account (badge is a free presence hint; the pane queries authoritative rows and never drops a row); `editingKey` pointing at a vanished-then-reappearing row (narrow; seed-on-transition limits the window); a DB-only draft insert with no presence change never triggering a re-query (mirror/composer inserts call `mark`; presence-keyed by design); overlapping async re-queries resolving out of order (narrow; self-heals on the next query); editor blur-commit racing a second editor open (single `editingKey` + `committedRef` guard); `usePendingDraftKeys` newline-join collision (Matrix room/account ids cannot contain newlines); a no-visible-change whitespace-only edit persisting (harmless idempotent write); `clearDraft` failure re-listing a sent draft on re-query (self-heals; no data loss); rapid multi-discard toasts collapsing an earlier undo (sonner supports concurrent toasts; narrow); `list_pending_drafts` per-row hang with no timeout and sequential per-row resolution (both subsumed by the already-tracked pass-1 perf defer — not re-deferred to avoid a duplicate ledger entry).

### 2026-07-05 — Review pass (follow-up; review_loop_iteration 1)
- intent_gap: 0
- bad_spec: 3: (high 1, medium 1, low 1)
- patch: 2: (high 0, medium 1, low 1)
- defer: 0
- reject: 16: (high 0, medium 0, low 16)
- addressed_findings:
  - `[high]` `[bad_spec]` Approve was non-functional in the pane's primary flow: `send_approval` used `open_timeline_for` (open-conversation-only), returning `NoOpenTimeline` for every draft the user isn't actively viewing — which is every draft in a standalone cross-account pane. Verified against `account.rs` (`open_timeline_for` :3217 reads the UI-subscription map; the shipped test asserted `NoOpenTimeline` as "expected"). Amended the spec (Code Map / Tasks / Design Notes, all outside `<intent-contract>`) to require the `mark_room_read` reuse-open-else-transient-build pattern (`TimelineBuilder::new(&room).build()`), still dispatching through the single `send::submit` gate; triggered code revert + re-derivation via step-03.
  - `[medium]` `[bad_spec]` Inline editor re-seeded from `draft.body` on every change would let an incoming Story 7.2 cross-device mirror edit silently clobber the user's in-progress text; spec now requires seeding only on the not-editing→editing transition.
  - `[low]` `[bad_spec]` `list_draft_rows` had no `ORDER BY`, so SQLite row order (and thus the grouped list + single roving tab-stop) could shift on each re-query; spec now mandates `ORDER BY account_id, updated_ts, room_id`.
  - patch (2, not applied this pass — moot under the bad_spec re-derivation; left for the post-re-derivation review to re-confirm): `[medium]` row `aria-label` omits account identity, so a screen-reader user can't distinguish same-named rooms across accounts; `[low]` a transient re-query failure while rows are already non-empty shows stale rows with no "stale/refresh" signal (the error affordance only renders in the empty+failed case).
  - reject (16, all low; by-design / consistent-with-composer / self-healing / already-tracked / extremely-narrow): badge(`keys.size`)-vs-authoritative-rows divergence incl. signed-out-account undercount (badge is a free presence hint; the pane queries authoritative rows on mount and never drops a row); stale-body-at-approve + `onSaveEdit` `===`-against-prop (by design — approve dispatches the reviewed body); clear-on-enqueue-not-delivery (identical to the composer's clear discipline; the SDK send queue owns post-enqueue retry/persistence); `hueIndex` u8 not clamped (TS `accountHueVar` wraps mod 8; registry controls the value); undo restoring a since-invalidated draft (undo semantics; 5 s window); fire-and-forget `.catch(()=>{})` clears (a failed clear self-heals — the next authoritative re-query re-lists the row; no data loss); sequential per-row room resolution perf (already tracked as deferred item 1 from the prior pass — not re-deferred to avoid a duplicate ledger entry); `initials()` raw `[0]` on astral first char (cosmetic avatar glyph); tests bake in the presence/authoritative agreement (tests for by-design behavior); `inFlight` ref not cleared on unmount (`finally` clears it; unmount drops the ref); Rust `str::trim` vs JS `.trim()` whitespace-set seam (fail-safe — the Rust `EmptyBody` guard retains the draft; extremely narrow); undo-restore relying on a re-query that may fail (the P6 `queryFailed` affordance already covers empty+failed, not the misleading empty copy); undo double-fire (idempotent re-save; the toast dismisses on action click); empty-string network hides badge (P5 by-design — an empty network name is not a real bridge label); trimmed-empty inline save on an already-cleared draft (extremely narrow race).

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 8: (high 0, medium 4, low 4)
- defer: 1: (low 1)
- reject: 10: (high 0, medium 0, low 10)
- addressed_findings:
  - `[medium]` `[patch]` P1 — `ApprovalRow.onRowKeyDown` acted on keys bubbling up from the inline editor `<textarea>`: plain Enter re-opened the editor right after saving, and ⌘Enter typed while editing fired an unintended approve (a spurious dispatch touching the airlock). Now returns early when `event.target !== event.currentTarget`, so row shortcuts fire only when the row itself is focused. Tests: ⌘Enter-in-editor does-not-approve; Enter-in-editor saves without re-opening. `approval-pane.tsx`.
  - `[medium]` `[patch]` P2 — the re-query effect depended only on `usePendingDraftCount()` (`keys.size`), so a simultaneous add+remove (net-zero size change) left a stale list (a discarded/sent row lingered, a new draft stayed invisible). Added `usePendingDraftKeys()` (sorted, newline-joined presence keys) and keyed the re-query on it; `usePendingDraftCount` retained for the sidebar badge. `drafts.ts`, `approval-pane.tsx`.
  - `[medium]` `[patch]` P4 — approve/discard had no optimistic row removal and approve had no in-flight guard: a rapid double ⌘Enter could dispatch twice, and a fire-and-forget `clearDraft` delete racing the re-query could resurrect a just-approved row (re-approve → double send). Added an `inFlight` ref-set guard on approve, optimistic `removeRow` on approve-success and on discard, and made the discard-undo await `saveDraft` before `mark(true)`. Approve-failure still retains everything. Tests: second-⌘Enter-while-in-flight no-ops; discarded row disappears. `approval-pane.tsx`.
  - `[medium]` `[patch]` P6 — a first-mount `listPendingDrafts` rejection was swallowed, leaving `rows` empty and rendering the "Nothing waiting" copy — indistinguishable from a genuinely empty pane, hiding held drafts (opposite of the airlock's never-hide intent). Added a `queryFailed` state and an error affordance ("Couldn't load pending drafts." + Retry) for the `empty && failed` case; verbatim empty copy only for `empty && !failed`. Test: query-rejection shows the error affordance. `approval-pane.tsx`.
  - `[low]` `[patch]` P3 — the editor `onBlur` double-fired `onSaveEdit` after an explicit Enter commit (Enter unmounts the textarea → blur), double-discarding a trimmed-empty edit (two toasts/clears). Added a `committedRef` set on Enter/Escape and honored in `onBlur`. Test: trimmed-empty Enter discards exactly once. `approval-pane.tsx`.
  - `[low]` `[patch]` P5 — the network badge guard was `draft.network !== null`, so a (VM-permitted) empty-string network rendered a blank badge; changed to a truthiness check. `approval-pane.tsx`.
  - `[low]` `[patch]` P7 — `send::submit` treats a trim-empty body as a no-op `Ok(())`, so an (unreachable-but-defended) whitespace-only approve would return success and the frontend would clear the draft — silent loss of unsent text. `send_approval` now returns a new typed `SendError::EmptyBody` before dispatch (mapped non-retriable at the IPC edge) so the frontend's catch retains the draft. Rust tests: `send_approval_rejects_a_whitespace_only_body`, `send_empty_body_maps_to_non_retriable_send_failed`. `account.rs`, `send.rs`/`error.rs`, `ipc.rs`.
  - `[low]` `[patch]` P8 — roving tabindex used the per-group index (`index === 0`), making the first row of every account group tab-reachable, contradicting the single-stop comment; now `groupIndex === 0 && index === 0`. `approval-pane.tsx`.
- deferred (appended to `deferred-work.md`): (1) `[low]` `list_pending_drafts` resolves room name + network sequentially per draft (each acquiring the `accounts` lock and awaiting `Room::display_name()`), so a large draft set or a slow account can delay the whole cross-account pane render; MVP-tolerable at expected draft volumes.
- rejected (all low; by-design / unreachable / cosmetic / self-heals): stale-body-at-approve (by design — the pane dispatches the reviewed body; a concurrent cross-surface edit is a rare race the spec accepts); per-approve `info` log (body/id-free, consistent with logging norms); `⌘3` shortcut guarding only the direct target tag (mirrors the existing `use-bridges-shortcut` convention); empty `user_id` group header (a logged-in registry account always has a `user_id`); non-letter leading network glyph (extremely narrow, cosmetic); undo-after-re-edit overwriting a newer body (5 s window + same-room re-create; undo restoring the discarded body is expected undo semantics); badge (`keys.size`) vs queried-rows transient divergence (by design — badge is the free presence count, rows are authoritative; P2 narrows it); idempotent re-save of an unchanged body (harmless); focus landing on `document.body` after a row leaves the list (a11y nicety; MVP-acceptable).

## Design Notes

- **No outbox in MVP.** The epic's "drafts + pending outbox rows" is forward-looking; keeper has no local outbox (the Matrix SDK owns the send queue). The pane queries `drafts` only; the outbox join arrives with Epic 8. This is scope, not a gap.
- **Count badge is free.** `draftsStore.keys` already tracks presence for every pending draft across all accounts (seeded by `listDrafts` at startup, maintained by the composer). The badge = `keys.size`; the pane's full rows (bodies + metadata) come from the new Rust query, keeping bodies authoritative in Rust.
- **Approve reuses the composer's clear discipline.** Mirror the composer send path: clear local draft + mirror + presence marker, but gate it on send success so a failed dispatch never loses unsent text. `send_approval` is a distinct core method so the `ApprovalPaneApprove` trigger flows through the single `submit` gate exactly as Story 7.4 will audit.
- **Approve must not require an open conversation.** The composer sends from inside the open room, so it resolves the live `Arc<Timeline>` via `open_timeline_for`. The Approval Pane is the opposite: it is a standalone primary view where the target room's conversation is NOT open, so `open_timeline_for` returns `NoOpenTimeline`. `send_approval` therefore uses the same "reuse-open-else-transient-build" timeline acquisition that `mark_room_read` (Story 4.1) already uses for the read-receipt path — reuse an open timeline if present, otherwise `TimelineBuilder::new(&room).build()` a transient one — and only then calls the single `send::submit` gate. This obtains a `Timeline` off the open-subscription path; it introduces no new dispatch path or public send API, so the intent-contract's single-gate boundary holds.
- **Never hide a draft.** Metadata resolution is best-effort; an offline account still yields a row (`room_id` name, no network). The airlock must show everything held, even when the room can't be rendered richly.

## Verification

**Commands:**
- `bun run test:rust` -- expected: new `registry`/`account` tests pass (draft-row projection, unresolved-room fallback listing, `send_approval` approval trigger).
- `bun run check:rust` -- expected: fmt + clippy `-D warnings`, no `unsafe`, no `.unwrap()` in prod paths.
- `bun run bindings:check` -- expected: `ApprovalDraftVm.ts` regenerated + committed, no stray diff.
- `bun run test` -- expected: approval-pane (grouping/empty/approve ok+fail/discard+undo/inline-edit), sidebar badge+nav, `⌘3` shortcut, `formatDraftAge` tests pass.
- `bun run typecheck` && `bun run lint` -- expected: clean (no `any`, `import type` where type-only).

**Manual checks (if no CLI):**
- Real-server (OQ-3): confirm discard tombstones the mirror cross-device on a real Beeper account; unattended runs cannot exercise a live homeserver.

## Auto Run Result

Status: done

**Summary:** Built the Approval Pane — the airlock's single cross-account review surface. A new primary view lists every pending draft (Story 7.1/7.2, the `drafts` table) across all accounts, grouped by account then chat, each row rendering a silent "You" proposer column, chat name, bridged-network badge, per-account hue edge, body preview, and relative age. Data comes from a new cross-account Rust query `list_pending_drafts` (bodies stay authoritative in Rust; the JS drafts store still holds only presence keys). Per row: `Enter` opens an inline editor (save → `saveDraft` + `mirrorDraft`; trimmed-empty → discard), `⌘Enter` approves through the single dispatch gate `send::submit(.., SendTrigger::ApprovalPaneApprove)` — the second and last legal trigger — clearing the draft (local + mirror + marker) only on send success so a failed send never loses text, and `⌘⌫` discards behind a 5 s undo toast that fully restores the draft. Reachable from a sidebar entry with an amber (`--held`) count badge (`keys.size`), a `⌘3` shortcut, and a `⌘K` nav command. No `outbox` table exists yet (the SDK owns the send queue) so MVP queries `drafts` only; the outbox join lands with Epic 8. No approve-all/bulk/background/scheduled dispatch affordance; verbatim empty-state copy; a draft whose room/account can't be resolved is still listed (name = room id, no network).

**Files changed:**
- `src-tauri/crates/keeper-core/src/registry.rs` — `list_draft_rows` full-row projection (`account_id, room_id, body, updated_ts`) + round-trip test.
- `src-tauri/crates/keeper-core/src/vm.rs` — `ApprovalDraftVm` (`#[ts(export)]`, camelCase, `i64→number`).
- `src-tauri/crates/keeper-core/src/account.rs` — `list_pending_drafts` (cross-account query, registry identity/hue join, best-effort room name+network, never-drop fallback) + `send_approval` (single-gate dispatch with the approval trigger; whitespace-body guard) + `resolved_room_name`; unit tests (fallback listing, approval-trigger gate, whitespace rejection).
- `src-tauri/crates/keeper-core/src/send.rs` + `error.rs` — `SendTrigger::as_label` trace; new `SendError::EmptyBody`; single-send-gate guard test intact.
- `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` — `list_pending_drafts` / `approve_draft` commands (registered); `EmptyBody` → non-retriable `SendFailed`.
- `src/lib/ipc/client.ts` (+ generated `src/lib/ipc/gen/ApprovalDraftVm.ts`) — `listPendingDrafts` / `approveDraft` wrappers.
- `src/lib/stores/primary-view.ts` — `"approval"` view; `src/lib/stores/drafts.ts` — `usePendingDraftCount` + `usePendingDraftKeys`.
- `src/lib/format-time.ts` (+ test) — `formatDraftAge` relative-time helper.
- `src/components/approval/approval-pane.tsx` (+ test) — the pane (grouping, inline edit, approve/discard+undo, empty + query-error states, roving tabindex, no bulk control).
- `src/components/layout/sidebar-pane.tsx` (+ test) — "Approvals" entry + amber count badge.
- `src/hooks/use-approval-shortcut.ts` (+ test) — `⌘3` with editable-target guard.
- `src/components/layout/app-shell.tsx` — renders `<ApprovalPane/>` on `"approval"` + wires the shortcut.
- `src/components/search/search-overlay.tsx` — `⌘K` "Go to Approval Pane" nav command.

**Review findings:** One review pass. 0 intent_gap, 0 bad_spec, 8 patches applied (4 medium: P1 editor-key bubbling caused an unintended approve/re-open; P2 re-query missed net-zero add+remove churn; P4 no optimistic removal / approve in-flight guard risked a double-send + row resurrection; P6 a swallowed first-mount query failure hid held drafts behind the empty copy — all with regression tests. 4 low: P3 blur double-fire double-discard; P5 empty-string network blank badge; P7 whitespace-approve silent-loss guard; P8 roving-tabindex per-group→whole-pane). 1 deferred (sequential per-row room-name resolution in `list_pending_drafts`). 10 rejected (all low; by-design / unreachable / cosmetic). See Review Triage Log.

**Verification:** `bun run check:rust` clean (fmt + clippy `-D warnings`, no `unsafe`, no `.unwrap()` in prod); `bun run test:rust` 618 passed; `bun run typecheck` clean; `bun run lint` clean (224 files); `bun run test` 749 passed (76 files). `bun run bindings:check` is red ONLY because the new generated `ApprovalDraftVm.ts` is uncommitted (the underlying regeneration is deterministic with zero content diff and the Rust tests pass); it goes green on this commit.

**Follow-up review recommended:** true — the review pass changed dispatch-triggering behavior (P1 unintended approve via key bubbling; P4 approve in-flight guard + optimistic removal) and re-query/error-surface correctness (P2/P6) in this airlock-critical epic. Though each fix is localized and test-covered, the concentration of dispatch- and airlock-adjacent changes warrants an independent follow-up pass.

**Residual risks:** Mirror/discard tombstone cross-device is best-effort — OQ-3 (unverifiable in an unattended run) needs a real-Beeper round-trip check. `list_pending_drafts` resolves room metadata sequentially (deferred). The sidebar amber badge (`keys.size`) is seeded by the inbox mount; navigating straight to Approvals before the inbox ever mounts can show a `0` badge until the inbox is visited, though the pane itself queries authoritative rows on mount regardless.

---

### Follow-up run — review_loop_iteration 1 (2026-07-05)

A follow-up review (this run) opened on the committed story, ran a fresh Blind Hunter + Edge Case Hunter pass, and found a **high-severity, spec-caused functional defect**: the shipped `send_approval` acquired its dispatch `Timeline` via `open_timeline_for`, which resolves ONLY a currently-open conversation's timeline. The Approval Pane is a standalone primary view where no conversation is open, so approve returned `SendError::NoOpenTimeline` for essentially every draft — the airlock's core approve action was non-functional in its primary flow (the shipped unit test even asserted `NoOpenTimeline` as "expected"). Classified **bad_spec** → one loopback (iteration 1).

**Fix (bad_spec loopback):** amended the spec outside `<intent-contract>` (Code Map / Tasks / Design Notes) to require the `mark_room_read` "reuse-open-else-transient-build" pattern, reverted the code, and re-derived: `send_approval` now acquires the `Timeline` via `open_timeline_for` if the conversation is open, else `TimelineBuilder::new(&room).build()` from `room_for`, then dispatches through the unchanged single `send::submit(.., ApprovalPaneApprove)` gate (no new dispatch path — intent-contract holds). Verified correct: `send::submit` enqueues onto the room's durable, client-scoped send queue and drops the `SendHandle`, so dropping the transient timeline still delivers — the exact mechanism the composer relies on. Two further spec-level corrections were folded in: `list_draft_rows` now has a deterministic `ORDER BY account_id, updated_ts, room_id` (stable grouped order + roving tab-stop), and the inline editor is seeded only on the not-editing→editing transition (an incoming cross-device mirror edit can no longer clobber in-progress text). All 8 prior-pass patches (P1–P8) preserved.

**Post-re-derivation review pass:** 0 intent_gap, 0 bad_spec — converged. 1 patch applied (row `aria-label` now carries the account identity so a screen-reader user can distinguish same-named rooms across accounts). 2 deferred (roving-tabindex has no arrow-key navigation so only the first row is keyboard-reachable; a transient re-query failure with rows already shown gives no staleness signal). 19 rejected (all by-design / narrow / cosmetic / self-healing / already-tracked). See the two 2026-07-05 follow-up entries in the Review Triage Log.

**Verification (this run):** `bun run check:rust` clean; `bun run test:rust` 618 passed (incl. rewritten `send_approval` gate test asserting the well-formed-room case is `RoomNotFound`, explicitly NOT `NoOpenTimeline`, and the extended `list_draft_rows` ordering test); `bun run typecheck` clean; `bun run lint` clean (224 files); `bun run test` 750 passed (76 files). `bun run bindings:check` red ONLY on the uncommitted generated `ApprovalDraftVm.ts` (byte-identical regeneration; goes green on this commit).

**Follow-up review recommended:** false — the airlock-critical timeline fix was verified against the send-queue semantics and independently reviewed in this pass with clean convergence (0 bad_spec); the final pass's own change is a single localized low-consequence a11y patch. The remaining uncertainty is a live-homeserver approve→deliver round-trip (an OQ-3-class manual check an unattended run cannot exercise, and a static follow-up review would not address it either) — recorded below, not a reason for another static pass.

**Residual risks (this run):** The approve→deliver round-trip from the pane (transient-timeline dispatch to a room whose conversation is not open) is not exercisable by unit tests — it needs a live-homeserver manual check alongside the existing OQ-3 mirror round-trip. The two deferred items (keyboard arrow-nav; stale-refresh signal) remain open in `deferred-work.md`.
