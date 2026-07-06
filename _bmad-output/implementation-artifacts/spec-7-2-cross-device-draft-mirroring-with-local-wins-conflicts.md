---
title: 'Story 7.2: Cross-Device Draft Mirroring with Local-Wins Conflicts'
type: 'feature'
created: '2026-07-05'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '3ba3b530b4c646eb92b6a66d3983641f7abe0b48'
final_revision: 'acf264bcad1d02e3b08e4639afff19669188961c'
context: []
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Story 7.1 made composer drafts durable locally, but a draft written on one device is invisible on the user's other devices/clients, so unsent text does not follow the user.

**Approach:** Mirror each local draft to the account as **per-room Matrix account data** under custom type `dev.keeper.draft` (synced, best-effort, debounced) plus a best-effort `Room::save_composer_draft`, keeping the local `drafts` table as the single source of truth (AD-15). Read the mirror when a chat opens and observe live remote edits; on conflict the **local unsent text always wins** and the remote version is offered for one-tap adoption via a quiet chip — local text is never silently destroyed. Mirror failures never touch local persistence; the only symptom is a missing cross-device echo.

## Boundaries & Constraints

**Always:**
- The `drafts` table in `keeper.db` stays the single source of truth; the mirror is a downstream projection, never read back as truth except to offer adoption.
- Mirror write = per-room account data event `dev.keeper.draft` (a Ruma `EventContent`, synced) carrying `{ body, updated_ts }`, **plus** a best-effort `Room::save_composer_draft` (Element-family interop). Both are best-effort: every error is swallowed and logged at `debug`/`warn` (never the body), and can never block or fail local persistence.
- Mirroring runs **off the keystroke path**, debounced/coalesced so bursts of typing do not hammer the homeserver (a cadence looser than the ~200 ms local save).
- On conflict — a remote draft that differs from **non-empty** local unsent text — the local version wins and stays in the composer; the remote version is surfaced via a quiet chip above the composer ("Edited on another device — Use that version") for one-tap adoption. Adoption replaces the composer body with the remote body; it is the only way remote text enters a non-empty composer.
- On chat open with an **empty/untouched** composer and a present remote draft, adopt the remote body into the composer (drafts follow the user across devices). Restore continues to respect 7.1's `restoreConsumed` latch (a late async result never clobbers text the user already touched).
- Clearing a draft (send / emptied composer / account sign-out) clears its mirror best-effort: write the tombstone (`body: ""`) account-data event and `Room::clear_composer_draft`. An empty-body mirror reads back as "no remote draft".
- Observe live remote edits by registering a `dev.keeper.draft` room-account-data event handler per account in `activate()` (mirroring the existing `archive_handler`/`redaction_handler` registration), feeding a process broadcast that a `draft_mirror_subscribe` channel relays to the frontend; store the handler handle on `AccountHandle` so account teardown drops it.
- All Matrix logic stays in Rust; the frontend only calls thin commands and renders view models (architecture invariant).

**Block If:**
- Writing a synced custom `dev.keeper.draft` room-account-data event, or registering its event handler, cannot be done without adding an AGPL/GPL dependency or an `unsafe` block (cargo-deny / `unsafe_code = "deny"`). HALT `blocked`.
- Making the mirror non-destructive to local persistence would require moving a blocking/synchronous network or account-data write onto the keystroke/local-save path (best-effort + debounce + swallow cannot satisfy it). HALT `blocked`.

**Never:**
- No change to the send/dispatch path, `send.rs`, or `SendQueue` (Story 7.4); no Approval Pane or cross-account draft-list UI (Story 7.3).
- Never let remote text overwrite non-empty local unsent text automatically, and never delete or truncate local text on any conflict — local always wins; adoption is user-initiated.
- Never use `updated_ts` (or any timestamp) to pick remote over non-empty local as the winner; the winner rule is purely local-wins. `updated_ts` is informational/forward-scaffolding only.
- Never let a mirror error surface as a blocking dialog or affect local persistence; the only permitted symptom is the absent cross-device echo (graceful per-feature degradation on partial servers, OQ-3).
- Never mirror synchronously on the keystroke path; never add a new AGPL/GPL dep or `unsafe`; never log draft bodies.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Type where mirroring is supported | Composer body `B` saved for `(a,r)` | After the mirror debounce, `dev.keeper.draft` account data `{body:B,...}` + `save_composer_draft(B)` written; another device receives it | Account-data/composer write fails → swallowed; local row unaffected; no cross-device echo |
| Open chat, empty composer, remote present | Local draft absent for `(a,r)`; remote `{body:R}` | Composer adopts `R` (draft follows the user) | `load_remote_draft` fails → composer falls back to local (empty); no crash |
| Open chat, local text differs from remote | Local `L` (non-empty), remote `R≠L` | Composer keeps `L`; conflict chip shown; tapping "Use that version" sets composer to `R` | Load failure → no chip, local `L` retained |
| Open chat, local equals remote | Local `L`, remote `L` | No chip; local retained | n/a |
| Live remote edit while composer open | Composer has local `L` (non-empty); remote edit `R≠L` arrives via subscription | Conflict chip appears for one-tap adoption; local `L` untouched | Subscription drop → no live chip; next open re-reconciles from account data |
| Live remote edit, empty untouched composer | Composer empty and `restoreConsumed` false; remote `R` arrives | Composer adopts `R` | If user has typed (`restoreConsumed`), no auto-adopt — chip only |
| Clear/send draft | Draft for `(a,r)` cleared locally | Tombstone `{body:""}` account data + `clear_composer_draft`; other device's next open shows no remote draft | Mirror-clear fails → stale remote may transiently re-present on another device; clearing again reconciles (never destroys text) |
| Partial server (no custom account data) | `set_account_data` rejected repeatedly | Local persistence fully intact; drafts stay local-only; only missing echo | All errors swallowed/logged; feature degrades silently (OQ-3) |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/drafts.rs` -- NEW mirror module: `KeeperDraftEventContent` (Ruma `EventContent`, `type = "dev.keeper.draft"`, `kind = RoomAccountData`, fields `body: String`, `updated_ts: i64`); `mirror_draft(room, body)` (best-effort `set_account_data` + `save_composer_draft`, deduped by last-mirrored body per key); `clear_draft_mirror(room)` (tombstone + `clear_composer_draft`); `load_remote_draft(room) -> Option<(String, i64)>` (empty body → `None`); the `DraftMirrorBatch` payload.
- `src-tauri/crates/keeper-core/src/lib.rs` -- add `pub mod drafts;`.
- `src-tauri/crates/keeper-core/src/account.rs` -- `AccountManager` methods `mirror_draft`/`clear_draft_mirror`/`load_remote_draft` (via `room_for`), a `dev.keeper.draft` event handler registered in `activate()` feeding a `broadcast`, its handle stored on `AccountHandle` (like `archive_handler`), and `subscribe_draft_mirror(sink) -> u64` relaying the broadcast (mirror `subscribe_connection_status` lifecycle/reaper).
- `src-tauri/crates/keeper/src/ipc.rs` -- thin commands `mirror_draft`/`clear_draft_mirror` (async, resolve via `state.accounts`), `load_remote_draft -> Option<RemoteDraftVm{body, updated_ts}>`, `draft_mirror_subscribe(channel: Channel<DraftMirrorBatch>)` + `draft_mirror_unsubscribe(id)` (mirror `connection_status_subscribe`).
- `src-tauri/crates/keeper/src/lib.rs` -- register the new commands in `generate_handler!`.
- `src/lib/ipc/gen/*` -- regenerated ts-rs bindings for `DraftMirrorBatch`/`RemoteDraftVm` (generated, never hand-edited; `bindings:check` must be clean).
- `src/lib/ipc/client.ts` -- `mirrorDraft(a,r,body)`, `clearDraftMirror(a,r)`, `loadRemoteDraft(a,r)`, `subscribeDraftMirror(onBatch)`, `unsubscribeDraftMirror(id)`.
- `src/lib/stores/drafts.ts` -- extend with a `remote` map `key -> {body, updatedTs}`, `applyRemote(a,r,body,updatedTs)`, and `useRemoteDraft(a,r)`; keep the presence set unchanged.
- `src/components/chat/composer.tsx` -- mount reconcile (`loadRemoteDraft` alongside `loadDraft`: adopt into empty composer or raise conflict chip), live remote via `useRemoteDraft`, conflict chip above the textarea (below the reply/edit banner) with an adopt action, schedule a debounced `mirrorDraft` in the save path and `clearDraftMirror` in the clear path.
- `src/components/layout/chat-list-pane.tsx` -- in the existing inbox mount effect (that already seeds `listDrafts()`), start one app-lifetime `subscribeDraftMirror` pumping `draftsStore.applyRemote`; unsubscribe on unmount.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/drafts.rs` (+ `lib.rs` `pub mod`) -- define `KeeperDraftEventContent`, `mirror_draft`/`clear_draft_mirror`/`load_remote_draft`, per-key last-mirrored dedupe, and `DraftMirrorBatch`; all mirror I/O best-effort (errors returned to callers that swallow) -- the synced projection + Element interop write.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- `AccountManager::mirror_draft`/`clear_draft_mirror`/`load_remote_draft` via `room_for`; register the `dev.keeper.draft` handler in `activate()` → `broadcast`, store its handle on `AccountHandle`; `subscribe_draft_mirror` relaying the broadcast with the connection-status reaper/lifecycle -- resolve rooms + observe live remote edits.
- [x] `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- add and register `mirror_draft`, `clear_draft_mirror`, `load_remote_draft`, `draft_mirror_subscribe`, `draft_mirror_unsubscribe`; add `RemoteDraftVm`/`DraftMirrorBatch` ts-rs exports -- expose the mirror to the UI.
- [x] `src/lib/ipc/client.ts` -- `mirrorDraft`/`clearDraftMirror`/`loadRemoteDraft`/`subscribeDraftMirror`/`unsubscribeDraftMirror` wrappers -- typed IPC surface.
- [x] `src/lib/stores/drafts.ts` -- add the `remote` map, `applyRemote`, and `useRemoteDraft`; keep the presence set -- feeds live conflict detection.
- [x] `src/components/chat/composer.tsx` -- mount reconcile (adopt-into-empty / conflict-chip), live `useRemoteDraft` reconcile, conflict chip + adopt action, debounced `mirrorDraft` on save, `clearDraftMirror` on clear -- the local-wins UX + mirror triggers.
- [x] `src/components/layout/chat-list-pane.tsx` -- start the single app-lifetime `subscribeDraftMirror` → `applyRemote` in the inbox mount effect; unsubscribe on unmount -- live remote stream.
- [x] Rust unit tests (`drafts.rs`) -- `KeeperDraftEventContent` (de)serialization round-trip, empty-body tombstone → `load_remote_draft` `None`, dedupe skips an identical re-mirror -- covers the mirror I/O matrix DB/serde cases.
- [x] Frontend tests -- composer adopt-into-empty, local-wins conflict chip + adopt, no chip when equal, live-edit chip; drafts store `applyRemote`/`useRemoteDraft`; chat-list-pane subscription seed/cleanup -- covers the UI edge cases.

**Acceptance Criteria:**
- Given a draft written or edited on device A for `(account, room)`, when mirroring is supported and the debounce elapses, then device B receives the draft as `dev.keeper.draft` room account data and (with an empty composer) shows it on opening that chat.
- Given non-empty local unsent text and a differing remote draft (on open or arriving live), when reconciliation runs, then the local text is retained and a quiet "Edited on another device — Use that version" chip offers the remote for one-tap adoption; local text is never overwritten without that tap.
- Given a draft is sent, emptied, or its account signed out, when the clear runs, then the mirror is tombstoned so other devices stop showing it (best-effort).
- Given a homeserver that rejects the custom account-data write (partial server), when mirroring fails, then local draft persistence and restore are fully unaffected and the only symptom is the absent cross-device echo.

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 6: (high 0, medium 3, low 3)
- defer: 1: (low 1)
- reject: 13: (high 0, medium 0, low 13)
- addressed_findings:
  - `[medium]` `[patch]` Send-path mirror race: `send()` cancelled the local save timer before `await onSend` but left `draftMirrorTimer`/`pendingMirror` armed, so a mid-send flush could write the draft to `dev.keeper.draft` account data and reorder after the post-send tombstone (`clearPersistedDraft → clearMirror`), resurrecting a sent draft on other devices. Now cancels the mirror timer + nulls `pendingMirror` at the same point as the local timer (a failed non-edit send re-schedules the mirror in the catch). Added regression test. `composer.tsx`.
  - `[medium]` `[patch]` Self-echo false conflict chip: room account data is account-level and the SDK echoes a device's own write back to it, so after type→pause(mirror fires)→resume-typing the composer offered the user their own stale text as "Edited on another device" with no second device present. Added an `origin` device-id to `KeeperDraftEventContent`, stamped on every mirror/tombstone write, and `register_draft_handler` now drops events whose `origin` equals its own device id. `drafts.rs`, `account.rs`.
  - `[medium]` `[patch]` Edit-mode contamination: the live-reconcile effect and `adoptRemote` had no edit-mode guard (unlike the keystroke persist path), so a remote draft arriving mid-edit raised the chip over the edit body and adopting it overwrote + persisted/mirrored the edit as the room draft. The live effect now bails (and withdraws any offer) in edit mode, the chip is not rendered while editing, and `adoptRemote` is guarded via a `pendingModeRef`. Added regression test. `composer.tsx`.
  - `[low]` `[patch]` Best-effort violation: `mirror_draft`/`clear_draft_mirror` used `?` on `save_composer_draft`/`clear_composer_draft`, propagating a local-interop failure even after the synced write landed. Now swallowed-and-logged (`warn`, never the body) so a failed interop write never fails an already-landed synced mirror. `drafts.rs`.
  - `[low]` `[patch]` `load_remote_draft` propagated a deserialize error on malformed/partial `dev.keeper.draft` account data; now treated as "no remote draft" (unreadable → absent), keeping local text authoritative. `drafts.rs`.
  - `[low]` `[patch]` Doc-comment fix: the draft-mirror relay claimed a self-reaping lifecycle it does not perform (it ends on sender-drop and its handle is cleared by the eventual unsubscribe; the map is bounded to one app-lifetime sub). Corrected both doc sites. `account.rs`.
- deferred (appended to `deferred-work.md`): (1) `[low]` `LAST_MIRRORED` dedupe map is process-wide and never pruned on account sign-out/teardown — unbounded (one entry per `(account,room)` ever mirrored) over a very long session; no functional break because the server state matches the stale entry, so a dedupe-skip is correct.
- rejected (all low; noise / by-design / unreachable / self-heals): live-reconcile not re-firing on local typing divergence (by-design — the chip surfaces *remote* changes, not local divergence); `onChange` chip dismissal only on exact equality (the standing offer is correct until adopt/remote-change); a `Lagged` broadcast dropping a tombstone (self-heals on the next open reconcile; local-wins means no data loss); StrictMode/dev double-subscribe in `chat-list-pane` (dev-only, unsubscribed on cleanup); backend double-subscribe distinct ids (harmless dup `applyRemote`); `flushMirror` mirroring to the wrong room on rapid switch (refs are captured stably, composite-keyed remount); auto-adopt into a composer with a pending attachment (narrow; a remote body as a caption is not clearly wrong); space-delimited store key collision (re-raise of a 7.1-rejected finding — ULID account ids + Matrix `!local:server` room ids contain no spaces); `AccountManager` teardown not aborting the relay (self-terminates on sender drop); `clear_draft_mirror` dedupe-skip skipping a redundant tombstone (correct — the shared account-data slot already holds the intended state); `now_ms` returning 0 on a pre-epoch clock (informational `updated_ts`, never consulted for the winner).

### 2026-07-05 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 1, low 1)
- defer: 0
- reject: 16: (high 0, medium 0, low 16)
- addressed_findings:
  - `[medium]` `[patch]` Read-path self-echo gap: the `origin` device-echo filter added in the prior pass guarded only the **live** broadcast handler (`register_draft_handler`); `load_remote_draft` discarded `content.origin`, so on chat open a device could be offered its *own* stale mirror as an "Edited on another device" conflict chip whenever local text had diverged from its last (e.g. never-landed) mirror write — the exact bug class the origin field was introduced to close, surviving on the on-open read path. `load_remote_draft` now runs the same own-device filter (via a new pure `resolve_remote_draft(content, own_device)` helper mirroring the live handler); an own-origin event resolves to `None`, foreign/legacy origins stay adoptable. Added a unit test. `drafts.rs`.
  - `[low]` `[patch]` Sign-out draft-body leak: Story 7.2 added actual draft **bodies** to `draftsStore.remote`, but `useSignOut` never pruned the store (its own `clear()` doc says it should run "on full sign-out"), so a signed-out account's unsent text lingered in process memory and left stale inbox markers. Added `draftsStore.clearAccount(accountId)` (prefix-scoped prune of both `keys` and `remote`, cross-account-safe, same-reference no-op when empty) and called it in `useSignOut`. Added store + hook regression tests. `drafts.ts`, `use-sign-out.ts`.
- rejected (all low; noise / by-design / unreachable / self-heals / already-deferred): live-reconcile effect omitting `draft` from deps so it doesn't re-raise the chip on *local* divergence (by-design — the chip surfaces *remote* changes, and the standing offer is correct until adopt/remote-change); a chip lingering over a composer the user emptied mid-session, and a chip (not auto-adopt) over a just-sent empty composer (both spec-compliant — `restoreConsumed` latches to chip-only once the user has touched the composer); "trim asymmetry" between mirror and local save (factually wrong — `flushMirror` and `flushDraft` both `body.trim()` identically); `own_device` empty disabling the self-filter and a device-id collision swallowing a genuine edit (unreachable — a synced client always has a unique device id); `applyRemote` lacking an `updated_ts` monotonic guard and a reordered/lagged batch regressing the offered body (self-heals on next reconcile; local-wins means no data loss); the app-wide subscription living on `ChatListPane` rather than a true app-root (re-subscribes on remount; the inbox's lifetime tracks the logged-in shell); a failed non-edit send with partial attachment delivery re-mirroring the retained text (correct — the retained draft is genuinely unsent; the "mirror stays tombstoned" claim is wrong, `scheduleDraftSave`→`scheduleMirror` re-arms it); `flushMirror` targeting the wrong room on a rapid switch (composite-keyed remount captures refs stably); `now_ms` sentinels on clock skew (`updated_ts` is informational, never the winner); no retry backoff on a persistently rejecting server (best-effort + debounced; MVP-acceptable graceful degradation, OQ-3); `save_composer_draft` overwriting a pre-existing Element rich/reply/attachment composer draft with plain text (by-design interop — keeper deals only in plain-text drafts); backend double-subscribe / `LAST_MIRRORED` cross-relogin dedupe-skip (idempotent dup, and the stale dedupe entry matches the server-side account-data slot so the skip is correct); `LAST_MIRRORED` unbounded growth (already on the deferred ledger from the prior pass — not re-filed).

## Design Notes

- **Local-wins is structural, not timestamped.** Remote is only ever read to *offer* adoption; the composer's local text is authoritative until the user taps adopt. This makes "never destructive" impossible to violate and avoids clock-skew merge bugs. `updated_ts` rides along for future display/telemetry only.
- **`save_composer_draft` is best-effort local interop.** In matrix-sdk 0.18 it persists to the SDK's local state store (not itself synced), so the synced mechanism is the `dev.keeper.draft` account-data event; the composer-draft write is the additive Element-family interop the epic calls for. Write `ComposerDraft { plain_text: body, html_text: None, draft_type: NewMessage, attachments: [] }` with `thread_root: None`.
- **Dedupe prevents an adopt→save→mirror echo storm.** Skip a mirror write when the body equals the last body mirrored for that key. Reconciliation compares **bodies** (not `updated_ts`), so a re-mirror carrying only a new timestamp is ignored on the other device and the system converges after at most one redundant write.
- **Debounce lives in the composer** (a UI cadence timer, not Matrix logic): schedule `mirrorDraft` on a looser debounce than the 200 ms local save; `clearDraftMirror` fires on the clear path. The Rust `drafts` module owns the actual account-data/SDK writes.
- **Tombstone, not delete.** Account data cannot be truly removed, so clearing writes `{body: ""}`; `load_remote_draft` maps empty body → `None`. A best-effort-failed clear can transiently re-present a cleared draft cross-device — acceptable because it re-*shows* recoverable text, never destroys it.
- **Event handler in `activate()`** mirrors the `archive_handler`/`redaction_handler` precedent exactly (register → store handle on `AccountHandle` → dropped on teardown); the subscription relay mirrors `subscribe_connection_status` (reaper map, self-reaping task).

## Verification

**Commands:**
- `bun run test:rust` -- expected: keeper-core `drafts` mirror tests pass.
- `bun run check:rust` -- expected: fmt + clippy clean (`-D warnings`), no `unsafe`.
- `bun run test` -- expected: composer conflict/adopt, drafts store remote, chat-list-pane subscription tests pass.
- `bun run typecheck` -- expected: no TS errors.
- `bun run lint` -- expected: biome clean.
- `bun run bindings:check` -- expected: `src/lib/ipc/gen` regenerated and committed (new `DraftMirrorBatch`/`RemoteDraftVm`), no stray diff.

**Manual checks (if no CLI):**
- Real-server validation (OQ-3): against a real Beeper/partial-server account, confirm `dev.keeper.draft` round-trips across two keeper instances and that a server rejecting the custom type degrades to local-only with no user-visible error. Deferred to attended validation; unattended runs cannot exercise a live homeserver.

## Auto Run Result

Status: done

**Summary:** Made unsent composer drafts follow the user across devices. Each local draft (the `drafts` table remains the AD-15 source of truth) is projected to per-room Matrix account data under the synced custom type `dev.keeper.draft` `{ body, updated_ts, origin }`, plus a best-effort local `Room::save_composer_draft` (Element interop). A new `keeper-core::drafts` module owns the mirror; a `dev.keeper.draft` event handler registered per account in `activate()` feeds a process broadcast that the app-wide `draft_mirror_subscribe` relay streams to the composer. On chat open (and live via the subscription) the composer reconciles **local-wins**: an empty/untouched composer auto-adopts the remote; a differing non-empty local draft keeps local text and offers the remote via a quiet "Edited on another device — Use that version" chip. Mirroring is debounced (looser than the 200 ms local save), best-effort, and never affects local persistence — a rejecting/partial server degrades to local-only with no user-visible error (OQ-3). No send-path change, no Approval Pane (Stories 7.3/7.4).

**Files changed:**
- `src-tauri/crates/keeper-core/src/drafts.rs` (new) — `KeeperDraftEventContent` (`dev.keeper.draft`, with `origin` device id), `mirror_draft`/`clear_draft_mirror`/`load_remote_draft`, per-key last-mirrored dedupe, `draft_mirror_batch`; unit tests (serde round-trip incl. origin/legacy default, tombstone→None, batch mapping, dedupe).
- `src-tauri/crates/keeper-core/src/lib.rs` — `pub mod drafts;`.
- `src-tauri/crates/keeper-core/src/vm.rs` — `RemoteDraftVm`/`DraftMirrorBatch` ts-rs view models.
- `src-tauri/crates/keeper-core/src/account.rs` — `AccountManager` `mirror_draft`/`clear_draft_mirror`/`load_remote_draft`, `subscribe_draft_mirror`/`unsubscribe_draft_mirror`, `register_draft_handler` (drops this device's own echo by `origin`), broadcast + `AccountHandle.draft_handler`.
- `src-tauri/crates/keeper/src/ipc.rs`, `lib.rs` — thin commands `mirror_draft`/`clear_draft_mirror`/`load_remote_draft`/`draft_mirror_subscribe`/`draft_mirror_unsubscribe`, registered.
- `src/lib/ipc/client.ts` — `mirrorDraft`/`clearDraftMirror`/`loadRemoteDraft`/`subscribeDraftMirror`/`unsubscribeDraftMirror`.
- `src/lib/ipc/gen/DraftMirrorBatch.ts`, `RemoteDraftVm.ts` (generated).
- `src/lib/stores/drafts.ts` — `remote` map + `applyRemote` + `useRemoteDraft` (presence set unchanged).
- `src/components/chat/composer.tsx` — mount + live reconcile (adopt-into-empty / local-wins chip / edit-mode-inert), debounced `mirrorDraft` on save + `clearDraftMirror` on clear, send cancels the queued mirror.
- `src/components/layout/chat-list-pane.tsx` — app-lifetime `subscribeDraftMirror` → `applyRemote`.
- Test files: `composer.test.tsx` (mirror adopt/conflict/edit-suppression/send-race), `drafts.test.ts`, `chat-list-pane.test.tsx`, `conversation-pane.test.tsx`.

**Review findings:** Initial pass — 6 patches applied (3 medium: send-path mirror race resurrecting a sent draft cross-device; self-echo false "another device" chip during single-device typing; edit-mode contamination of the persistent draft — all with regression tests. 3 low: best-effort `?` on composer-draft interop writes; `load_remote_draft` malformed-data→None; relay self-reap doc). 0 intent_gap, 0 bad_spec, 1 deferred (unbounded `LAST_MIRRORED` dedupe map), 13 rejected. Follow-up pass (2026-07-05) — 2 more patches: `[medium]` completed the self-echo defense on the on-open read path (`load_remote_draft` now applies the same own-device `origin` filter as the live handler, so a diverged own mirror no longer raises a bogus conflict chip); `[low]` `useSignOut` now prunes `draftsStore` (new `clearAccount`) so a signed-out account's unsent draft bodies don't linger in memory. 0 intent_gap, 0 bad_spec, 0 new defers, 16 rejected (all low). See Review Triage Log.

**Verification:** `bun run check:rust` clean (fmt + clippy `-D warnings`, no `unsafe`); `bun run test:rust` 612 passed (+1); `bun run typecheck` clean; `bun run lint` clean (220 files); `bun run test` 719 passed (74 files, +3 regression); `bun run bindings:check` clean (no stray binding diff).

**Follow-up review recommended:** false — the follow-up pass made only two localized, low-consequence fixes: the read-path `origin` filter is symmetric with the already-reviewed live-path filter and fully unit-tested, and the sign-out prune is trivial store plumbing with regression tests. Nothing broad, risky, or behavior-shifting remains to warrant another independent pass.

**Residual risks:** Mirror is best-effort — a partial/rejecting server (OQ-3, unverifiable in an unattended run) silently degrades to local-only; needs the manual real-Beeper round-trip check. Room account data is a single shared per-room slot (account-level), so two devices editing the same room share one mirror; local-wins + the adoption chip handle the read-side, and a best-effort-failed tombstone can transiently re-present a cleared draft cross-device (re-shows recoverable text, never destroys). `LAST_MIRRORED` grows unbounded over a very long session (deferred).
