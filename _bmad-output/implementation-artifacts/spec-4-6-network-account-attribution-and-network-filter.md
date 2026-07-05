---
title: 'Network & Account Attribution and Network Filter'
type: 'feature'
created: '2026-07-05'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: ['oversized']
baseline_revision: '3f6b59020d1768ff04e6da20ccaf99ec662277a4'
final_revision: '08f32785011b8f282f511d5f282fef3b8dd4d34c'
---

<intent-contract>

## Intent

**Problem:** Chat rows carry an Account hue bar but no Network identity, so two chats with the same remote contact reached via different bridged Networks (or the same Network on different Accounts) are visually indistinguishable, and there is no way to filter the inbox by Network. The Network badge was deliberately deferred to this story (RoomAvatar.tsx:7).

**Approach:** Stream a per-room bridged-Network label (MSC2346 `m.bridge`, the same source as the existing delete-confirmation `roomNetworkLabel`) on `InboxRoomVm`; render it as a uniform 16 px badge overlaid on the room avatar in both chat rows and the conversation header, alongside an account-initial chip in the header. Add a Rust-side Network filter that mirrors the Story 4.5 Space filter exactly (ephemeral `selected_network`, pre-partition retain, poke-and-re-emit) and composes AND with it, plus a new NETWORKS sidebar group and a 6th inbox channel streaming the distinct connected Networks.

## Boundaries & Constraints

**Always:** Network identity is derived only from a room's `m.bridge`/legacy bridge state via `bridge::room_bridge_network` (local state read, no `/hierarchy` or network fetch) — never fabricated. A native Matrix room (no bridge state) has `network: None` and shows no badge. The Network badge is a uniform neutral color for every Network (never per-Network coloring, and never the primary/mention accent). Filtering, ordering, sectioning, and the distinct-Networks list are computed in Rust and streamed; TypeScript never re-derives or re-sorts them (AD-20). The Network filter selection is ephemeral (no registry key, cleared on relaunch), keyed by Network name (cross-account). The Network filter and the Space filter compose as AND and each renders a dismissible chip.

**Block If:** The bridged-Network label needs a per-Network icon asset or a curated Network→icon map to satisfy attribution (out of scope; badge shows the label initial). A design decision is required to give native-Matrix rooms a first-class "Matrix" Network chip/badge.

**Never:** No bridge-health dot on the NETWORKS rows (deferred to Epic 6). No per-Network coloring of rows, panes, or bubbles (DESIGN Don'ts). No create/join/leave/manage of Networks. No `/hierarchy` or provisioning calls. Do not add a network field to `RoomVm` filtering logic in TypeScript (Rust owns the retain). Do not touch `.mark_as_read(`/`.send_single_receipt(` outside `signals.rs` (AD-14 guard).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Bridged room projected | Room has `m.bridge` with `protocol.displayname="Telegram"` | `RoomVm.network = Some("Telegram")`; row + header show a 16 px "T" badge | n/a |
| Native room projected | Room has no bridge state | `network = None`; no badge rendered | n/a |
| Distinct-Networks snapshot | Merged set spans Telegram (2 accounts) + Signal + native | `NetworksSnapshot.networks = [Signal, Telegram]` (deduped by name, sorted; native excluded) | n/a |
| Select a Network chip | `selected_network = Some("Telegram")` | Every window retains only rooms with `network == Some("Telegram")`, across accounts; chip shown | Unknown/empty match → all windows empty (honest) |
| Compose with Space filter | Space + Network both active | Space retain then Network retain (AND); both chips shown; window totals reflect the intersection | n/a |
| Clear (chip ✕ / Esc / active-row toggle) | A filter is active | Selection + Rust filter cleared; full inbox restored | Best-effort poke; stream is truth |
| Stale selection reconciled | Active Network no longer in streamed snapshot (last room left) | Selection + filter auto-cleared so the inbox is not indefinitely empty | n/a |
| Empty filtered inbox | Filter active, window empty | "No chats in {filter}." with a Clear filter action | n/a |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- `RoomVm` (~L320) + `InboxRoomVm` (~L957): add `network: Option<String>` (bridged-Network label; `None` = native). NEW `NetworkVm { name: String }` and `NetworksSnapshot { networks: Vec<NetworkVm> }`, both `#[serde(rename_all="camelCase")] #[ts(export)]` (generate `NetworkVm.ts`/`NetworksSnapshot.ts`). Update every RoomVm/InboxRoomVm builder + doc-comment.
- `src-tauri/crates/keeper-core/src/account.rs` -- `room_item_to_vm` (~L2759, async): resolve `network = bridge::room_bridge_network(<room handle>).await` (local state read, same resolution as `room_network_label` at ~L1349). NEW `type NetworksSink`. `subscribe_inbox` (~L318): take a 6th `networks_sink`, pass to `InboxMerger::new`. NEW `set_network_filter(&self, network: Option<String>)` (mirror `set_space_filter` ~L1775): lock inbox → `handle.merger.set_network_filter(network)`. `InboxHandle` unchanged (no new producer — the Networks list is derived in the merge).
- `src-tauri/crates/keeper-core/src/inbox.rs` -- `MergeState`/`InboxMerger` (~L60): add `networks_sink: NetworksSink`, `selected_network: Option<String>`. `new(...)` (~L91): add `networks_sink` (last arg). NEW `set_network_filter(Option<String>)`: store, `emit()`. `to_inbox_room` (~L428): copy `network`. `emit` (~L273): after `merge()` and BEFORE the space/network retains, derive `NetworksSnapshot` from the full merged set (distinct non-`None` `network`, deduped, name-sorted) and push via `networks_sink`; then apply the Space retain, then a Network retain (`merged.retain(|r| r.network.as_deref() == selected_network.as_deref())`) — AND composition. `remove_account` (~L120): if the removed account owned the only rooms on `selected_network`, the next `emit` naturally empties then reconciles via the snapshot; no extra clear needed (selection is name-keyed, cross-account). Extend the `capturing_merger*` fixtures (~L530) to a sixth (networks) capture + a helper to assert the snapshot.
- `src-tauri/crates/keeper/src/ipc.rs` -- `inbox_subscribe` (~L1429): add a 6th `networks: Channel<NetworksSnapshot>` wrapped into a `NetworksSink`. NEW `#[tauri::command] set_network_filter(state, network: Option<String>)` → `AccountManager::set_network_filter(network)` (mirror `set_space_filter` ~L1464; `to_ipc_error`).
- `src-tauri/crates/keeper/src/lib.rs` -- `generate_handler!` (~L47): register `set_network_filter`.
- `src/lib/ipc/client.ts` -- `subscribeInbox` (~L276): take a 6th `onNetworks`, create a 6th `Channel<NetworksSnapshot>`, pass `networks` to `inbox_subscribe`. NEW `setNetworkFilter(network: string | null): Promise<void>` → invoke `set_network_filter`.
- `src/lib/stores/networks.ts` -- NEW slim mirror (clone of `spaces.ts`): `networks: NetworkVm[]`, `activeNetwork: string | null`, `applySnapshot`, `setActiveNetwork`, `clear`; `networksStore` + `useNetworksStore`.
- `src/components/chat/RoomAvatar.tsx` -- render `<AvatarBadge>` (from `ui/avatar.tsx`) overlaid on the `<Avatar>` when `room.network !== null`: 16 px (`size-4`), 2 px ring (already on AvatarBadge), uniform neutral bg (override the `bg-primary` default, e.g. `bg-secondary text-secondary-foreground`), content = the Network label's first grapheme uppercased, `aria-label`/`title` = the full Network name. Remove the deferral comment.
- `src/components/layout/networks-group.tsx` -- NEW (clone of `spaces-group.tsx`): uppercase "NETWORKS" `section-label` group of single-select rows (Network name), active via `aria-current`/`bg-accent`, hidden (`return null`) when empty. Row click toggles `setActiveNetwork(...)` **and** `setNetworkFilter(...)`. No bridge-health dot.
- `src/components/layout/sidebar-pane.tsx` -- render `<NetworksGroup>` immediately after `<SpacesGroup>` (~L4 import; place per sidebar structure: primary views → SPACES → NETWORKS).
- `src/components/layout/conversation-pane.tsx` -- header (~L849): to the left of the detail toggle, render the selected room's `RoomAvatar` (network badge comes free) + display name + an account-initial chip (hue-tinted `Avatar size="sm"` with `initials(userId)` + `accountHueVar(hueIndex)`, reusing the extracted helper). Look up the selected room's `InboxRoomVm` via the new `useSelectedRoomVm` hook; when not found, render only the account chip (from `accountsStore` by `accountId`).
- `src/hooks/use-selected-room-vm.ts` -- NEW: subscribe the four window stores (`rooms`/`pins-rooms`/`favorites-rooms`/`archive-rooms`) + `roomsStore.selected`, return the matching `InboxRoomVm | null`.
- `src/lib/account-initials.ts` -- NEW: extract the private `initials(userId)` helper out of `account-footer.tsx` and re-import it there + in the header (one source).
- `src/components/layout/chat-list-pane.tsx` -- subscribe the 6th channel → `networksStore.applySnapshot`; clear `networksStore` list on unsubscribe and on no-accounts; re-apply `setNetworkFilter(activeNetwork)` after (re)subscribe; reconcile a stale `activeNetwork` (not in the streamed snapshot → clear selection + filter), mirroring the Space reconcile (~L132). Render the Network chip alongside the Space chip (chip bar shows when either is active); build the empty-state label from all active filters (" · " joined); Esc / a chip ✕ clears **all** active filters; extend `clearSpaceFilter` into a `clearFilters` that clears both.
- Tests: `inbox.rs` (network retain; AND with space; `NetworksSnapshot` dedup/sort/native-excluded; `to_inbox_room` copies `network`; empty match empties windows); `networks.test.ts`; `networks-group.test.tsx` (renders/hidden/select-clear invokes `setNetworkFilter`); `RoomAvatar.test.tsx` (badge shown when `network` set, absent when null, initial derived); `chat-row.test.tsx` (badge present for bridged); `conversation-pane` header test (badge + account chip; graceful when room absent); `use-selected-room-vm` test; `client.ts` wrappers (6th channel, `setNetworkFilter`); `chat-list-pane.test.tsx` (6th channel feeds store; Network chip show/dismiss; composition with Space chip; Esc clears both; empty label; resubscribe re-apply; stale reconcile); fixtures updated for the new `network` field in `rooms.test.ts`/`archive-rooms.test.ts`/`pins-rooms.test.ts`/`favorites-rooms.test.ts`/`spaces` tests/`use-sign-out.test.ts` and any Rust `RoomVm`/`InboxRoomVm` builders.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `network: Option<String>` to `RoomVm`+`InboxRoomVm`; NEW `NetworkVm`+`NetworksSnapshot` (`#[ts(export)]`); update builders + regenerate bindings. -- carry per-room Network identity + the list stream shape.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- resolve `network` in `room_item_to_vm` via `bridge::room_bridge_network`; `NetworksSink`; `subscribe_inbox` 6th sink into `InboxMerger::new`; `set_network_filter` routing. -- populate Network identity and wire the filter poke.
- [x] `src-tauri/crates/keeper-core/src/inbox.rs` -- merger `networks_sink`/`selected_network`; `set_network_filter`; `to_inbox_room` copies `network`; `emit` derives+emits the distinct-Networks snapshot pre-filter and applies the Network retain after the Space retain (AND); six-capture fixtures + golden tests. -- compute the Networks list + filtered windows from one merge.
- [x] `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- 6th `inbox_subscribe` channel; `set_network_filter` command + registration. -- expose the Networks stream + filter mutation.
- [x] `src/lib/ipc/client.ts` -- `subscribeInbox(…, onNetworks)`; `setNetworkFilter`. -- typed access to the 6th stream + command.
- [x] `src/lib/stores/networks.ts` -- NEW mirror (list + active Network name). -- hold Networks + the active filter.
- [x] `src/lib/account-initials.ts` + `src/components/layout/account-footer.tsx` -- extract `initials(userId)` to a shared helper. -- one source for the account initial.
- [x] `src/hooks/use-selected-room-vm.ts` -- NEW selected-room lookup across the four window stores. -- feed the header its room identity.
- [x] `src/components/chat/RoomAvatar.tsx` -- render the 16 px uniform Network badge when `network` is set; remove the deferral note. -- attribute every avatar with its Network.
- [x] `src/components/layout/networks-group.tsx` + `sidebar-pane.tsx` -- NEW NETWORKS sidebar group (single-select, hidden when empty), rendered after SPACES. -- list + select Networks.
- [x] `src/components/layout/conversation-pane.tsx` -- header shows room avatar (with Network badge) + name + account-initial chip. -- attribute the conversation header.
- [x] `src/components/layout/chat-list-pane.tsx` -- 6th channel → store; Network chip alongside Space chip; compose AND; clear-all on Esc/✕; empty-state label; resubscribe re-apply; stale reconcile. -- surface the Network filter + composition.
- [ ] Tests -- Rust merger (retain/AND/snapshot dedup/native-exclude/copy); TS store, networks-group, RoomAvatar badge, chat-row, header, use-selected-room-vm, client wrappers, chat-list-pane (chip/compose/Esc/empty/resubscribe/reconcile); update all `network`-field fixtures. -- cover behavior.

**Acceptance Criteria:**
- Given any chat row and the conversation header, when a room is bridged (`network` resolved), then both render a 16 px uniform Network badge overlaid bottom-right on the avatar (2 px ring, showing the Network label initial) plus the Account attribution (3 px hue edge bar on rows, hue-tinted account-initial chip in the header); a native Matrix room shows the Account attribution but no Network badge (FR-24, UX-DR3) — and Network identity never appears as per-row/pane/bubble coloring.
- Given the NETWORKS sidebar group, when the merged inbox contains bridged rooms, then each distinct connected Network is listed once (Rust-deduped, name-sorted, group hidden when none); selecting a Network filters every inbox window to that Network's rooms across all accounts with the four-way split and recency order computed and streamed from Rust — never re-derived in TypeScript — and shows a dismissible chip.
- Given an active Network filter and an active Space filter, when both are set, then the inbox shows their AND intersection and both chips render; when the user clicks the active row again, dismisses a chip, or presses Esc from the chat list, then the filter(s) clear and the full inbox is restored; when the filtered inbox is empty, then "No chats in {filter}." with a Clear filter action is shown.
- Given a code audit, then Network identity is read only from local `m.bridge`/legacy bridge state (no network fetch), the filter is applied inside `InboxMerger::emit` on a name-keyed ephemeral `selected_network` (no registry key, no per-Network icon map, no `network` filtering in TS), the NETWORKS list is derived from the unfiltered merged set, and `.mark_as_read(`/`.send_single_receipt(` remain solely in `signals.rs` (AD-14 guard green).

## Spec Change Log

_No spec amendments — the review pass produced no intent_gap or bad_spec findings; all actionable findings were patches to the implementation._

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 8: (high 0, medium 2, low 6)
- defer: 1: (medium 1)
- reject: 5
- addressed_findings:
  - `[medium]` `[patch]` Any single filter chip's ✕ cleared BOTH composed filters — split into per-chip `clearSpaceFilter`/`clearNetworkFilter` (Space ✕ clears Space only, Network ✕ clears Network only); Esc + empty-state button still clear all; added two chat-list-pane tests.
  - `[medium]` `[patch]` The merger never self-healed a `selected_network` whose Network vanished (asymmetric with `selected_space`), leaving the inbox filtered-empty and relying on the TS reconcile (AD-20 violation) — `emit` now validates `selected_network` against the unfiltered distinct-Networks set and self-clears; updated/added three Rust merger tests.
  - `[low]` `[patch]` `emit_networks` could suppress a four-window tick if the networks channel closed first — the distinct-Networks snapshot is now derived before the retains but pushed LAST (after the four windows emit).
  - `[low]` `[patch]` Badge/sidebar initial comment claimed "grapheme" but takes the first code point — corrected the comment (labels are ASCII protocol names) and added a `?? ""` empty guard in `RoomAvatar` + `networks-group`.
  - `[low]` `[patch]` Unified the not-yet-hydrated Space label fallback ("this Space" vs "Space") to a single "Space".
  - `[low]` `[patch]` `useSelectedRoomVm` re-scanned four window arrays on every header render — wrapped in `useMemo`; documented the window-precedence tie-break.
  - `[low]` `[patch]` Header account-chip test used an ambiguous `getByText("A")` — added `data-testid="account-initial-chip"` and query by testid.
  - `[low]` `[patch]` Documented that two bridges sharing a protocol displayname collapse into one name-keyed Network by design (the label is the cross-account identity key).

### 2026-07-05 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 0
- reject: 19
- addressed_findings:
  - `[low]` `[patch]` The Network badge never rendered at the AC-mandated uniform 16 px: `RoomAvatar` is used at `size="lg"` (row/header) and `size="xl"` (Pins strip), and `AvatarBadge`'s own `group-data-[size=lg|xl]/avatar:size-3(.5)` variants have higher CSS specificity than the plain `size-4` (and tailwind-merge does not dedupe across differing variants), so the badge silently shrank to 12/14 px. Forced 16 px with the Tailwind v4 important modifier (`size-4!`) and added a `toHaveClass("size-4!")` guard to `RoomAvatar.test.tsx` (the prior tests asserted text/aria/title but never size, so the regression shipped untested).
  - reject notes: 19 findings dropped as precedent-consistent, graceful, already-addressed, or already-deferred — e.g. a closed networks channel setting `state.closed` (identical to the accepted Story 4.5 spaces-sink behavior; all six channels are created/torn down together, so an independent close does not occur); the Rust self-heal + frontend `onNetworks` reconcile both clearing a vanished selection (idempotent, mirrors the accepted 4.5 Space reconcile); the empty-state `"Space"` fallback wording (a deliberate prior-pass decision); the mid-`Reset` self-heal race and rapid-toggle chip race (graceful, self-correcting); the truncation-ellipsis Network-name collision (edge of the already-by-design name-collision); the non-ASCII badge-initial glyph (already guarded/`?? ""`, labels are ASCII protocol names); and the per-batch `room_bridge_network` resolution cost — **already captured** in `deferred-work.md` from the initial pass, so not re-deferred.

## Design Notes

**Network is per-room, not per-account.** A single Matrix account (e.g. Beeper) hosts rooms bridged to many Networks. Network is therefore resolved per room from its own `m.bridge` state via the existing `bridge::room_bridge_network` (bridge.rs:74) — the same untrusted, length-capped label used by the delete confirmation — and carried on `RoomVm`. Native rooms resolve to `None` and are honestly badge-less (no fabricated "Matrix" identity; that is a Block-If decision, not assumed here).

**Filter mirrors the Space filter, composes AND.** `set_network_filter` reuses the proven poke-and-re-emit path (`update_pins`/`set_space_filter`): store `selected_network`, `emit()`. In `emit`, the Network retain runs immediately after the Space retain, so both narrow the same pre-partition merged set and precedence (Pins > Favorites > Archive/Inbox) and per-window recency are preserved within the intersection. Unlike Spaces, no producer is needed: the distinct-Networks snapshot is derived from the already-merged rooms each `emit`, so it updates live with sync and stays stable regardless of active filters (derived pre-retain).

**Badge without an icon pipeline.** No curated Network→icon assets exist, so the 16 px badge shows the label's initial in a uniform neutral chip — honest, asset-free, and compliant with "Network identity appears only as badges, never as coloring." A real icon set is a later concern (Block-If).

Golden (RoomAvatar overlay):
```tsx
{room.network !== null && (
  <AvatarBadge className="size-4 bg-secondary text-secondary-foreground text-[9px]"
    aria-label={`${room.network} network`} title={room.network}>
    {[...room.network][0]?.toUpperCase()}
  </AvatarBadge>
)}
```

**Header room lookup + windowing caveat.** The header reads the selected room's `InboxRoomVm` from the four window stores (they stream full windows today), reusing `RoomAvatar` so the header badge and the row badge share one source. If the room is absent (future true windowing, or a filter hid it), the header degrades to the account-initial chip alone (from `accountsStore`, always present) — never a crash.

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` pass (new merger fields/methods, no `.unwrap()`).
- `bun run test:rust` -- expected: inbox merger tests pass (network retain, AND with space, snapshot dedup/sort/native-exclude, `to_inbox_room` copy).
- `bun run check` -- expected: biome + tsc + vitest pass, including regenerated `NetworkVm.ts`/`NetworksSnapshot.ts` bindings and all updated `network`-field fixtures.
- `grep -rn "mark_as_read\|send_single_receipt" src-tauri/crates --include=*.rs | grep -v signals.rs` -- expected: no matches (AD-14 guard).

**Manual checks (if no CLI):**
- With ≥2 accounts and bridged rooms, confirm rows + header show the correct Network badge and account hue/initial; selecting a NETWORKS chip filters across accounts; composing with a SPACES chip shows both chips and the intersection; Esc/✕ restores the full inbox.

## Auto Run Result

Status: done

**Summary:** Implemented Story 4.6 — per-room bridged-Network attribution (a uniform 16 px badge on chat-row avatars and the new conversation header) plus a Rust-authoritative Network filter that composes AND with the Space filter, a NETWORKS sidebar group, and a 6th inbox channel streaming the distinct connected Networks. Network identity is derived only from local `m.bridge` state (`bridge::room_bridge_network`), carried on `RoomVm`/`InboxRoomVm` as `network: Option<String>`; native rooms show no badge. Mirrors the Story 4.5 Space-filter machinery (ephemeral `selected_network`, pre-partition retain, poke-and-re-emit).

**Files changed (one-liners):**
- `keeper-core/src/vm.rs` — `network` field on `RoomVm`+`InboxRoomVm`; new `NetworkVm`/`NetworksSnapshot` (`#[ts(export)]`).
- `keeper-core/src/account.rs` — resolve `network` in `room_item_to_vm`; `NetworksSink`; 6th sink in `subscribe_inbox`; `set_network_filter` routing.
- `keeper-core/src/inbox.rs` — merger `networks_sink`/`selected_network`; `set_network_filter`; `emit` derives distinct Networks (self-heals a stale selection) + Network retain after Space retain (AND) + emits the snapshot last; `distinct_network_names`/`push_networks`; merger tests.
- `keeper/src/ipc.rs` + `lib.rs` — 6th `inbox_subscribe` channel; `set_network_filter` command + registration.
- `src/lib/ipc/client.ts` — `subscribeInbox` 6th `onNetworks` channel; `setNetworkFilter`.
- `src/lib/stores/networks.ts` — new mirror store (list + active Network).
- `src/lib/account-initials.ts` — extracted shared `initials(userId)` (re-imported in account-footer + header).
- `src/hooks/use-selected-room-vm.ts` — memoized selected-room lookup for the header.
- `src/components/chat/RoomAvatar.tsx` — render the Network badge (uniform neutral, first code point).
- `src/components/layout/networks-group.tsx` + `sidebar-pane.tsx` — NETWORKS sidebar group after SPACES.
- `src/components/layout/conversation-pane.tsx` — header identity block (avatar+badge, name, account chip).
- `src/components/layout/chat-list-pane.tsx` — 6th channel → store; per-chip dismissal; AND composition; Esc clears all; resubscribe re-apply; reconcile.
- `gen/*.ts` — regenerated bindings (`NetworkVm`, `NetworksSnapshot`, `RoomVm`, `InboxRoomVm`).
- Tests — merger, store, networks-group, RoomAvatar/chat-row badge, header, use-selected-room-vm, client wrappers, chat-list-pane; all `network`-field fixtures updated.

**Review findings breakdown:** 8 patches applied (2 medium — per-chip filter dismissal, Rust self-heal of a stale Network selection; 6 low — emit ordering, badge code-point comment + empty guard, label fallback, `useSelectedRoomVm` memoization, robust header test, collision doc). 1 deferred (per-room bridge-state read on every batch — perf/measurement). 5 rejected (redundant emit, IPC positional-channel suggestion, lock ordering, header null-null degrade, brief reconcile flash — all precedent-consistent or graceful).

**Follow-up review pass (2026-07-05):** An independent follow-up review (Blind Hunter + Edge Case Hunter) surfaced one actionable low-severity patch — the Network badge rendered at 12/14 px instead of the AC-mandated uniform 16 px because `AvatarBadge`'s `group-data-[size=lg|xl]/avatar` size variants out-specified the plain `size-4` override on the `lg`/`xl` avatars it is used on. Fixed with `size-4!` (Tailwind v4 important) plus a size-class test guard. The remaining 19 findings were rejected as precedent-consistent, graceful, already-addressed, or already-deferred (the per-batch `room_bridge_network` cost is already in `deferred-work.md`). No intent_gap or bad_spec, no new deferrals.

**Follow-up review recommended:** false — the follow-up pass made a single localized low-consequence CSS/cosmetic fix (badge size) plus a test assertion; nothing behavior/API/data-affecting warrants another independent pass.

**Verification:** Initial pass — `bun run check:rust` PASS (rustfmt + clippy `-D warnings`); `bun run test:rust` PASS (327 tests). Follow-up pass — `bun run check` PASS (biome clean + tsc clean + vitest 547/547, incl. the new `size-4!` guard); Rust untouched this pass (no `.rs` changes). AD-14 guard green (no `mark_as_read`/`send_single_receipt` outside `signals.rs`).

**Residual risks:** (1) Per-room Network resolution cost on large accounts is deferred (functionally correct; local reads). (2) Network identity uses the bridge protocol displayname as both key and label — distinct bridges sharing a name collapse into one Network by design. (3) Native-Matrix rooms have no Network chip/badge (intentional: no fabricated identity); a first-class "Matrix" Network was flagged as a human decision, not assumed.
