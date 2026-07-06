---
title: 'Incognito Read Receipts with Scoped Policy'
type: 'feature'
created: '2026-07-05'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: 'b123e33f5fac5bda531439ed30a10b75bcacbf5c'
final_revision: '99f5bfc263091497be476d7ad0360c243579903c'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** Reading a message currently always emits a public `m.read` receipt, so the sender sees "read" and the user answers under social pressure. Beeper paywalls the fix; keeper ships it free.

**Approach:** Add an Incognito toggle at three scopes (global, per-Account, per-Chat) whose effective policy is resolved deterministically (Chat > Account > Global) at emission time inside the `signals` seam; when Incognito is effective the read is dispatched as `m.read.private` (own devices still sync, the remote party keeps showing unread), and the Chat header surfaces the effective scope with a violet chip plus a violet composer focus ring. This story covers read receipts only — typing/presence suppression, coupling caveats, and manual release are Story 8.2.

## Boundaries & Constraints

**Always:**
- All receipt emission stays inside `keeper-core::signals`; the crate-wide sole-gate test (only `signals.rs` may call `.mark_as_read(` / `.send_single_receipt(`) MUST still pass. The private-receipt path also goes through `signals`.
- Effective-policy resolution is a pure function in `signals`, resolving Chat over Account over Global, evaluated at emission time.
- When effective Incognito is on, `mark_read` dispatches `ReceiptType::ReadPrivate`; when off, `ReceiptType::Read` (unchanged behavior).
- Per-Chat and per-Account scopes are tri-state: unset = inherit the next-broader scope; Global is a plain bool defaulting to off (Incognito off by default).
- Storage lives in `keeper.db` (WAL): global in the `settings` k/v table, per-Account as a nullable column on `accounts`, per-Chat in a new `(account_id, room_id)`-keyed table — following the existing registry/drafts patterns.
- Receipt/marked-read best-effort behavior is preserved: a dispatch failure is logged and swallowed, never a UI error.
- VMs follow spine conventions: camelCase serde, `Vm` suffix, ts-rs binding in `src/lib/ipc/gen/`, mirrored into a zustand store; `--incognito` token used for all violet surfaces.

**Block If:**
- The installed matrix-sdk `Timeline` API cannot dispatch a private read receipt (no `ReceiptType::ReadPrivate` path through `mark_as_read`/`send_single_receipt`). HALT — do not fake privacy by suppressing the receipt entirely.

**Never:**
- Do not implement typing suppression, presence withholding, coupling caveats, or manual "Mark read publicly" release — those are Story 8.2. Do not add the ⌘⇧I hotkey (Epic 9).
- Do not add a second receipt/typing emit path or move the private-receipt call outside `signals`.
- Do not hold Matrix/receipt state in a JS store as source of truth — the store only mirrors the Rust VM.
- Do not resolve effective policy on the frontend — the frontend renders the resolved VM only.

## I/O & Edge-Case Matrix

Effective-policy resolver (`resolve_incognito`): inputs `chat: Option<bool>`, `account: Option<bool>`, `global: bool` → `{ enabled, source }`. The first eight rows (2 global × 2 account × 2 chat, each broader scope either inherited or overridden) are the deterministic resolver contract — each MUST have a unit test. The final two rows are the receipt-dispatch behaviors that consume the resolved policy.

| Scenario | Input (global, account, chat) | Expected (enabled, source) | Error Handling |
|----------|-------------------------------|----------------------------|----------------|
| Global off, all inherit | (false, None, None) | (false, Global) | none |
| Global on, all inherit | (true, None, None) | (true, Global) | none |
| Account enables over global-off | (false, Some(true), None) | (true, Account) | none |
| Account disables over global-on | (true, Some(false), None) | (false, Account) | none |
| Chat enables over global-off | (false, None, Some(true)) | (true, Chat) | none |
| Chat disables over global-on | (true, None, Some(false)) | (false, Chat) | none |
| Chat overrides account (off wins) | (false, Some(true), Some(false)) | (false, Chat) | none |
| Chat overrides account (on wins) | (true, Some(false), Some(true)) | (true, Chat) | none |
| Read with effective Incognito on | user reads a Chat where resolver → enabled | `signals::mark_read` dispatches `ReceiptType::ReadPrivate`; own read position syncs, remote shows unread | dispatch failure logged + swallowed (Ok) |
| Read with effective Incognito off | user reads a non-Incognito Chat | `mark_read` dispatches public `ReceiptType::Read` (unchanged) | dispatch failure logged + swallowed (Ok) |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/signals.rs` -- AD-14 sole receipt gate; `mark_read` currently hardcodes `ReceiptType::Read`. Add the resolver + policy types here; branch receipt type on effective policy.
- `src-tauri/crates/keeper-core/src/registry.rs` -- SQLite (WAL). `settings` k/v, `accounts` table with `ensure_*_column` migration pattern, `drafts` `(account_id, room_id)` table pattern. Add global/account/chat incognito storage + a combined-scope read.
- `src-tauri/crates/keeper-core/src/account.rs` -- `mark_room_read` (~line 2605) calls `signals::mark_read(&timeline)`. Fetch scopes from registry, resolve, pass policy. Add core get/set methods for the three scopes.
- `src-tauri/crates/keeper-core/src/vm.rs` -- VM structs (`AccountVm`, `TypistVm`); add `IncognitoVm` with ts-rs derive.
- `src-tauri/crates/keeper/src/ipc.rs` -- `#[tauri::command]`s (e.g. `mark_room_read` ~2207). Add incognito get/set commands.
- `src-tauri/crates/keeper/src/lib.rs` -- `tauri::generate_handler![...]` (~47-146); register new commands.
- `src/lib/ipc/client.ts` -- typed IPC wrappers + `gen/` binding re-exports; add incognito calls + `IncognitoVm`.
- `src/lib/stores/incognito.ts` (new) -- zustand mirror store + hooks, following `src/lib/stores/drafts.ts`.
- `src/components/layout/conversation-pane.tsx` -- Chat header identity/chip row (~1123-1153); add the violet effective-scope chip that toggles per-Chat.
- `src/components/chat/composer.tsx` -- add violet focus ring when Incognito is effective for the active room.
- `src/components/settings/settings-dialog.tsx` -- add a Privacy section with the global Incognito `Switch`.
- `src/components/layout/account-footer.tsx` -- add a per-Account Incognito item to the account `DropdownMenu`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/signals.rs` -- add `IncognitoScope` enum (`Global`/`Account`/`Chat`), an `EffectivePolicy { enabled, source }`, and pure `resolve_incognito(chat, account, global)`; change `mark_read` to take the resolved policy and dispatch `ReceiptType::ReadPrivate` when enabled else `ReceiptType::Read`; keep the SOLE-RECEIPT-GATE and update the doc comment. Unit-test all eight resolver rows.
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- add `ensure_incognito_column()` (nullable `incognito INTEGER` on `accounts`, NULL=inherit) and a `chat_incognito(account_id TEXT, room_id TEXT, enabled INTEGER NOT NULL, PRIMARY KEY(account_id, room_id))` table; add get/set for global (`settings` key `incognito.global`), account, and chat scopes plus a `incognito_scopes(account_id, room_id) -> (chat: Option<bool>, account: Option<bool>, global: bool)` read. Unit-test round-trips + inherit (absent) semantics.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- in `mark_room_read`, read scopes via registry and `signals::resolve_incognito`, pass the policy to `signals::mark_read`; add `incognito_get/set_global/set_account/set_chat` core methods.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `IncognitoVm { effective: bool, source: IncognitoScope, global: bool, account: Option<bool>, chat: Option<bool> }` (camelCase serde, ts-rs export to `src/lib/ipc/gen/`).
- [x] `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- add and register commands `incognito_get(account_id, room_id) -> IncognitoVm`, `incognito_set_global(enabled)`, `incognito_set_account(account_id, value: Option<bool>)`, `incognito_set_chat(account_id, room_id, enabled: Option<bool>)`; map errors via the existing `IpcError` path.
- [x] `src/lib/ipc/client.ts` -- add wrappers + re-export `IncognitoVm` from `./gen/IncognitoVm`.
- [x] `src/lib/stores/incognito.ts` (new) -- zustand store mirroring effective VM per `${accountId} ${roomId}`, global bool, and per-account values; `refresh(accountId, roomId)` after any mutation. Add `useIncognito` / `useGlobalIncognito` hooks. Colocated test.
- [x] `src/components/layout/conversation-pane.tsx` -- render a violet (`bg-incognito`/`text-incognito-foreground`) chip in the header when `effective` is true, labeled by `source` ("Incognito — this chat overrides account" / "— account" / "— global"); clicking the chip toggles the per-Chat scope. Colocated test for the three labels.
- [x] `src/components/chat/composer.tsx` -- tint the composer focus ring violet while Incognito is effective for the active room.
- [x] `src/components/settings/settings-dialog.tsx` -- add a "Privacy" section: global Incognito `Switch` bound to `incognito_set_global`, sentence-case honest copy.
- [x] `src/components/layout/account-footer.tsx` -- add a per-Account Incognito toggle `DropdownMenuItem` (tri-state set/clear) to each account row menu.

**Acceptance Criteria:**
- Given Incognito effective at any scope (per resolver), when the user reads that Chat, then `signals::mark_read` dispatches `m.read.private` — the user's own read position still syncs across their devices and the remote party's client keeps showing the message unread.
- Given Incognito off at all effective scopes, when the user reads a Chat, then a public `m.read` is dispatched exactly as before (no regression).
- Given the effective state, when the Chat renders, then the header shows the violet Incognito chip carrying the effective scope label and the composer focus ring tints violet while Incognito applies.
- Given the precedence rules, then `resolve_incognito` is deterministic and all eight I/O-matrix combinations are covered by passing unit tests.
- Given the whole change, then the crate-wide AD-14 sole-gate test still passes and `bun run check`, `bun run check:rust`, `bun run test:rust` are green.

## Spec Change Log

_No bad_spec loopbacks — the review pass produced only patch-level fixes; the intent contract and spec sections were not amended._

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 5: (high 1, medium 2, low 2)
- defer: 1: (high 0, medium 1, low 0)
- reject: 9
- addressed_findings:
  - `[high]` `[patch]` `mark_room_read` failed *open* (public `m.read`) on a registry scope-read error, leaking a read the user meant to keep private — changed to fail *closed*: `policy` is now `Option`, emission is skipped when the scope read fails, logged at `warn` (`src-tauri/crates/keeper-core/src/account.rs`).
  - `[medium]` `[patch]` The privacy-critical private-vs-public branch of `mark_read` had no test — extracted a pure `receipt_type_for(&EffectivePolicy)` helper and pinned both branches (`ReadPrivate` when enabled, `Read` when not), preserving the AD-14 sole gate (`src-tauri/crates/keeper-core/src/signals.rs`).
  - `[medium]` `[patch]` Toggling global (Settings) or per-account (menu) Incognito while a chat was open did not update that chat's chip/ring until a room reopen — added a `policyVersion` bump to the incognito store, bumped after successful global/account writes, with the open chat's refresh effect depending on it (`src/lib/stores/incognito.ts`, `settings-dialog.tsx`, `account-footer.tsx`, `conversation-pane.tsx`).
  - `[low]` `[patch]` Chip click had no `.catch` (unhandled rejection on write failure) — added best-effort catch (`src/components/layout/conversation-pane.tsx`).
  - `[low]` `[patch]` Room-open refresh effect was fire-and-forget with no cancel guard — added a `cancelled` flag to `refreshIncognito` so a late read cannot clobber after a fast room switch (`src/lib/stores/incognito.ts`, `conversation-pane.tsx`).

### 2026-07-05 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 1, low 1)
- defer: 1: (high 0, medium 0, low 1)
- reject: 18
- addressed_findings:
  - `[medium]` `[patch]` The per-Account Incognito submenu (`AccountIncognitoSubmenu`) reverted on a persist failure using a plain captured `prev` with no write-ordering guard — unlike `PrivacySection`, which uses a monotonic `writeId`. A slow failed write racing a newer successful selection could clobber the radio back to a stale value, misrepresenting a privacy setting. Added the same monotonic `writeId` guard so only the newest write may revert (`src/components/layout/account-footer.tsx`).
  - `[low]` `[patch]` Two doc comments on the mark-read / re-mark effects still asserted the receipt was a public `m.read`, stale after this story made the receipt type policy-dependent (a misleading claim in a privacy feature). Corrected both to state the type is a Rust-side decision from the effective Incognito policy (`src/components/layout/conversation-pane.tsx`).
- deferred:
  - `[low]` `[defer]` matrix-sdk emits an implicit *public* read receipt on message send, outside the `signals` sole-gate, so sending under Incognito still leaks a public read position — pre-existing SDK behavior surfaced incidentally; logged to `deferred-work.md`.
- Notable rejects (verified against code, not real / out of scope): the fail-closed error branch is genuinely fail-*closed* (returns `None` and skips emission — the "fails open to public" claim is false); `set_incognito_account`'s no-op on an absent row is unreachable (account rows exist before the menu renders); the store's `global`/`byAccount` mirrors have no live consumer, so a stale chat read cannot regress a visible surface; the chip's empty `.catch` is consistent (no optimistic update to revert; a failed write leaves DB and store both unchanged); the public→private transition "no-retract" is inherent Matrix protocol reality (a receipt already sent cannot be un-sent) and future reads under Incognito still go private; per-chat inherit-clear affordance was already deferred in the prior pass.

## Design Notes

Resolver shape (the tri-state precedence is the whole contract):

```rust
pub fn resolve_incognito(chat: Option<bool>, account: Option<bool>, global: bool) -> EffectivePolicy {
    if let Some(c) = chat { return EffectivePolicy { enabled: c, source: IncognitoScope::Chat }; }
    if let Some(a) = account { return EffectivePolicy { enabled: a, source: IncognitoScope::Account }; }
    EffectivePolicy { enabled: global, source: IncognitoScope::Global }
}
```

`m.read.private` is the correct primitive: Matrix private read receipts sync to the *user's own* devices (read position converges across their devices) but are not visible to other room members, which is exactly the "read without pressure" contract. The header chip always shows the *effective* source, so a per-Chat override reads "this chat overrides account" even when the underlying value matches the account — the label reflects which scope decided, not equality.

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` pass (no `.unwrap()` in new paths).
- `bun run test:rust` -- expected: new resolver (8 rows) + registry round-trip tests pass; AD-14 sole-gate test still passes.
- `bun run check` -- expected: biome + tsc + vitest green, including new incognito store/chip tests.

## Auto Run Result

Status: done

### Summary
Implemented Story 8.1 — Incognito Read Receipts with Scoped Policy. Added a three-scope Incognito toggle (global / per-Account / per-Chat) whose effective policy resolves deterministically (Chat > Account > Global) inside the `keeper-core::signals` seam; when effective, reads dispatch `m.read.private` instead of the public `m.read` (own devices still sync, remote keeps showing unread). Surfaced a violet header chip carrying the effective scope, a violet composer focus ring, a Settings → Privacy global switch, and a per-Account tri-state menu item. The AD-14 sole-emitter invariant is preserved.

### Files changed
- `src-tauri/crates/keeper-core/src/signals.rs` — `IncognitoScope`, `EffectivePolicy`, pure `resolve_incognito`; `mark_read` branches receipt type via new testable `receipt_type_for`; 8-row resolver test + private/public branch test.
- `src-tauri/crates/keeper-core/src/registry.rs` — `chat_incognito` table, nullable `accounts.incognito` column (`ensure_incognito_column`), `settings` key `incognito.global`, get/set + `incognito_scopes` read; round-trip tests.
- `src-tauri/crates/keeper-core/src/account.rs` — `mark_room_read` resolves policy and fails **closed** on a scope-read error (skips emission, never leaks public); core get/set methods.
- `src-tauri/crates/keeper-core/src/vm.rs` — `IncognitoVm` (ts-rs → `src/lib/ipc/gen/`).
- `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` — six commands: `incognito_get`, `incognito_get_global`, `incognito_set_global`, `incognito_get_account`, `incognito_set_account`, `incognito_set_chat`.
- `src/lib/ipc/client.ts` — typed wrappers + `IncognitoVm`/`IncognitoScope` re-exports.
- `src/lib/stores/incognito.ts` — zustand mirror store + `policyVersion` re-trigger + cancel-guarded `refreshIncognito` + hooks; colocated tests.
- `src/components/layout/conversation-pane.tsx` — violet effective-scope chip (toggles per-Chat); policy-version-driven, cancel-guarded refresh.
- `src/components/chat/composer.tsx` — violet focus ring while Incognito effective.
- `src/components/settings/settings-dialog.tsx` — Privacy section with global switch (bumps policy version).
- `src/components/layout/account-footer.tsx` — per-Account tri-state Incognito submenu (bumps policy version).

### Review findings breakdown
- Patches applied (5): 1 high (fail-closed privacy fallback), 2 medium (private-branch test coverage; broad-scope-toggle staleness of open chat), 2 low (chip unhandled rejection; refresh cancel guard).
- Deferred (1): header chip is a one-way per-Chat toggle with no "inherit"-clear affordance (tracked in `deferred-work.md`).
- Rejected (9): pre-existing transient-timeline no-op semantics; unreachable `set_account` no-op-on-absent-row; cosmetic load-window ring/radio flashes; unused `useGlobalIncognito` export; frontend-guarded empty-`room_id` row; copy/doc-grammar nits.

### Verification
- `bun run check:rust` — rustfmt clean + clippy `-D warnings` pass.
- `bun run test:rust` — 629 tests passed (incl. resolver 8-row, private/public branch, registry round-trips, AD-14 sole-gate).
- `bun run check` — biome + tsc clean; 760 frontend tests passed (incthe incognito store's 8 tests incl. new policy-version + cancel-guard cases).

### Residual risks
- The deferred per-Chat inherit-clear affordance means a user who explicitly toggles a chat via the chip cannot return it to inherit from the UI (backend already supports it).
- matrix-sdk emits an implicit *public* read receipt on message send, outside the `signals` sole-gate (new defer this pass) — sending under Incognito still leaks a public read position. Marginal (sending reveals engagement anyway); tracked for a conscious scope/suppress decision.

### Follow-up review pass (2026-07-05)
An independent follow-up review re-ran Blind Hunter + Edge Case Hunter over the full baseline→HEAD diff. Outcome: 2 patches, 1 defer, 18 rejects; no intent_gap and no bad_spec (implementation matches the contract).
- **Patch (medium):** `AccountIncognitoSubmenu` gained a monotonic `writeId` revert guard (matching `PrivacySection`) so a slow failed per-account write can't clobber a newer successful selection back to a stale radio value (`account-footer.tsx`).
- **Patch (low):** Fixed two stale doc comments that still called the mark-read receipt a public `m.read` after the type became Incognito-policy-dependent (`conversation-pane.tsx`).
- **Defer (low):** Implicit public read receipt on message send bypasses the sole-gate (`deferred-work.md`).
- Load-bearing rejects were verified against the actual code: the registry-error branch is genuinely fail-*closed* (skips emission, does not leak public); the store's `global`/`byAccount` mirrors have no live consumer; the chip's empty `.catch` is consistent; the public→private "no-retract" is inherent protocol behavior.
- `bun run check` green after the patches: biome + tsc clean, 762 frontend tests pass. No Rust touched this pass (both fixes are frontend-only).
- `followup_review_recommended` set to `false`: the two fixes are localized and low-consequence with no receipt-path/behavior/API/data change.
