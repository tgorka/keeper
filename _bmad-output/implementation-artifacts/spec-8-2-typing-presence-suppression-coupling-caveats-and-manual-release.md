---
title: 'Typing/Presence Suppression, Coupling Caveats, and Manual Release'
type: 'feature'
created: '2026-07-05'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: [oversized]
baseline_revision: '0fc6fc9d187924a5dce76286da3765e2d5181b08'
final_revision: '015e9a441b41dd99da0c560072bb42703f0215f2'
---

<intent-contract>

## Intent

**Problem:** Story 8.1 made read receipts go private under Incognito, but three suppression promises are still open: typing indicators still leave the machine, networks that *couple* behaviors (e.g. WhatsApp couples sending read receipts with seeing others') are toggled with no disclosure, and there is no way to deliberately release a public read when the user *chooses* to acknowledge. Beeper paywalls this completeness; keeper ships it free.

**Approach:** Extend the `keeper-core::signals` seam (AD-14 sole gate): gate the existing `set_typing` on the same effective Incognito policy 8.1 already resolves (Chat > Account > Global) so zero typing events leave the machine while Incognito applies; add `release_receipt(timeline)` that emits exactly one *public* `m.read` on demand. Surface the per-Network coupling caveat — read from the versioned `coupling-caveats.json` seeded in Story 6.1 — inline in a per-Chat Incognito control in the header, which also carries the "Mark read publicly" release action. Presence is global in Matrix and keeper emits none, so it is already withheld; a guardrail test pins that it stays that way.

## Boundaries & Constraints

**Always:**
- Typing emission stays inside `signals::set_typing`; the crate-wide AD-14 sole-gate test (`.typing_notice(` / `.subscribe_to_typing_notifications(` / `.mark_as_read(` / `.send_single_receipt(` appear only in `signals.rs`) MUST still pass.
- Typing suppression reuses 8.1's policy: `set_typing` takes the resolved `EffectivePolicy` and, when `enabled`, emits nothing (neither `true` nor `false`). Scope read is **fail-closed** like `mark_room_read`: a registry read error suppresses emission (never leaks typing), logged at `warn`.
- `release_receipt` always dispatches public `ReceiptType::Read` at the current read position regardless of policy — it is the explicit, user-triggered exception; without it only private receipts are ever sent while Incognito applies.
- Coupling caveats are read from `bridges::data::coupling_caveats()` (the 6.1 data file), joined to the open room's Network by `networkId` (room→network via the existing bridge protocol id). The frontend renders resolved caveat text only — no caveat copy is authored in TypeScript.
- Best-effort dispatch preserved: a typing or receipt dispatch failure is logged and swallowed, never a UI error.
- New VM (`CouplingCaveatVm`) follows spine conventions: camelCase serde, `Vm` suffix, ts-rs binding into `src/lib/ipc/gen/`; violet `--incognito` token for all Incognito surfaces.

**Block If:**
- The installed matrix-sdk exposes no way to emit a public single/latest read receipt from within `signals` (no `mark_as_read(ReceiptType::Read)` or `send_single_receipt` path). HALT — do not fake release by re-marking private.

**Never:**
- Do not add a second typing/receipt emit path, and do not add any presence emission (`set_presence` / `PresenceState`) anywhere — presence stays withheld by construction.
- Do not add a separate typing-only toggle (PRD §4.7: bundled with Incognito in MVP) or the ⌘⇧I / ⌘⇧-release hotkeys (Epic 9).
- Do not resolve Incognito precedence or author caveat copy on the frontend; do not hold receipt/typing state in a JS store as source of truth.
- Do not touch Undo-Send / `outbox` (Story 8.3) or post-dispatch delete (Story 8.4).

## I/O & Edge-Case Matrix

`should_emit_typing(policy)` and the release/caveat behaviors:

| Scenario | Input / State | Expected | Error Handling |
|----------|--------------|----------|----------------|
| Typing, Incognito effective | policy.enabled = true, typing = true | no `typing_notice` emitted (suppressed) | none |
| Stop-typing, Incognito effective | policy.enabled = true, typing = false | no `typing_notice` emitted (suppressed) | none |
| Typing, Incognito off | policy.enabled = false, typing = true | `room.typing_notice(true)` emitted (unchanged) | dispatch failure logged + swallowed |
| Stop-typing, Incognito off | policy.enabled = false, typing = false | `room.typing_notice(false)` emitted (unchanged) | dispatch failure logged + swallowed |
| Typing, scope read fails | registry `incognito_scopes` errors | emission skipped (fail-closed) | logged at `warn` |
| Manual release | user triggers "Mark read publicly" on a room | `signals::release_receipt` dispatches exactly one public `ReceiptType::Read` at the latest position; own + remote clients see it read | dispatch failure logged + swallowed (Ok) |
| Caveat, coupled network | open room `networkId` = `whatsapp` | caveat text ("you may also stop seeing others' read receipts") returned + shown inline at the Incognito toggle | none |
| Caveat, uncoupled / native | `networkId` = `telegram` or null (native Matrix) | no caveat shown | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/signals.rs` -- AD-14 seam. `set_typing(room, typing)` currently emits unconditionally; add `policy` param + pure `should_emit_typing(&EffectivePolicy)` and suppress when enabled. Add `release_receipt(timeline) -> Result<bool, SignalError>` calling `mark_as_read(ReceiptType::Read)` (public). Keep sole-gate; add presence-withheld guardrail test.
- `src-tauri/crates/keeper-core/src/account.rs` -- `set_typing` (~2976): take `platform`, resolve scopes via `registry::incognito_scopes` + `signals::resolve_incognito` (fail-closed, mirror `mark_room_read` ~2609), pass policy to `signals::set_typing`. Add `release_receipt(platform, account_id, room_id)` sibling to `mark_room_read` (open/build timeline, call `signals::release_receipt`, best-effort).
- `src-tauri/crates/keeper-core/src/bridges/data.rs` -- `coupling_caveats()` loader already present (6.1); source of caveat text/`networkId`/`appliesTo`.
- `src-tauri/crates/keeper-core/src/bridges/mod.rs` -- add `coupling_caveats_catalog() -> Result<Vec<CouplingCaveatVm>, BridgeError>` projecting the data doc (mirror `catalog()` ~125).
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `CouplingCaveatVm { networkId, text, appliesTo }` (ts-rs export).
- `src-tauri/crates/keeper/src/ipc.rs` -- `set_typing` command (~2502): thread `state.platform` into `accounts.set_typing`. Add `release_receipt(account_id, room_id)` and `coupling_caveats() -> Vec<CouplingCaveatVm>` commands.
- `src-tauri/crates/keeper/src/lib.rs` -- register `release_receipt`, `coupling_caveats` in `generate_handler!`.
- `src/lib/ipc/client.ts` -- add `releaseReceipt(accountId, roomId)` + `couplingCaveats()` wrappers; re-export `CouplingCaveatVm`.
- `src/hooks/use-coupling-caveats.ts` (new) -- fetch caveats once, filter by `networkId` (mirror `use-bridge-catalog.ts`).
- `src/components/layout/conversation-pane.tsx` -- convert `ConversationIncognitoChip` (~284) into a per-Chat Incognito Popover control: effective → violet chip label; not-effective → subtle trigger enabling per-Chat. Popover body carries the per-Chat toggle, the inline coupling caveat (when the room's Network couples), and a "Mark read publicly" action (calls `releaseReceipt`) shown while effective. Colocated tests.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/signals.rs` -- add pure `should_emit_typing(&EffectivePolicy)`; change `set_typing` to `(room, typing, policy)` suppressing when `enabled`; add `release_receipt(timeline)` emitting public `ReceiptType::Read`; update doc comments (drop the "Story 8.2" TODO). Unit-test the four typing rows + a presence-withheld test asserting `.set_presence(`/`PresenceState` appear nowhere under `keeper-core/src`. Keep the sole-gate test green.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- thread `platform` + fail-closed scope resolution into `set_typing`, passing the policy to `signals::set_typing`; add `release_receipt(platform, account_id, room_id)` best-effort sibling.
- [x] `src-tauri/crates/keeper-core/src/bridges/mod.rs` + `vm.rs` -- add `CouplingCaveatVm` and `coupling_caveats_catalog()`; unit-test that every catalog caveat carries non-empty text and a real `networkId`.
- [x] `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- thread `platform` into the `set_typing` command; add + register `release_receipt` and `coupling_caveats` commands via the `IpcError` path.
- [x] `src/lib/ipc/client.ts` -- add `releaseReceipt` + `couplingCaveats` wrappers; re-export `CouplingCaveatVm`.
- [x] `src/hooks/use-coupling-caveats.ts` (new) -- hook returning caveats for a `networkId`; colocated test.
- [x] `src/components/layout/conversation-pane.tsx` -- Incognito Popover control (enable/disable per-Chat, inline caveat at toggle for coupled networks, "Mark read publicly" release while effective). Colocated tests: caveat shows for WhatsApp / hidden for native; release action calls `releaseReceipt`; effective-scope label unchanged.

**Acceptance Criteria:**
- Given Incognito effective for a Chat, when the user types, then zero typing events leave the machine (`set_typing` emits nothing) — asserted by unit tests on all four `should_emit_typing` rows plus the fail-closed skip — all through `signals` with the AD-14 sole-gate test still passing (FR-43, AD-14).
- Given presence, then keeper emits none anywhere and the guardrail test proves it stays withheld (FR-43).
- Given a coupled Network (WhatsApp), when the user opens the Incognito toggle on that Chat, then the coupling caveat text surfaces inline, sourced from `coupling-caveats.json`; an uncoupled or native Chat shows no caveat (FR-44).
- Given an Incognito Chat, when the user triggers "Mark read publicly", then `signals::release_receipt` emits exactly one public `m.read` at the current read position; without it only private receipts are sent while Incognito applies (FR-45).
- Given the whole change, then `bun run check`, `bun run check:rust`, and `bun run test:rust` are green.

## Design Notes

Typing suppression mirrors 8.1's receipt path exactly — the policy is resolved once at emission time and both `true` and `false` are dropped while Incognito is on. Dropping the `false` (stop-typing) too is deliberate: emitting a clear is itself a `m.typing` event on the wire, so honoring "zero typing events" means suppressing both; a public indicator shown *before* Incognito was enabled simply expires via the server's typing timeout.

`release_receipt` is intentionally distinct from `mark_read(policy)` rather than "mark_read with a forced-public policy": the explicit user action must never be mistaken for the automatic read path, and it reuses the already-gated `mark_as_read(ReceiptType::Read)` so it adds no new SDK surface to the sole-gate.

Presence in Matrix is a per-user (account-global) signal, not per-room, so per-Chat Incognito cannot scope it; "withheld where the protocol allows" is satisfied because keeper has no presence-emit path at all. The guardrail test (no `.set_presence(` / `PresenceState` under `keeper-core/src`) keeps a future change from silently adding one.

Caveats reuse the 6.1 data file and its existing `coupling_caveat_network_ids_are_all_catalog_networks` join test; 8.2 only adds the read-only VM projection + command (mirroring `bridge_catalog`) and the frontend surfacing.

## Verification

**Commands:**
- `bun run check:rust` -- rustfmt clean + clippy `-D warnings` (no `.unwrap()` in new paths).
- `bun run test:rust` -- new typing-suppression (4 rows) + fail-closed + presence-withheld + caveat-catalog tests pass; AD-14 sole-gate test still passes.
- `bun run check` -- biome + tsc + vitest green, including the coupling-caveat hook and Incognito-control tests.

## Spec Change Log

_No bad_spec loopbacks. The single review pass produced only patch-level fixes; the intent contract and all spec sections were left intact._

## Review Triage Log

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 5: (high 0, medium 1, low 4)
- defer: 0
- reject: 4
- addressed_findings:
  - `[medium]` `[patch]` The `presence_is_withheld_everywhere` guardrail scanned only `keeper-core/src`, but the module doc/spec claimed a crate-wide guarantee — a `set_presence` Tauri command added to the sibling `keeper` IPC crate would have slipped past. Extended the scan to also walk `../keeper/src` (asserts the dir resolves, fails loudly otherwise); doc corrected to "across keeper-core and the keeper IPC crate" (`src-tauri/crates/keeper-core/src/signals.rs`). No live leak existed (sliding sync via `SyncService`; IPC crate verified presence-clean).
  - `[low]` `[patch]` The header caveat rendered only `caveats[0]`, silently dropping any second+ coupling caveat a network might carry (the data schema permits several per `networkId`). Now maps over every matched caveat in both the effective popover body and the enable affordance; test extended to assert multiple caveats all render (`src/components/layout/conversation-pane.tsx`).
  - `[low]` `[patch]` The Incognito Popover was uncontrolled and the control was not keyed by room, so a room switch while it was open could rebind enable/disable/release to a different chat, and rapid clicks could double-fire release. Keyed the control by `selectedRoomId` (remount closes it on switch) and made the Popover controlled, closing it after the turn-off and "Mark read publicly" handlers fire; tests added for the effective→off transition and single `releaseReceipt` call (`src/components/layout/conversation-pane.tsx`).
  - `[low]` `[patch]` The `should_emit_typing` test asserted two byte-identical pairs mislabeled as the "typing=true/false rows", proving nothing (the pure gate takes no `typing` arg). Rewrote it to honestly pin the policy gate (false when enabled across both sources, true when disabled) with a comment noting the four I/O-matrix rows follow because the gate is `typing`-independent and `set_typing` early-returns on it (`src-tauri/crates/keeper-core/src/signals.rs`).
  - `[low]` `[patch]` `release_receipt` carried an unused `_platform` param (public release resolves no scopes) — a permanently-underscored arg implying missing fail-closed logic. Dropped it from the core method and the IPC command (`src-tauri/crates/keeper-core/src/account.rs`, `src-tauri/crates/keeper/src/ipc.rs`).
- Notable rejects (verified, not real / out of scope): the claimed-missing `coupling_caveat_network_ids_are_all_catalog_networks` join test **does** exist (`bridges/mod.rs:291`) — reviewers grepped `data.rs`; duplicate-`networkId` catalog validation is mooted by rendering all caveats (no user-facing drop remains); the stale-`refreshIncognito`-after-room-switch race cannot regress a visible surface because the mirror store is keyed per `(accountId, roomId)` (same reasoning accepted in 8.1); the hook's no-refetch-after-failed-initial-load is within the spec's blessed one-shot-fetch contract and treats an absent caveat as a benign no-hint.

## Auto Run Result

Status: done

### Summary
Story 8.2 completes the Incognito suppression surface on top of 8.1: typing indicators are now gated inside the `keeper-core::signals` seam and emit nothing while the effective Incognito policy (Chat > Account > Global) is on (fail-closed on a registry scope-read error); presence stays withheld by construction (keeper has no emit path) with a guardrail test across both crates; per-Network coupling caveats from the 6.1 `coupling-caveats.json` surface inline at Incognito toggle time in the header control; and an explicit "Mark read publicly" action emits exactly one public `m.read` on demand via `signals::release_receipt`.

### Files changed
- `src-tauri/crates/keeper-core/src/signals.rs` -- `should_emit_typing` gate; `set_typing(room, typing, policy)` suppresses start+stop under Incognito; `release_receipt(timeline)` emits public `ReceiptType::Read`; typing + cross-crate presence-withheld guardrail tests; AD-14 sole-gate preserved.
- `src-tauri/crates/keeper-core/src/account.rs` -- fail-closed scope resolution threaded into `set_typing`; best-effort `release_receipt(account_id, room_id)` sibling of `mark_room_read`.
- `src-tauri/crates/keeper-core/src/bridges/mod.rs` -- `coupling_caveats_catalog()` projecting the 6.1 data doc, with a catalog test.
- `src-tauri/crates/keeper-core/src/vm.rs` -- `CouplingCaveatVm { networkId, text, appliesTo }` (ts-rs export).
- `src-tauri/crates/keeper/src/ipc.rs` -- `set_typing` command threads the platform; new `release_receipt` and `coupling_caveats` commands via `to_ipc_error`.
- `src-tauri/crates/keeper/src/lib.rs` -- registered `release_receipt` and `coupling_caveats`.
- `src/lib/ipc/client.ts` -- `releaseReceipt` + `couplingCaveats` wrappers; `CouplingCaveatVm` re-export.
- `src/hooks/use-coupling-caveats.ts` (+ test) -- one-shot caveat fetch filtered by `networkId`.
- `src/lib/ipc/gen/CouplingCaveatVm.ts` -- generated ts-rs binding.
- `src/components/layout/conversation-pane.tsx` (+ test) -- per-Chat Incognito Popover control: enable/disable, inline caveat at toggle for coupled networks, "Mark read publicly" release; keyed by room, controlled open.

### Review findings breakdown
- Patches applied: 5 (1 medium — guardrail scope; 4 low — caveat rendering, popover keying/close, test cleanup, dead-param removal).
- Deferred: 0.
- Rejected: 4 (false-alarm missing test, mooted dup-networkId validation, non-regressing stale-refresh race, in-contract no-refetch).

### Verification
- `bun run check:rust` -- PASS (rustfmt clean + clippy `-D warnings`).
- `bun run test:rust` -- PASS (633 tests, 0 skipped; typing/presence/caveat + AD-14 sole-gate all green).
- `bun run check` -- PASS (biome + tsc + vitest: 79 files, 774 tests).

### Residual risks
- Typing suppression drops the stop-typing (`false`) event too, so a public typing indicator shown *before* Incognito was enabled lingers until the homeserver's typing timeout — a deliberate, documented trade-off to honor "zero typing events."
- Presence is a per-user (account-global) Matrix signal and cannot be scoped per-Chat; the guarantee is "keeper emits none," pinned by the cross-crate guardrail test — if a future presence feature is added it must consciously respect Incognito.
- The coupling-caveat catalog is fetched once per control mount; a transient IPC failure at first load suppresses the FR-44 hint for that session (within the spec's one-shot contract, treated as a benign no-hint).
