---
title: 'Bridges Surface with Data-Driven Risk Tiers'
type: 'feature'
created: '2026-07-05'
status: 'done'
baseline_revision: 'b18a8e70a56d8c8f70f9880d9b541efffaa3cb63'
final_revision: '7ad00d5e23940377180f83df3834be415b9ca3d5'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** keeper has no Bridges surface and no honest, data-driven risk labeling, so a user cannot see what they are signing up for before connecting a Network. Story 6.1 is the foundation for all of Epic 6.

**Approach:** Ship versioned JSON data files (risk tiers, coupling caveats, known-bot registry) under `src-tauri/crates/keeper-core/data/`, consumed and validated by a new keeper-core `bridges` module that exposes a data-driven bridge catalog over a Tauri command. Render a read-only Bridges primary view: one Bridge card per Network × Account (glyph, name, risk-tier badge from the data, health-dot placeholder, primary action), a worst-state sidebar health roll-up, and a volatile-tier `AlertDialog` connect gate.

## Boundaries & Constraints

**Always:**
- All risk/tier/caveat copy and badge mapping come from the JSON data files consumed by keeper-core — never hardcoded in TypeScript. The frontend renders only backend view models.
- Match the addendum §2 risk-tier table exactly for the tier set and per-tier network lists (Low, Maintenance-heavy, Volatile, Conditional, Out-of-scope).
- No `.unwrap()`/bare `.expect()` in Rust production paths; embedded JSON is parsed through `Result` with a `thiserror` error, no panics. Data files embedded with `include_str!`.
- Out-of-scope tier networks stay in the data file (for completeness) but are NOT surfaced as connectable cards.
- Cards are keyed Network × Account; account identity is the existing `AccountVm.accountId`.

**Block If:**
- A required data value (a network's tier, the tier→badge mapping, or the volatile ToS/ban acknowledgment copy) cannot be sourced from the planning artifacts and would have to be invented.

**Never:**
- No bridge discovery, provisioning/bot login, or real health monitoring — those are Stories 6.2/6.3/6.4/6.5. Health is a placeholder here; the primary action opens the volatile gate (or is a no-op stub) but does not perform a login.
- Do not modify the existing singular `bridge.rs` module (Story 3.8 label resolution); add a new `bridges/` module.
- No Matrix/network I/O; the catalog is static data.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Load catalog | `bridge_catalog` invoked; valid embedded JSON | `Vec<BridgeNetworkVm>` excluding out-of-scope tier; each carries `tier`, `tierLabel`, `badgeStyle`, `requiresAck`, and `ackCopy` (Some only when `requiresAck`) | No error expected |
| Malformed data file | An embedded JSON file fails to parse at runtime | Command returns an `IpcError`; Bridges view shows an error state | Parse via `Result`; no panic |
| Volatile connect | User clicks primary action on a volatile card | `AlertDialog` shows the tier badge + `ackCopy`; proceeds only after "I understand the risk — connect" | Cancel closes dialog, no side effect |
| Low-risk connect | User clicks primary action on a low-risk card | Proceeds directly (no dialog) | n/a |
| No accounts | Bridges view renders with zero signed-in accounts | Empty state prompting to add an account; no cards | n/a |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/data/risk-tiers.json` -- NEW; versioned tiers → networks, badge style, ack copy (matches addendum §2).
- `src-tauri/crates/keeper-core/data/coupling-caveats.json` -- NEW; per-network coupling caveats (seed: WhatsApp read-receipt coupling); consumed later by FR-44.
- `src-tauri/crates/keeper-core/data/known-bots.json` -- NEW; network → candidate bot localparts (seed: standard mautrix defaults); consumed later by 6.2 discovery.
- `src-tauri/crates/keeper-core/src/bridges/mod.rs` -- NEW module; `catalog()` → `Vec<BridgeNetworkVm>`; register in `lib.rs`.
- `src-tauri/crates/keeper-core/src/bridges/data.rs` -- NEW; `include_str!` + serde deserialize + `OnceLock` cache + validation for all three files.
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `BridgeNetworkVm`, `RiskTier`, `BadgeStyle` (ts-rs `#[ts(export)]`, camelCase).
- `src-tauri/crates/keeper-core/src/error.rs` -- add `BridgeError` variant; roll into `CoreError`.
- `src-tauri/crates/keeper/src/ipc.rs` -- add `#[tauri::command] bridge_catalog`; map `BridgeError` in `to_ipc_error`.
- `src-tauri/crates/keeper/src/lib.rs` -- register `bridge_catalog` in `invoke_handler`.
- `src/lib/ipc/client.ts` -- add `bridgeCatalog()` wrapper + re-export `BridgeNetworkVm`.
- `src/lib/stores/primary-view.ts` -- add `"bridges"` to `PrimaryView`.
- `src/components/layout/sidebar-pane.tsx` -- wire Bridges entry → `setView("bridges")`, active state, worst-state roll-up dot.
- `src/components/layout/app-shell.tsx` -- render `BridgesPane` when `primaryView === "bridges"`.
- `src/components/layout/bridges-pane.tsx` -- NEW; renders cards per Account × Network; empty state.
- `src/components/bridges/bridge-card.tsx` -- NEW; card + volatile `AlertDialog` gate.
- `src/hooks/use-bridge-catalog.ts` -- NEW; fetch catalog once via IPC.
- `src/hooks/use-bridges-shortcut.ts` -- NEW; ⌘4 → `setView("bridges")` (mirror `use-search-shortcuts.ts`).

## Tasks & Acceptance

**Execution:**
- [x] `data/risk-tiers.json`, `data/coupling-caveats.json`, `data/known-bots.json` -- author the three versioned files (`"version": 1`) matching addendum §2; tiers carry `id`, `label`, `badge`, `requiresAck`, `acknowledgment`, `surfaced`, and `networks[]{id,name,glyph}`.
- [x] `bridges/data.rs` -- `include_str!` each file, deserialize into typed structs, cache in `OnceLock`, return `Result<_, BridgeError>`; validate on parse.
- [x] `bridges/mod.rs` -- `catalog()` maps tier data → flat `Vec<BridgeNetworkVm>`, excluding `surfaced == false` tiers; register module in `lib.rs`.
- [x] `vm.rs` -- define `BridgeNetworkVm { networkId, name, glyph, tier, tierLabel, badgeStyle, requiresAck, ackCopy }`, `RiskTier`, `BadgeStyle`; add ts-rs round-trip/export test.
- [x] `error.rs` -- add `BridgeError::Data(String)`, `CoreError::Bridge`.
- [x] `ipc.rs` + `lib.rs` -- add and register `bridge_catalog` command; map `BridgeError` to an `IpcError`.
- [x] `bridges/data.rs` (tests) -- unit-test the I/O matrix data cases: all files parse; volatile tier `requiresAck` with non-empty `acknowledgment`; low tier not; out-of-scope `surfaced == false`; `catalog()` excludes it; every network has non-empty `name`/`glyph`; each known-bot entry has ≥1 localpart.
- [x] `client.ts` / `use-bridge-catalog.ts` -- typed `bridgeCatalog()` fetch hook.
- [x] `primary-view.ts` / `sidebar-pane.tsx` / `app-shell.tsx` -- add `"bridges"` view, wire sidebar entry active state + click, worst-state roll-up dot, render `BridgesPane`.
- [x] `bridges-pane.tsx` + `bridge-card.tsx` -- render Account × Network cards (glyph, name, data-driven badge, placeholder health dot, primary action); volatile `AlertDialog` gate; empty state; colocated `*.test.tsx`.
- [x] `use-bridges-shortcut.ts` -- ⌘4 shortcut.

**Acceptance Criteria:**
- Given the repo, when the story lands, then `src-tauri/crates/keeper-core/data/` holds versioned JSON for risk tiers, coupling caveats, and the known-bot registry, parsed and validated by keeper-core with passing unit tests, and no tier/caveat/badge copy is hardcoded in TypeScript.
- Given the Bridges view rendered with ≥1 account, when it renders, then each surfaced Network × Account shows a Bridge card (glyph, name, data-driven risk-tier badge, placeholder health dot, primary action), and the sidebar Bridges entry shows a worst-state health roll-up dot.
- Given a volatile-tier Network, when the user initiates connect, then an `AlertDialog` with the tier badge and the data-file acknowledgment copy requires "I understand the risk — connect" before proceeding, while a low-risk Network proceeds with no dialog.
- Given `bun run check:all`, when run, then Biome + tsc + vitest + rustfmt + clippy (`-D warnings`) + cargo-nextest all pass and `BridgeNetworkVm.ts` is generated under `src/lib/ipc/gen/`.

## Design Notes

- Path mapping: the story's `crates/keeper-core/` is `src-tauri/crates/keeper-core/` in this repo.
- `include_str!` embeds the JSON at compile time (no runtime file-not-found path); parse once into a `OnceLock`. "Versioned" = an in-file `"version": 1` field plus git history.
- Tier→badge mapping is data (`badge` field), surfaced as the `BadgeStyle` enum (`secondary` → Low, `outlineDegraded` → Maintenance-heavy, `filledDisconnected` → Volatile, `outline` → Conditional). The card maps `BadgeStyle` to the shadcn `Badge` variant / `--bridge-*` tokens (already in `src/index.css`).
- Health is a neutral placeholder (real state machine is 6.5); the roll-up helper computes worst-state from card health so it is ready, and shows no/neutral dot when nothing is configured.
- Discovery (6.2) will later replace the static per-account catalog projection with real per-homeserver status; keep the catalog account-agnostic so 6.2 can layer status on top.
- `known-bots.json` is seeded with the well-known public mautrix bot localparts (factual defaults, extensible in 6.2), not invented planning content.

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` pass.
- `bun run test:rust` -- expected: cargo-nextest green, including new `bridges` data tests; ts-rs exports `BridgeNetworkVm.ts`.
- `bun run check` -- expected: Biome + tsc + vitest pass, including new bridge-card / bridges-pane tests.

## Spec Change Log

_No bad_spec loopbacks: the review produced only localized patches, applied directly to the diff._

## Review Triage Log

### 2026-07-05 — Follow-up review pass
- intent_gap: 0
- bad_spec: 0
- patch: 5: (high 0, medium 0, low 5)
- defer: 0
- reject: 16: (high 0, medium 0, low 16)
- addressed_findings:
  - `[low]` `[patch]` `error.rs` + `bridges/data.rs`: derived `Clone` on `BridgeError` and simplified `as_ref_result` to `cached.as_ref().map_err(Clone::clone)`, removing the fragile non-exhaustive `match Err(BridgeError::Data(..))` that would silently break the day a second variant is added.
  - `[low]` `[patch]` `bridges/data.rs`: removed the now-stale `#[allow(dead_code)]` on the three `version` fields (they are genuinely read by the validators since the prior pass added the `version == 1` checks); the attribute was masking real usage and would hide a truly-unused field later.
  - `[low]` `[patch]` `bridges/data.rs`: `validate_known_bots` now rejects a duplicate `networkId` across entries (Story 6.2 joins the registry to the catalog by `networkId`, so a duplicate would make that join ambiguous), mirroring the surfaced-network-id rigor already applied to `risk-tiers.json`.
  - `[low]` `[patch]` `bridges/data.rs` (tests): added 14 negative validation tests that build deliberately-bad in-memory docs and drive every rejection branch to `Err` — unsupported version (×3 files), empty tiers, empty tier id/label, `surfaced ⇔ known-tier` mismatch (both directions), require-ack-but-empty-copy, surfaced-empty-networks, empty network field, duplicate surfaced network id, out-of-scope-accepted-only-when-unsurfaced, empty caveat field, empty known-bot networkId, no-valid-localparts, duplicate known-bot networkId. Closes the exact residual risk the prior pass flagged (validation branches previously exercised only against correct data).
  - `[low]` `[patch]` `bridges/mod.rs` (tests): added a `coupling-caveats.networkId ⊆ catalog` cross-file test mirroring the existing known-bots one, so a typo'd caveat networkId fails loudly now instead of silently never matching in Epic 8 (FR-44).
- rejected (noise / unreachable / design-approved): `BADGE_STYLE[badgeStyle]` undefined-crash and `ackCopy` null empty-description (both backend-guaranteed by the typed enum + `validate_risk_tiers`, near-unreachable; the enum-crash was already rejected in the prior pass), empty-catalog bare headers (unreachable with correct embedded data), ⌘4-while-dialog-open (harmless — the gate lives in the Bridges pane, so ⌘4 is a no-op there), `worstBridgeHealth([])` dead code + precedence test and duplicated health-dot rendering (Design-Note placeholder; Story 6.5 owns real health and reworks the area — prior-pass rejected), `tier_from_id` wildcard "only literal out-of-scope" tightening (over-constraining; the new negative tests already pin the surfaced⇔known invariant), "unavailable right now" error copy (near-unreachable embedded-asset path), refetch-per-mount IPC churn (negligible for immutable embedded data), ⌘4 discoverability (mirrors `useSearchShortcuts`), AlertDialog tier-badge outside announced content (the `AlertDialogDescription` already announces the risk ack copy; badge is decorative reinforcement), health-dot `aria-hidden` (placeholder; 6.5), docstring "bytes" wording (cosmetic), tautological volatile-"proceeds" test and no-frontend-out-of-scope test (nothing to proceed to until 6.3; Rust test already guards exclusion), `accountId`-only-a-data-attribute (intentional scaffolding for 6.3 login dispatch).

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 10: (high 0, medium 0, low 10)
- defer: 0
- reject: 7: (high 0, medium 0, low 7)
- addressed_findings:
  - `[low]` `[patch]` `bridge-card.tsx`: removed the redundant `ackCopy !== null` render guard that could leave a `requiresAck` action a silent no-op; the AlertDialog now renders on `requiresAck` alone.
  - `[low]` `[patch]` `bridge-card.tsx`: volatile `filledDisconnected` badge now renders a filled disconnected-red badge (UX-DR8) instead of a 10% tint.
  - `[low]` `[patch]` `use-bridges-shortcut.ts`: ⌘4 now ignores the chord while an input/textarea/select/contenteditable is focused, so it can't yank the user out mid-typing.
  - `[low]` `[patch]` `bridges/data.rs`: validate the schema `version == 1` for all three embedded data files.
  - `[low]` `[patch]` `bridges/data.rs`: reject duplicate surfaced network ids (would collide the Network × Account card key).
  - `[low]` `[patch]` `bridges/data.rs`: reject a surfaced tier with no networks.
  - `[low]` `[patch]` `bridges/data.rs`: enforce `surfaced ⇔ known-tier`, so mis-hiding the safety-critical volatile tier fails loudly instead of silently dropping risk copy.
  - `[low]` `[patch]` `bridges/mod.rs`: added a lock-in test pinning the exact surfaced network set per tier to addendum §2 (regression guard for the central Always rule).
  - `[low]` `[patch]` `bridges/mod.rs` + `known-bots.json`: reconciled the `googlechat`→`google` networkId to the catalog (fixes a latent Story 6.2 join miss) and added a known-bots⊆catalog cross-file test.
  - `[low]` `[patch]` `risk-tiers.json`: fixed the Low-tier ack-copy typo "Recommend"→"Recommended".
- rejected (noise / unreachable / design-approved): raw-serde/empty error-string rendering (near-unreachable embedded-asset path, non-secret), empty-catalog & error-vs-no-accounts precedence & offline (unreachable with correct data), glyph-length / unknown-`badgeStyle` crash (impossible with the typed enum + controlled data), sidebar dot double-guard cosmetics (correct; 6.5 reworks the area), `worstBridgeHealth` untested placeholder (6.5 owns real health), `BADGE_STYLE`-in-TS (Design-Note-sanctioned seam), dead non-ack acknowledgment data (not a defect).

## Auto Run Result

Status: done

**Summary:** Shipped the data-driven Bridges surface. Three versioned JSON data files (`risk-tiers.json`, `coupling-caveats.json`, `known-bots.json`) under `src-tauri/crates/keeper-core/data/` are the single source of truth, embedded via `include_str!`, parsed once into `OnceLock`, and validated on first access (no `.unwrap()`; failures funnel through `BridgeError`). A new `bridges/` core module projects the surfaced tiers into a flat `BridgeNetworkVm` catalog (out-of-scope excluded), exposed by the one-shot `bridge_catalog` Tauri command. The React Bridges primary view renders a Bridge card per Network × Account (glyph, name, data-driven risk badge, placeholder health dot, primary action), gates volatile/conditional connects behind an AlertDialog carrying the data-file acknowledgment copy and the "I understand the risk — connect" confirm, wires the sidebar entry + worst-state health roll-up + ⌘4, and shows honest empty/error states — all as a pure renderer of the backend VM.

**Files changed:**
- `src-tauri/crates/keeper-core/data/{risk-tiers,coupling-caveats,known-bots}.json` — new versioned data files (addendum §2).
- `src-tauri/crates/keeper-core/src/bridges/{mod,data}.rs` — new module: embed, parse, validate, cache, and project the catalog (+ unit tests).
- `src-tauri/crates/keeper-core/src/{lib,vm,error}.rs` — register module; add `BridgeNetworkVm`/`RiskTier`/`BadgeStyle` VMs (ts-rs) + round-trip tests; add `BridgeError` → `CoreError::Bridge`.
- `src-tauri/crates/keeper/src/{ipc,lib}.rs` — `bridge_catalog` command + `IpcErrorCode::Internal` mapping + invoke-handler registration.
- `src/lib/ipc/gen/{BridgeNetworkVm,RiskTier,BadgeStyle}.ts` — generated bindings.
- `src/lib/ipc/client.ts` — `bridgeCatalog()` wrapper + re-exports.
- `src/lib/stores/primary-view.ts` — add `"bridges"` view.
- `src/components/layout/{app-shell,sidebar-pane}.tsx` — route the Bridges view; sidebar entry active state + worst-state roll-up dot.
- `src/components/layout/bridges-pane.tsx` (+ test) — the Bridges surface (per-account × network cards, empty/error/loading states).
- `src/components/bridges/bridge-card.tsx` (+ test) — the Bridge card + volatile AlertDialog gate.
- `src/hooks/{use-bridge-catalog,use-bridges-shortcut}.ts` — catalog fetch hook + ⌘4 shortcut.

**Review findings breakdown:** 10 patches applied (all low severity — see Review Triage Log); 0 deferred; 7 rejected as noise/unreachable/design-approved. No intent gaps, no spec-repair loopbacks.

**Verification:**
- `bun run check:rust` — PASS (rustfmt clean + clippy `-D warnings`).
- `bun run test:rust` — PASS (433 tests, incl. 2 new bridge regression tests; regenerated `BridgeNetworkVm.ts`).
- `bun run check` — PASS (Biome + tsc + 608 vitest tests).

All three gates re-run independently by the workflow after the review patches.

**Residual risks:** The new risk-data validation branches (version, duplicate-id, empty-tier, `surfaced ⇔ known-tier`) are exercised only against the correct embedded data — they lack dedicated negative tests proving they reject bad data — hence `followup_review_recommended: true`. Health is an intentional placeholder (Story 6.5 owns the real state machine, so the sidebar roll-up shows no dot yet); the primary action is a stub (Story 6.3 owns real provisioning login); `known-bots.json` is a partial seed (Story 6.2 extends discovery).

### 2026-07-05 — Follow-up review pass

Status: done

**Summary:** Independent follow-up review (Blind Hunter + Edge Case Hunter, run in parallel). It closed the residual risk that triggered it: the validation branches now have dedicated **negative tests** proving they reject bad data. Applied 5 low-severity patches, all in the Rust validation/test layer — no production behaviour, API, or frontend change beyond a trivial `Clone` derive and a dead-code-attribute cleanup, plus one new malformed-data validation branch (duplicate known-bot `networkId`).

**Patches applied (all low):**
- `error.rs` + `bridges/data.rs` — derived `Clone` on `BridgeError`; `as_ref_result` is now the variant-agnostic `cached.as_ref().map_err(Clone::clone)` (removed a fragile non-exhaustive match).
- `bridges/data.rs` — removed stale `#[allow(dead_code)]` on the three `version` fields (they are read by the validators).
- `bridges/data.rs` — `validate_known_bots` now rejects a duplicate `networkId` (protects the Story 6.2 join).
- `bridges/data.rs` (tests) — 14 negative validation tests driving every rejection branch to `Err`.
- `bridges/mod.rs` (tests) — a `coupling-caveats.networkId ⊆ catalog` cross-file test mirroring the known-bots one.

**Review findings breakdown:** 5 patches applied; 0 deferred; 16 rejected (noise/unreachable/design-approved — see Review Triage Log). No intent gaps, no spec-repair loopbacks.

**Verification (all re-run independently):**
- `bun run check:rust` — PASS (rustfmt clean + clippy `-D warnings`).
- `bun run test:rust` — PASS (449 tests, up from 433: the new negative + cross-file tests).
- `bun run check` — PASS (Biome + tsc + 608 vitest tests + core-tauri-free gate).

**Residual risks:** None new. The prior residual (untested validation branches) is now closed, so `followup_review_recommended` is set to `false`. The intentional placeholders remain out of scope by design: health is a placeholder (Story 6.5), the primary connect action is a stub (Story 6.3), and `known-bots.json` is a partial seed (Story 6.2).
