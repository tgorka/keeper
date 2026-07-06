# Epic 7 Context: Drafts & Approval Pane — The Airlock

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

This epic makes unsent text a first-class, durable object rather than a throwaway UI string. Everything a user types is persisted locally the instant it is entered, survives chat switches, restarts, and crashes, mirrors across devices without ever clobbering local text, and is surfaced in a single cross-account Approval Pane where each pending draft is edited, approved, or discarded. It is guarded by the product's hardest invariant — the "airlock": keeper can never dispatch anything the user did not explicitly approve. This deliberately separates the act of writing from the act of sending, giving the user a single reviewable surface before anything leaves the device, and it establishes the send-path guarantee that all future agent-proposal features must honor.

## Stories

- Story 7.1: Persistent Per-Chat Drafts
- Story 7.2: Cross-Device Draft Mirroring with Local-Wins Conflicts
- Story 7.3: Approval Pane
- Story 7.4: Explicit-Approval Invariant — Enforced and Tested

## Requirements & Constraints

- **Drafts persist per chat, instantly and durably.** Composer text must survive chat switches, force-quit, and crashes, and be restored in the same chat's composer on return/relaunch. Chats with pending drafts are visibly marked in the unified inbox; sending or clearing a composer removes the draft and its marker. Persistence runs on the keystroke path, which must stay within the composer input-latency budget (< 16 ms/frame) — persist without blocking typing.
- **Cross-device mirroring is best-effort, never destructive.** Drafts mirror to the account so they follow the user across devices/clients where supported. Editing a draft updates the mirror. On conflict (the draft changed on another device while local unsent text exists), the local version always wins and the remote version is offered for one-tap adoption — local text is never silently destroyed. Mirror failures (e.g. partial-server gaps, tracked as an open question against real Beeper accounts) must not affect local persistence; the only visible symptom is the missing cross-device echo.
- **Approval Pane is a single cross-account surface.** It lists every pending draft across all chats and accounts, grouped by account then chat, each row attributed by chat, network, and account. Per draft the user can edit inline, approve (send), or discard. Approve dispatches through the normal send pipeline (honoring the Undo-Send Window once Epic 8 lands); discard removes the draft locally and from mirrored account data with a short undo affordance. Reachable via sidebar entry (amber count badge), Command Palette, and a dedicated shortcut.
- **Explicit-approval invariant is a product-level guarantee, not an implementation detail.** No background, scheduled, automated, or bulk dispatch path may exist in MVP. There must be exactly two user-initiated dispatch triggers and no programmatic API through which a draft can be sent. This is enforced by code and tests, and documented as the binding contract for future agent-proposal features (agents may propose; only the user approves). Introducing any unattended send path requires a new planning-level decision.
- **No approve-all / select-all-and-send affordance in MVP.** Approving is deliberately per-draft.

## Technical Decisions

- **Local truth, mirrored (AD-15).** The `drafts` table in `keeper.db` is the single source of truth. Mirroring writes to per-Room Matrix account data under the custom type `dev.keeper.draft`, debounced and best-effort. Additionally write `Room::save_composer_draft` for Element-family client interop. On conflict, local unsent text wins and the remote version is surfaced for one-tap adoption. The Approval Pane is a **cross-account query over `drafts` + pending `outbox` rows**.
- **Single dispatch gate (AD-13).** The only path into any account's `SendQueue` is `send::submit(text|draft, trigger)` with `trigger ∈ {ComposerSend, ApprovalPaneApprove}`. No other public API may dispatch. Approval-Pane approve uses the `ApprovalPaneApprove` trigger — the second and last legal trigger. Approval inserts into the `outbox` table with a dispatch-time offset for the Undo-Send Window (Epic 8); the invariant and gate exist independently of the window.
- **Story 7.4 is an audit + test story.** Rust tests must assert both that the two triggers dispatch and that no other public API can cause dispatch. The contract must be documented in `keeper-core::send` rustdoc as binding on future agent-proposal features.
- **Data locations.** `keeper.db` (WAL mode) holds drafts, outbox, settings, and registries. Draft data lives only in `keeper.db` and mirrored account data — logout/account-teardown semantics follow the existing keeper.db conventions, not the per-account SDK dir.
- **State stores.** Draft/approval state is a dedicated Zustand vanilla store domain fed by Rust-streamed diffs; the store holds streamed data plus ephemeral UI state only. The core logic lives in a `drafts` module (drafts store, account-data mirror, approval queries) alongside the `send` module.

## UX & Interaction Patterns

- **Held-amber semantics.** Amber (the "held"/airlock accent) means exactly one thing: *written, not sent*. It marks draft rows, the Approval Pane count badge, and the Undo-Send countdown — never hover, chrome, or emphasis.
- **Draft marker in inbox.** Chat rows with a pending draft show a pencil glyph plus a "Draft" prefix in the preview line.
- **Approval Pane layout.** Sidebar entry with amber count badge (shortcut `⌘3`; also via `⌘K`). Rows grouped by account then chat under section-label headers; each row shows chat + network badge + account hue, draft preview, and age. `Enter` opens the inline editor; `⌘Enter` approves (send); `⌘⌫` discards with a 5 s undo toast. Layout reserves a leading proposer-attribution column for post-MVP agents; MVP renders "You" silently. Empty state: "Nothing waiting. Drafts you write stay here until you approve them — nothing sends without you."
- **Cross-device conflict chip.** When a remote draft edit arrives while local unsent text exists, show a quiet chip above the composer: "Edited on another device — Use that version" for one-tap adoption.
- **Privacy surfacing.** Settings → Privacy and the Approval Pane empty state both carry "Nothing sends without you." No surface anywhere may offer scheduled, background, or bulk dispatch.
- **Accessibility.** Roving tabindex within the Approval Pane, visible focus ring on every focusable, and universal `Esc` semantics apply.

## Cross-Story Dependencies

- **7.1 → 7.2, 7.3.** Persistent local drafts are the foundation; mirroring and the Approval Pane both build on the `drafts` table.
- **7.3 → 7.4.** The invariant story audits and tests the send path exercised by the Approval Pane.
- **Depends on Epic 3 (composer complete)** for the composer surface drafts attach to.
- **Interacts with Epic 8 (Undo-Send).** Approve honors the Undo-Send Window once Epic 8 lands; the outbox/dispatch-gate model (AD-13) is shared, but Epic 7's invariant and dispatch gate must stand on their own before the window exists.
- **Depends on real-server validation (open question OQ-3):** the `dev.keeper.draft` account-data path must be verified against a real Beeper/partial-server account and degrade per-feature with disclosure.
