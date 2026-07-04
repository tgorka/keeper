---
title: 'Account Manager — Unlimited Concurrent Accounts'
type: 'feature'
created: '2026-07-04'
status: 'done'
baseline_revision: '87cd66005dd6ee25fb07743f6fccba7e80fd7ade'
final_revision: '2f6abb07e1e324d78d09c3bed2df266753ec293e'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** keeper is capped at one signed-in Account: the shell is gated on a single `currentAccount`, login overwrites any prior account, and the room list streams from one account. Epic 1's backend is already multi-account-shaped (`AccountManager` holds a `HashMap<account_id, AccountHandle>` and every IPC command takes `accountId`), but nothing lets a user run two accounts at once or merges their chats.

**Approach:** Extract an `AuthProvider` trait (password as its first impl) behind a shared add-account orchestration; add a `keeper-core::inbox` module that merges every active account's room-list stream into one recency-ordered windowed view model over a new `inbox_subscribe` channel; persist a per-account hue (8-hue wheel) in the registry; and refactor the frontend from single-account gating to a merged inbox whose rows carry `accountId` + hue (3 px edge bar), with a minimal add-account entry point and multi-account session restore.

## Boundaries & Constraints

**Always:**
- All Matrix logic, ordering, and filtering stay in Rust; the frontend only renders view models (never re-derive inbox order/filter in TS).
- Adding the Nth account must behave identically to adding the 2nd — no code path may assume or enforce an account-count limit.
- Every login continues to pass the existing SSS capability gate before any Account state, store dir, or Keychain entry is created; cancel/failure leaves zero residue (existing `rollback`).
- Tokens/sessions remain only in the macOS Keychain (`dev.tgorka.keeper`); passwords are never persisted and never cross IPC into JS.
- Each account is one `matrix_sdk::Client` supervised by its own `AccountHandle` with a per-account `tracing` span; send/receive works independently per account.
- Sign-out of one account tears down only that account's tasks + rows and deletes only its `accounts/<id>/sdk/` dir + its Keychain entries; other accounts keep syncing.
- Each account is assigned a hue index (0–7) at add time, persisted in `keeper.db`, stable across restarts; the frontend maps the index to a CSS hue and renders it as a 3 px chat-row edge bar.
- Rust `unsafe_code` denied, no `.unwrap()`/bare `.expect()` in production paths, `?` + `thiserror`; TS `strict`, no `any`, `import type`. New deps pass cargo-deny.

**Block If:**
- Extending the `accounts` registry table or `AccountVm` would require a destructive migration that could drop existing account rows (a nullable `hue_index` column with backfill is fine; anything lossy → HALT).
- A real second homeserver account is genuinely required to satisfy an acceptance test that cannot instead be met by unit-testing the inbox merge with synthetic per-account streams.

**Never:**
- No OIDC or Beeper login (Stories 2.2 / 2.3) — password is the only `AuthProvider` impl here.
- No polished account switcher: avatar, hue dot, homeserver line, sync-state glyph, DropdownMenu, click-to-filter-inbox, and the "keep local archive" sign-out AlertDialog are all Story 2.5. Build only a minimal footer list + "Add Account" button.
- No scroll-driven inbox virtualization / infinite windowing — reuse the existing bounded snapshot-then-diff streaming; full unified-inbox organization is Epic 4.
- No at-rest passphrase choice (Story 2.6); no `matrix-js-sdk`; no new global mutable singletons.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Add 2nd account | One account signed in; user completes password login for a 2nd (same or different homeserver) | New `AccountHandle` supervised; its rooms appear merged into the one inbox by recency; both accounts sync independently | Login errors handled as today (SSS gate, InvalidCredentials, rollback) — first account unaffected |
| Merge ordering | N active accounts each streaming rooms | Single `InboxBatch` stream; rows ordered by latest-event timestamp descending across all accounts; each row carries `accountId` + `hueIndex` | Missing timestamp sorts last (stable) |
| Nth account (no cap) | 5 accounts already active; add a 6th | 6th integrates identically to the 2nd; inbox shows all; no limit error anywhere | n/a |
| Hue assignment | Add account | Lowest unused hue index in 0..8 assigned and persisted; if all 8 in use, `total_count % 8` | n/a |
| Sign out one of many | ≥2 active accounts; sign out account A | A's rooms leave the inbox; A's tasks/sdk-dir/Keychain removed; account B keeps syncing and stays in inbox | Idempotent cleanup (existing `sign_out_cleanup`) |
| Restore all on boot | keeper.db lists ≥2 accounts with valid Keychain sessions | `session_restore` returns all restorable accounts; shell mounts; inbox merges all | An account whose Keychain session is missing is skipped, not fatal |
| Open a chat from merged inbox | User selects a row belonging to account B | Timeline + composer subscribe/send using that row's `accountId`, not a global "current" | RoomNotFound / send errors handled per existing paths |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/auth.rs` -- `login_password` + SSS gate + `rollback`; extract `AuthProvider` trait + `PasswordAuthProvider` and a shared `add_account` orchestration; add hue assignment; make `find_restorable_account` return all restorable accounts.
- `src-tauri/crates/keeper-core/src/registry.rs` -- `accounts` table (`registry.rs:19-48`); add nullable `hue_index` column + migration, read/write hue, list-all helper.
- `src-tauri/crates/keeper-core/src/inbox.rs` -- NEW `keeper-core::inbox`: merge N per-account room-list streams into one recency-ordered model; emit `InboxBatch` diffs.
- `src-tauri/crates/keeper-core/src/account.rs` -- `AccountManager`/`AccountHandle` (`account.rs:78-114`); per-account `tracing` spans (`instrument`) on activation/producers/reconnect; expose active accounts + add/remove notifications to inbox; reuse `run_producer` (`account.rs:712-757`) logic for the merger; `sign_out`/`shutdown` (`account.rs:539-573`) notify inbox.
- `src-tauri/crates/keeper-core/src/vm.rs` -- `RoomVm` (`vm.rs:118-140`), `AccountVm`; add `InboxRoomVm` (RoomVm fields + `accountId` + `hueIndex`), `InboxBatch`/`InboxOp`, add `hueIndex` to `AccountVm`.
- `src-tauri/crates/keeper-core/src/error.rs` -- add `InboxError` (or extend `AccountError`) for the merged stream.
- `src-tauri/crates/keeper/src/ipc.rs` -- new `inbox_subscribe`/`inbox_unsubscribe` commands (activate all accounts, stream `InboxBatch`); `session_restore` (`ipc.rs:387`) returns `Vec<AccountVm>`; register commands in the builder.
- `src/lib/stores/accounts.ts` -- from `currentAccount: AccountVm | null` to `accounts: AccountVm[]` with `addAccount`/`removeAccount`/`hydrateAll`.
- `src/hooks/use-session-restore.ts` -- hydrate all restored accounts.
- `src/lib/stores/rooms.ts` -- hold merged `InboxRoomVm[]`; selection becomes `{ accountId, roomId }`.
- `src/lib/ipc/client.ts` -- add `subscribeInbox`; `sessionRestore` returns array (ts-rs regen).
- `src/components/layout/chat-list-pane.tsx` -- subscribe to merged inbox instead of per-account room list.
- `src/components/chat/chat-row.tsx` -- 3 px left hue edge bar from `hueIndex`.
- `src/components/layout/conversation-pane.tsx` -- timeline/send use the selected row's `accountId` (`conversation-pane.tsx:79,114,157,170`).
- `src/components/layout/account-footer.tsx` -- minimal multi-account list + "Add Account" button.
- `src/components/auth/login-screen.tsx` + `src/App.tsx` -- "add account" mode (don't replace); gate shell on `accounts.length > 0`; render add-account overlay.
- `src/index.css` -- define 8 `--account-hue-N` CSS variables (`index.css:7-101`).

## Tasks & Acceptance

**Execution:**
- [x] `keeper-core/src/auth.rs` -- Define `AuthProvider` trait (async login → session/client) with `PasswordAuthProvider` as first impl; wrap the shared add-account orchestration (SSS gate → store dir → provider login → Keychain session → registry row → hue assignment → rollback on any failure) around it. Refactor `login_password` onto this path.
- [x] `keeper-core/src/registry.rs` -- Add nullable `hue_index` column with idempotent migration + backfill; helpers to assign (lowest unused in 0..8, else `count % 8`), read, and list all accounts.
- [x] `keeper-core/src/inbox.rs` -- NEW: build the merge that consumes each active account's room-list stream (reuse `run_producer` conversion), maintains a recency-ordered merged list of `InboxRoomVm`, and emits `InboxBatch` (snapshot + ops + total); react to account add/remove.
- [x] `keeper-core/src/vm.rs` -- Add `InboxRoomVm` (RoomVm fields + `accountId` + `hueIndex`), `InboxBatch`/`InboxOp`; add `hueIndex` to `AccountVm`; derive ts-rs bindings.
- [x] `keeper-core/src/account.rs` -- Wrap per-account supervision in `tracing` spans carrying `account_id`; expose active-account set + add/remove signals to the inbox; ensure `sign_out`/`shutdown` remove the account from the inbox.
- [x] `keeper-core/src/error.rs` -- Add the inbox stream error variant and map it in `to_ipc_error`.
- [x] `keeper/src/ipc.rs` -- Add `inbox_subscribe`/`inbox_unsubscribe` (activate all accounts, stream `InboxBatch`); change `session_restore` to return all restorable accounts; register new commands.
- [x] `src/lib/stores/accounts.ts` -- Multi-account store (`accounts: AccountVm[]`, add/remove/hydrateAll) replacing single-`currentAccount` model.
- [x] `src/hooks/use-session-restore.ts` + `src/App.tsx` -- Hydrate all restored accounts; gate shell on `accounts.length > 0`; render add-account overlay.
- [x] `src/lib/ipc/client.ts` + `src/lib/stores/rooms.ts` -- Add `subscribeInbox`; hold merged `InboxRoomVm[]`; change selection to `{ accountId, roomId }`.
- [x] `src/components/layout/chat-list-pane.tsx` + `src/components/chat/chat-row.tsx` -- Subscribe to merged inbox; render the 3 px hue edge bar per row.
- [x] `src/components/layout/conversation-pane.tsx` -- Drive timeline subscribe / `sendText` / `retrySend` from the selected row's `accountId`.
- [x] `src/components/layout/account-footer.tsx` + `src/components/auth/login-screen.tsx` -- Minimal footer list of accounts each with sign-out + an "Add Account" button that opens login in add mode.
- [x] `src/index.css` -- Define 8 `--account-hue-N` CSS variables (light + dark).
- [x] Tests -- Rust: inbox merge unit tests (recency order across synthetic per-account streams; add/remove account; N=6 no cap), hue assignment/migration, AuthProvider password path, sign-out isolation. Frontend: accounts store multi-account, chat-row hue bar, chat-list-pane inbox subscribe, conversation-pane uses row `accountId`.

**Acceptance Criteria:**
- Given the Epic 1 single-account code, when this story completes, then password login runs through an `AuthProvider` trait (password as first impl) and `AccountManager` supervises a registry of `AccountHandle`s each with its own Client/SyncService/streams under a per-account tracing span.
- Given ≥2 signed-in accounts (same or different homeservers), when the chat list renders, then it shows all chats from all accounts merged by recency, computed in `keeper-core::inbox` and streamed as one view model, with send/receive working independently per account.
- Given the merged inbox, then each row is attributed to its account by a persisted 8-hue-wheel hue rendered as a 3 px edge bar, stable across restarts.
- Given the codebase, then no path enforces an account-count limit — a 6th account integrates identically to a 2nd (proven by test).
- Given ≥2 active accounts, when one is signed out, then only its tasks/rows/sdk-dir/Keychain are removed and its rooms leave the inbox while the others keep syncing.
- Given ≥2 accounts persisted in `keeper.db` with valid Keychain sessions, when the app restarts, then all are restored and merged.

## Spec Change Log

_No `bad_spec` loopback occurred; the spec was implemented as written._

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 1, low 0)
- defer: 2: (high 0, medium 0, low 2)
- reject: 19
- addressed_findings:
  - `[medium]` `[patch]` Orphaned inbox producer on sign-out: `AccountManager::shutdown` removed the account's merger slot and aborted its `subscriptions`, but the account's inbox producer lives in `InboxHandle.producers` (a `Vec`, not the account's subscription map), so it was never aborted on sign-out — it kept its `RoomList` (and the `Client`'s SQLite handles) alive past `sign_out_cleanup`'s store-dir deletion, self-healing only when the frontend re-subscribed the whole inbox. Fixed: keyed `InboxHandle.producers` by `account_id`; `shutdown` now aborts and awaits that account's producer (releasing its store handles) before store teardown.

 keep the trait narrow to the mechanism-specific credential→session step so 2.2/2.3 can add `oidc`/`beeper` impls without touching shared orchestration. Example shape:
  ```rust
  #[async_trait]
  trait AuthProvider {
      async fn authenticate(&self, client: &Client) -> Result<(), AuthError>;
  }
  // shared: probe SSS → build store → provider.authenticate(&client) → persist session → registry row (+hue) → rollback on Err
  ```
- **Hue lives as an index in Rust; colors live in CSS.** Backend stores/streams `hue_index: u8` (0–7); the frontend maps `--account-hue-{n}` → the 3 px bar. This keeps color values in the theming layer.
- **Merge:** order by latest-event timestamp desc; treat the existing 200-entry page per account as the bound (no new virtualization). `InboxBatch` mirrors the existing snapshot-then-`VectorDiff` pattern (`vm.rs:209-222`), adding `accountId`/`hueIndex` per row and a `total`.
- **Selection model:** because rows now come from different accounts, the selected chat must be `{ accountId, roomId }`; the timeline/composer bind to that pair, not a global current account.
- **Minimal UI is intentional throwaway:** the footer list + Add Account button are the smallest thing that makes multi-account real and testable; Story 2.5 replaces the footer with the designed switcher (avatars, hue dots, glyphs, dropdown, filter, sign-out dialog).

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` pass (no `.unwrap()`, no `unsafe`).
- `bun run test:rust` -- expected: cargo-nextest green, including new inbox-merge / hue / sign-out-isolation tests.
- `bun run check` -- expected: biome + tsc (strict, no `any`) + vitest green, including new store/component tests.
- ts-rs binding regeneration (locate the repo's `export_bindings` test/command) -- expected: `src/lib/ipc/gen/*.ts` regenerated with `InboxRoomVm`/`InboxBatch`/updated `AccountVm`, no stale/uncommitted drift.
- `cargo deny check` (from `src-tauri/`) -- expected: pass (only if any dependency was added).

**Manual checks (if no CLI):**
- With two accounts on different homeservers, confirm one merged recency-ordered list with distinct hue bars, independent send/receive, and that signing out one leaves the other syncing.

## Auto Run Result

Status: **done**

**Summary.** Turned keeper from a single-account client into a multi-account one. Extracted an `AuthProvider` trait (`PasswordAuthProvider` as the first impl) behind a shared `add_account` orchestration (SSS gate → store → authenticate → Keychain session → registry row + hue → rollback). Added `keeper-core::inbox`, a Rust-side merger that folds every active account's room-list stream into one recency-ordered `InboxBatch` over a new `inbox_subscribe` channel. Persisted a per-account hue index (0–7, 8-hue wheel) in `keeper.db` via a non-destructive migration. Refactored the frontend from single-`currentAccount` gating to a merged inbox whose rows carry `{accountId, hueIndex}` (3 px edge bar), with a minimal add-account overlay, multi-account session restore, and per-account sign-out. Per-account `tracing` spans wrap each account's supervision.

**Files changed (one-line each).**
- Backend: `keeper-core/src/auth.rs` (AuthProvider trait + PasswordAuthProvider + add_account orchestration + multi-restore), `registry.rs` (hue_index column/migration/assignment/backfill), `inbox.rs` (new merger), `account.rs` (subscribe_inbox/unsubscribe_inbox, per-account spans, shutdown→inbox teardown + producer abort), `vm.rs` (InboxRoomVm/InboxBatch/InboxOp + AccountVm.hueIndex), `error.rs` (InboxError), `keeper/src/ipc.rs` + `lib.rs` (inbox_subscribe/unsubscribe commands, session_restore→Vec, registration).
- Frontend: `lib/stores/accounts.ts` (multi-account), `lib/stores/rooms.ts` (merged InboxRoomVm[] + {accountId,roomId} selection), `lib/stores/add-account.ts` (new), `lib/account-hue.ts` (new hue→CSS var), `lib/ipc/client.ts` (subscribeInbox, sessionRestore array), `hooks/use-session-restore.ts`/`use-sign-out.ts`/`use-connection-status.ts`, `components/layout/{chat-list-pane,conversation-pane,account-footer}.tsx`, `components/chat/chat-row.tsx`, `components/auth/login-screen.tsx`, `App.tsx`, `index.css` (8 account-hue vars), generated `lib/ipc/gen/{AccountVm,InboxBatch,InboxOp,InboxRoomVm}.ts`.

**Review findings.** 2 reviewers (adversarial + edge-case). After dedup/severity/triage: intent_gap 0, bad_spec 0, patch 1 (medium), defer 2 (low), reject 19.
- Patch applied: orphaned inbox producer on sign-out — `shutdown` didn't abort the account's inbox producer (stored in `InboxHandle`, not the account's subscriptions), leaking its `RoomList`/SQLite handles past store-dir deletion. Fixed by keying producers on `account_id` and aborting + awaiting that producer before store teardown.
- Deferred (see `deferred-work.md`): (1) shell-wide connection pill keys on `accounts[0]` only → per-account sync-state glyph is Story 2.5; (2) inbox fully re-subscribes on every account-set change (empty-list flash + redundant reactivation) → incremental account add/remove belongs to Epic 4 windowing.

**Verification.**
- `bun run check:rust` — PASS (rustfmt clean, clippy `-D warnings` clean), re-run after the patch.
- `bun run test:rust` — PASS, 124 tests (inbox merge recency / missing-ts / N=6 no-cap / add-remove; hue assignment / reuse / non-destructive migration; multi-restore + skip-missing-session + legacy-hue backfill).
- `bun run check` — PASS (biome + tsc strict + 171 vitest + core-tauri-free boundary).
- ts-rs bindings regenerated with no stale drift; `cargo deny` N/A (no new deps).

**Residual risks.**
- The live shutdown→inbox-producer-abort ordering is verified by reasoning + gates; the full multi-`Client` teardown across two real homeservers is not automated (needs a live homeserver, per the spec's Block-If) — the merge/hue/restore seams are unit-tested with synthetic streams.
- The two deferred items are known, documented limitations of this minimal-scope story, not regressions.
