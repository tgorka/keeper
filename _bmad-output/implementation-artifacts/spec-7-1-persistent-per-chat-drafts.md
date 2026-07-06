---
title: 'Story 7.1: Persistent Per-Chat Drafts'
type: 'feature'
created: '2026-07-05'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: 'c29a12dbf28a315b3b309d9037e60ccd75ff34b7'
final_revision: '151362cce0a6bf21c7834092d75ca0acfa93dec7'
context: []
warnings: []
---

<intent-contract>

## Intent

**Problem:** Composer text lives only in ephemeral React state, so switching chats, force-quitting, or crashing silently destroys any half-written message.

**Approach:** Make unsent text durable by persisting it per `(account, room)` to a new `drafts` table in `keeper.db` (the source of truth, AD-15) on the keystroke path, restoring it into the same chat's composer on return/relaunch, and marking inbox rows that carry a pending draft with an amber pencil + "Draft" prefix.

## Boundaries & Constraints

**Always:**
- `keeper.db` `drafts` table is the single source of truth; keyed by `(account_id, room_id)`; WAL, idempotent `CREATE TABLE IF NOT EXISTS` migration in `registry::open`, mirroring the `pins` precedent.
- Persistence on the keystroke path is fire-and-forget and debounced (~200 ms) so keystroke handling stays within the composer frame budget (< 16 ms/frame) — never a synchronous IPC round-trip per keystroke.
- Restore is per `(account, room)`: opening a chat loads its stored body; the composer already remounts per room (keyed by `selectedRoomId`).
- Clearing a draft (successful send, or composer emptied/whitespace-only) deletes its row and its inbox marker.
- Draft markers are cross-account, seeded at startup from a single `list_drafts` over the whole table; amber (`held`) means only "written, not sent" (use the `text-held` token).
- Signing out an account deletes that account's draft rows (extend `registry::delete_account`, exactly as it drops `pins`).

**Block If:**
- Meeting the "instant, durable" persistence guarantee would require a blocking synchronous IPC write on the keystroke path (i.e. fire-and-forget + debounce + unmount flush cannot satisfy it). HALT `blocked`.

**Never:**
- No cross-device mirroring — no Matrix account data, no `dev.keeper.draft`, no `Room::save_composer_draft` (Story 7.2).
- No Approval Pane, no cross-account draft list UI (Story 7.3).
- No change to the send/dispatch path or `send.rs` (drafts are pre-send state; the only touch is deleting a draft after a send succeeds).
- Never store secret material anywhere but the existing keeper.db conventions; never log draft bodies.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Type then switch chat | Composer has body `B` for `(a,r)`; user opens another chat | `B` persisted to `drafts` under `(a,r)`; pending debounce flushed on composer unmount; on return `B` restored | Persist failure swallowed (fire-and-forget); typing never blocked |
| Relaunch with drafts | `drafts` has rows for several `(a,r)` across accounts | Startup `list_drafts` seeds markers; those inbox rows show amber pencil + "Draft"; opening a row restores its body | `list_drafts` failure → no markers, no crash |
| Send succeeds | Composer body sent for `(a,r)` | Draft row for `(a,r)` deleted; marker cleared | On send failure, composer keeps text; row + marker remain |
| Composer emptied / whitespace-only | User deletes all text, or body trims to empty | No row written; any existing row deleted; marker cleared | Delete is idempotent (no row = no-op) |
| Edit-mode prefill active | User enters edit on a message | Edit prefill wins; draft-restore does not clobber edit text; existing edit cancel/restore behavior unchanged | n/a |
| Account signed out | `(a,*)` account removed | All `drafts` rows for `a` deleted | Idempotent |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/registry.rs` -- keeper.db access layer; add `drafts` table to `open()`, add `set_draft`/`get_draft`/`delete_draft`/`list_drafts` (mirror `set_pin`/`get_pins`), extend `delete_account` to drop drafts.
- `src-tauri/crates/keeper/src/ipc.rs` -- add `#[tauri::command]` `set_draft`/`get_draft`/`delete_draft`/`list_drafts`, resolving `data_dir` via `state.platform.data_dir()` and delegating to `registry::*` (mirror existing pins/settings commands).
- `src-tauri/crates/keeper/src/lib.rs` -- register the four commands in `tauri::generate_handler!`.
- `src/lib/ipc/client.ts` -- add wrappers `saveDraft`, `loadDraft`, `clearDraft`, `listDrafts` over `invoke`.
- `src/lib/stores/drafts.ts` -- NEW Zustand vanilla store: presence-only set of draft keys; `applyKeys`, `mark(accountId, roomId, present)`, `clear`; `useHasDraft(accountId, roomId)` selector hook.
- `src/components/chat/composer.tsx` -- accept `accountId`/`roomId` props; restore body on mount, debounced persist on change, clear on send/empty, flush on unmount.
- `src/components/layout/conversation-pane.tsx` -- pass `accountId`/`selectedRoomId` to `<Composer>`.
- `src/components/chat/chat-row.tsx` -- render amber `Pencil` + "Draft" prefix in the preview line when `useHasDraft` is true.
- `src/components/layout/chat-list-pane.tsx` -- seed the drafts store from `listDrafts()` in the inbox mount effect.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- add `drafts(account_id TEXT, room_id TEXT, body TEXT NOT NULL, updated_ts INTEGER NOT NULL, PRIMARY KEY(account_id, room_id))` to `open()`; add CRUD (`set_draft` upsert, `get_draft`→`Option<String>`, `delete_draft` idempotent, `list_drafts`→`Vec<(String,String)>`); drop drafts in `delete_account` -- durable source of truth.
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- unit tests: CRUD roundtrip, upsert overwrite, idempotent delete, `list_drafts` across accounts, `delete_account` drops drafts -- covers the I/O matrix DB cases.
- [x] `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- add and register the four thin commands -- expose CRUD to the UI.
- [x] `src/lib/ipc/client.ts` -- add `saveDraft`/`loadDraft`/`clearDraft`/`listDrafts` -- typed IPC surface.
- [x] `src/lib/stores/drafts.ts` -- NEW presence store + `useHasDraft` -- feeds the inbox marker.
- [x] `src/components/chat/composer.tsx` + `conversation-pane.tsx` -- restore-on-mount, debounced fire-and-forget persist, clear-on-send/empty, unmount flush; thread `accountId`/`roomId` -- persistence + restore.
- [x] `src/components/chat/chat-row.tsx` -- amber pencil + "Draft" preview prefix when a draft exists -- the marker.
- [x] `src/components/layout/chat-list-pane.tsx` -- seed drafts store from `listDrafts()` on inbox mount -- markers survive relaunch.
- [x] Frontend tests: drafts store (`mark`/`applyKeys`/`useHasDraft`), composer persist/restore/clear behavior, chat-row marker rendering -- covers UI edge cases.

**Acceptance Criteria:**
- Given text typed in a chat's composer, when the user switches chats, force-quits, or the app crashes, then on return/relaunch the persisted text is restored in that same chat's composer (source of truth is the `drafts` table).
- Given chats with pending drafts across multiple accounts, when the inbox renders, then each such row shows the amber pencil glyph + "Draft" prefix in its preview line, seeded at startup.
- Given a sent or cleared composer, when the send succeeds or the body becomes empty, then the draft row and its marker are removed.
- Given an account is signed out, then its draft rows are deleted, leaving no orphaned drafts or markers.

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 1, medium 1, low 1)
- defer: 0
- reject: 11: (high 0, medium 0, low 11)
- addressed_findings:
  - `[high]` `[patch]` Editing an existing message overwrote then deleted the room's persistent draft (keystroke-persist and post-send clear both ran in edit mode). Guarded the persist path with `pending?.mode !== "edit"`; edit-send now restores the pre-edit draft and leaves the stored row + marker intact. Added a regression test (`composer.test.tsx`).
  - `[medium]` `[patch]` A send outlasting the ~200 ms debounce could let a flushed `saveDraft` reorder after the post-send `clearDraft`, orphaning a draft row + amber marker on an already-sent chat across relaunch. Cancel the queued debounce before dispatching the send; re-persist the retained body on non-edit send failure.
  - `[low]` `[patch]` `drafts.ts` header cited `AD-9` instead of `AD-15`; corrected.
- rejected (final severity low; not this story's problem or cannot fire): wholesale `applyKeys` re-seed dropping a sub-debounce marker during an account add/sign-out (self-heals from DB on next keystroke/relaunch; a draft younger than the debounce is not yet durable by design); `accountId ?? ""` fallback (moves in lockstep with `roomId`, cannot fire); `${a} ${r}` space-delimited key (ULID / `!room:server` ids contain no spaces); composer `key` omits accountId (room ids globally unique); optimistic marker not reverting on IPC failure (self-heals from DB truth on relaunch); out-of-order typing save/clear (single shared debounce timer; only the last flush fires); load-after-clear / stale `pending` closure (guarded by `draftRef`); StrictMode double-mount flush (cleanup + null `pendingDraft` guard); marker seeded for an archived/filtered/just-signed-out row (`useHasDraft` only lights rendered rows; sign-out drops rows); whitespace-only body via the public IPC (sole caller trims; `body NOT NULL` holds); `updated_ts` write-only (intentional forward scaffolding per spec).

### 2026-07-05 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 1, low 2)
- defer: 3: (high 0, medium 0, low 3)
- reject: 9: (high 0, medium 0, low 9)
- addressed_findings:
  - `[medium]` `[patch]` Composer was keyed `key={selectedRoomId}` alone while a draft is keyed by `(accountId, roomId)`. Switching to a same-`roomId` chat under another account (two accounts in one room) would keep the same Composer instance mounted, leaking account A's unsent text into account B and persisting it under B's key. Keyed on `${accountId}:${selectedRoomId}` to force a remount + fresh restore, matching the `[accountId, selectedRoomId]` keying every other subscription in `conversation-pane.tsx` already uses. NOTE: the prior pass rejected this as "room ids globally unique"; reconsidered because same-`roomId` rows across accounts are possible and the fix is strictly safe (can only add remounts, never remove) and convention-aligning. `conversation-pane.tsx`.
  - `[low]` `[patch]` The mount draft-restore could clobber content established during its async `loadDraft` window — a near-instant send (draft cleared, `draftRef` empty) or entering edit with an empty prefill both slip past the `draftRef.current.length > 0` / stale `pending?.mode` guards, resurrecting a just-sent/edited body. Added a `restoreConsumed` latch set by type/send/edit-prefill so a late restore bails. Strictly additive guard (only prevents clobbering). Removed a now-inert `useExhaustiveDependencies` suppression (the effect now reads only refs). NOTE: prior pass rejected the narrower form as "guarded by `draftRef`"; `draftRef` does not cover the emptied-after-interaction cases. `composer.tsx`.
  - `[low]` `[patch]` `set_draft` IPC docstring falsely claimed it "Upserts the trimmed body" — the Rust layer stores `body` verbatim (the frontend trims). Corrected the docstring to state it stores verbatim and that the frontend trims/deletes-empty. `ipc.rs`.
- rejected (final severity low; not this story's problem or cannot fire): `accountId ?? ""` writing an empty-account row (the Composer renders only when `selectedRoomId !== null`, which implies a non-null `accountId` from the same `selected` object — unreachable); `updated_ts` write-only column (mirrors the `pins` precedent, intentional forward scaffolding); transient stale marker between sign-out and async re-seed (self-heals, marginal); universal `.catch(() => {})` swallowing persist failures (explicit fire-and-forget contract — must never block typing); optimistic marker vs unconfirmed DB write (re-seeded from DB truth on relaunch, by design); attachment-only send's redundant idempotent `clearDraft` (harmless no-op); edit-send not re-`mark`ing the store (marker already correct; divergence case unreachable); wholesale `applyKeys` re-seed racing a live mark (self-heals); whitespace-only stored draft restoring (the save path always trims/deletes-empty, so no such row can be written).

### 2026-07-05 — Review pass (follow-up 2)
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 4: (high 0, medium 1, low 3)
- reject: 11: (high 0, medium 0, low 11)
- addressed_findings:
  - `[low]` `[patch]` The failed-non-edit-send re-persist read the `draft` closure captured at send time, not the live composer text. If the user retyped during the in-flight send and then left the chat, the unmount flush persisted the stale pre-retype body. Switched the catch's re-persist to `scheduleDraftSave(draftRef.current)` (the live ref), so a failed send always re-persists what the composer actually shows. Strictly-safe (identical when no retype occurred). `composer.tsx`.
- deferred (appended to `deferred-work.md` as new entries): (1) `[medium]` in-flight `saveDraft` vs post-send `clearDraft` write-reorder can resurrect a sent draft across relaunch — prior mitigation only cancels the still-queued timer; safe fix (per-key write serialization / Rust stale-write guard) touches the spec-protected send-ordering path and merits attended review; (2) `[low]` `open()` sets no `busy_timeout`, so a contended draft write hits `SQLITE_BUSY` and is silently swallowed (pre-existing registry-wide, surfaced by drafts' typing-cadence writes); (3) `[low]` no size cap on `body` — a multi-MB paste is re-shipped/rewritten per debounce and bloats keeper.db; (4) `[low]` draft *content* stored plaintext at rest deserves a conscious ADR note vs. the app's Keychain/`sdk_encryption` at-rest posture.
- rejected (final severity low; cannot fire, self-heals, or by-design — several re-raise items the two prior passes already rejected with the same rationale): `${a} ${r}` space-delimited key collision (ULID account ids + Matrix `!localpart:server` room ids forbid spaces — unreachable); `accountId ?? ""` empty-account row (Composer renders under `selectedRoomId !== null`, and `accountId`/`roomId` derive from the same non-null `selected` object — unreachable); wholesale `applyKeys` re-seed clobbering a live sub-debounce mark (self-heals from DB on next keystroke/relaunch); universal `.catch(() => {})` with no telemetry (the explicit fire-and-forget contract — must never block typing); `updated_ts` wall-clock / write-only column (intentional forward scaffolding mirroring `pins`); `restoreConsumed` latch not set on paste-attachment (restoring a stored body as a caption under a fresh attachment is not clearly wrong; sub-`loadDraft`-window race); enter-edit-before-restore transient composer-vs-marker divergence (no data loss — the row persists and restores correctly on the next fresh open); cancel-edit not re-marking (subsumed by the prior; self-heals); non-atomic `delete_account` (three sequential deletes on one connection is the pre-existing pins pattern the spec mandates mirroring "exactly"); tests don't exercise IPC failure/reorder modes (test-only, no direct user consequence).

## Design Notes

Keep the DB layer in `registry.rs` (the `pins` precedent for per-`(account,room)` keeper-local state) rather than adding a `keeper-core::drafts` module now — the higher-level drafts module (account-data mirror, approval queries) is genuinely Story 7.2/7.3 scope.

The marker store is presence-only (which `(account, room)` keys have a draft, not bodies) so it stays small; the composer loads its own body via `loadDraft` on mount. Persist example (fire-and-forget, debounced, flush on unmount):

```ts
// composer onChange, after setDraft(next):
scheduleSave(next); // ~200ms debounce -> void saveDraft(a, r, next.trim())
                    // trimmed-empty -> void clearDraft(a, r) + draftsStore.mark(a, r, false)
useEffect(() => () => flushSave(), []); // persist latest on room switch/unmount
```

`list_drafts` returns `Vec<(String,String)>` (serializes to `[accountId, roomId][]`), so no new `ts-rs` binding is needed.

## Verification

**Commands:**
- `bun run test:rust` -- expected: keeper-core registry drafts tests pass.
- `bun run check:rust` -- expected: fmt + clippy clean (`-D warnings`).
- `bun run test` -- expected: drafts store, composer, and chat-row tests pass.
- `bun run typecheck` -- expected: no TS errors.
- `bun run lint` -- expected: biome clean.
- `bun run bindings:check` -- expected: no uncommitted `src/lib/ipc/gen` diff (confirms no unexpected binding drift).

## Auto Run Result

Status: done

**Summary:** Made unsent composer text a durable, per-`(account, room)` object. A new `drafts` table in `keeper.db` (source of truth, AD-15) is written fire-and-forget on a ~200 ms debounce off the keystroke path, restored into the composer on chat open, and dropped on send / clear / account sign-out. Inbox rows with a pending draft show an amber pencil + "Draft" prefix, seeded cross-account at startup. No cross-device mirroring, Approval Pane, or send-path change (out of scope for 7.1).

**Files changed:**
- `src-tauri/crates/keeper-core/src/registry.rs` — `drafts` table + `set_draft`/`get_draft`/`delete_draft`/`list_drafts` CRUD; `delete_account` drops the account's drafts; unit tests.
- `src-tauri/crates/keeper/src/ipc.rs`, `lib.rs` — four thin `#[tauri::command]`s delegating to `registry::*`, registered in the invoke handler.
- `src/lib/ipc/client.ts` — `saveDraft`/`loadDraft`/`clearDraft`/`listDrafts` wrappers.
- `src/lib/stores/drafts.ts` (new) — presence-only Zustand store + `useHasDraft`; `drafts.test.ts` (new).
- `src/components/chat/composer.tsx` — restore-on-mount, debounced fire-and-forget persist, clear-on-send/empty, unmount flush; edit mode never touches the persistent draft.
- `src/components/layout/conversation-pane.tsx` — passes `accountId`/`roomId` to `<Composer>`.
- `src/components/chat/chat-row.tsx` — amber `text-held` `Pencil` + "Draft" preview prefix when a draft exists.
- `src/components/layout/chat-list-pane.tsx` — seeds the drafts store from `listDrafts()` on inbox mount.
- Test files updated: `composer.test.tsx` (incl. new edit-preservation regression), `chat-row.test.tsx`, `chat-list-pane.test.tsx`, `conversation-pane.test.tsx`.

**Review findings (initial pass):** 3 patches applied (1 high: edit mode overwrote/deleted the persistent draft — guarded persist + edit-send restores pre-edit draft, keeps the row; 1 medium: send-vs-debounce ordering race orphaning a draft row/marker — cancel debounce before send, re-persist on failure; 1 low: `AD-9`→`AD-15` doc fix). 0 intent_gap, 0 bad_spec, 0 deferred, 11 rejected.

**Review findings (follow-up pass, 2026-07-05):** 3 patches applied (1 medium: Composer keyed on `roomId` alone leaked/mis-persisted a draft when switching to a same-`roomId` chat under another account — now keyed on `${accountId}:${selectedRoomId}` to force a fresh remount+restore; 1 low: mount draft-restore could clobber content established during its async `loadDraft` window — added a `restoreConsumed` latch; 1 low: `set_draft` docstring falsely claimed it trims — corrected). 0 intent_gap, 0 bad_spec, 3 deferred (per-op connection churn; pre-edit-draft-not-re-persisted-on-edit-send; `temp_dir` nanosecond-collision test flake — all pre-existing/narrow), 9 rejected (see Review Triage Log). The two medium/low correctness patches reverse findings the initial pass rejected; both are strictly-safe additive hardening that close real gaps in the prior rationale. Follow-up review recommended: an independent pass should sanity-check the composite-key reversal.

**Verification (follow-up pass):** `bun run test` 699 passed (74 files); `bun run typecheck` clean; `bun run lint` clean on changed files; `cargo test -p keeper-core --lib` 533 passed. Patches touched `conversation-pane.tsx` (key), `composer.tsx` (restore latch), `ipc.rs` (docstring only) — no schema, IPC-shape, or `src/lib/ipc/gen` binding change.

**Residual risks:** Cross-device marker consistency during a concurrent account add/sign-out re-seed is eventually-consistent (self-heals from DB truth); a force-quit within the ~200 ms debounce window can lose the last keystrokes of an actively-typed draft (bounded, by design). The composite-key change reverses a prior explicit decision (see follow-up triage note); it is strictly additive (can only add remounts) but merits the recommended independent confirmation. All within the debounced-durability contract.

**Review findings (follow-up pass 2, 2026-07-05):** Independent Blind Hunter + Edge Case Hunter pass. 1 patch applied (low: failed-non-edit-send re-persist read the stale `draft` closure instead of the live `draftRef.current`, so a retype-during-send + immediate room-switch persisted the pre-retype body — now reads the live ref). 0 intent_gap, 0 bad_spec, 4 deferred (1 medium: in-flight-save vs post-send-clear write-reorder resurrecting a sent draft — fix touches the spec-protected send-ordering path; 3 low: no `busy_timeout` PRAGMA → swallowed `SQLITE_BUSY` draft loss, unbounded draft `body` size, plaintext message-content at rest), 11 rejected (all low; several re-raised prior-pass rejections with unchanged rationale — space-delimited key and `accountId ?? ""` are provably unreachable, the rest self-heal or are by-design; see Review Triage Log). The single patch is strictly-safe and localized, so no independent follow-up review is recommended.

**Verification (follow-up pass 2):** `bun run test` 699 passed (74 files); `bun run typecheck` clean; `bun run lint` clean (220 files). The patch touched only `composer.tsx` (send catch block — `draft` → `draftRef.current`); no schema, IPC-shape, or binding change, so no Rust re-run needed.
