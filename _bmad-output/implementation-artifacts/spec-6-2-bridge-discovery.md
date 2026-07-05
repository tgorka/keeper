---
title: 'Bridge Discovery'
type: 'feature'
created: '2026-07-05'
status: 'done'
baseline_revision: '333c8076cad90b176e23d689e93676bf90ffdbca'
final_revision: '7f5803afb7f1e92a492802a4494e6358eb10a6f2'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** The Bridges surface (Story 6.1) is a static catalog projection — it shows every surfaced Network as a card regardless of whether that Network's bridge actually exists on the account's homeserver, with a placeholder health dot. A user cannot see which bridges are really on their homeserver or their real setup status, and must know no bot Matrix IDs. FR-25/AD-16 require zero-config discovery.

**Approach:** Add a per-Account discovery pass in `keeper-core` that merges three sources — (a) `GET /_matrix/client/v3/thirdparty/protocols`, (b) a known-bot MXID existence probe (profile lookup over `known-bots.json` localparts), and (c) a scan of joined rooms for `m.bridge` portals and bot DMs — into a per-Network status (`configured` / `not logged in` / `logged in`), keyed Network × Account. Expose it over a one-shot `bridge_discover(accountId)` Tauri command. The Bridges pane replaces its static catalog projection with the discovered set (each discovered Network joined to the 6.1 catalog for glyph/name/tier/badge/ackCopy), rendering an honest "No bridges found on {homeserver}." empty state with a companion-stack docs link when discovery finds none.

## Boundaries & Constraints

**Always:**
- Discovery is zero-config and per-Account: no user ever names a bot MXID. Cards stay keyed Network × Account (`accountId` + `networkId`).
- All three sources contribute and are **merged**; the result is best-effort. A homeserver that does not implement `thirdparty/protocols` (404 / `M_UNRECOGNIZED`) is normal — degrade to sources (b) and (c), never fail discovery.
- Status derivation is honest and evidence-based (pure function): `m.bridge` portal room for the Network → **logged in**; else a bot DM / management room with a known bot but no portal → **not logged in**; else present only via protocols list or a resolving known-bot MXID → **configured**; else the Network is not discovered (no card).
- Discovered Networks are **catalog-gated**: only Networks present in the 6.1 `catalog()` (surfaced tiers) are surfaced as cards; join by `networkId`. `m.bridge` `protocol.id` and `thirdparty/protocols` keys map directly to catalog `networkId`s (as reconciled in 6.1, e.g. `google`).
- The frontend stays a pure renderer of the discovery VM: it renders discovered status; it invents no tier/copy/status text. Risk-tier badges and ack copy still come only from the 6.1 catalog data.
- No `.unwrap()` / bare `.expect()` in Rust production paths; individual source failures (a failed protocol probe, one unreachable profile lookup) are logged via `tracing` and skipped, never panic and never abort the whole discovery.
- Reuse `bridge::parse_bridge_network_name` semantics for reading `m.bridge` / `uk.half-shot.bridge` state; do not reimplement bridge-state parsing.

**Block If:**
- The three discovery sources cannot be mapped to catalog `networkId`s from existing data (`known-bots.json` + `risk-tiers.json`) and correct status would require inventing a Network identity or a fourth status not in the AC.

**Never:**
- No provisioning/bot **login** or the `BridgeTransport` trait — that is Story 6.3/6.4. Discovery observes state; it never initiates a login. The card's primary action remains the 6.1 stub (volatile ack gate → no-op) until 6.3.
- No live health state machine (healthy/degraded/disconnected) or 60 s polling — that is Story 6.5. Discovery is a point-in-time probe; the live health dot stays the 6.1 placeholder.
- No fabricated companion-stack docs URL or fake hosted service; the link points at keeper's real repository docs.
- Do not surface discovered protocols that have no catalog entry as cards (keeper has no vetted risk data for them); log them via `tracing` for a future story.
- No new IPC streaming channel; discovery is a one-shot command.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Two bridges present | Homeserver has mautrix-whatsapp + mautrix-telegram; `bridge_discover(acct)` | `BridgeDiscoveryVm` lists both, each with derived status; `homeserver` set to the account's server name | No error expected |
| Portal exists | A room carries `m.bridge` with `protocol.id = "whatsapp"` | WhatsApp status = **logged in** | n/a |
| Bot DM, no portal | A DM whose direct target is `@whatsappbot:hs`, no portal room | WhatsApp status = **not logged in** | n/a |
| Protocol only | `thirdparty/protocols` lists `signal`; no DM, no portal | Signal status = **configured** | n/a |
| Protocols unsupported | `thirdparty/protocols` returns 404 / `M_UNRECOGNIZED` | Discovery proceeds on sources (b)+(c) only | Logged; not surfaced as an error |
| MXID probe absent | `get_profile(@telegrambot:hs)` → `M_NOT_FOUND` | Telegram not marked configured via source (b) | 404 → absent, not an error |
| No bridges | Discovery finds zero catalog Networks | `networks: []`; pane shows "No bridges found on {homeserver}." + companion-stack docs link | n/a |
| Uncatalogued protocol | `protocols` lists a Network absent from the catalog | Not surfaced as a card; logged via `tracing` | n/a |
| Unknown account | `bridge_discover("bogus")` | Command returns an `IpcError` | Account lookup miss → `BridgeError` |
| Transport failure | Homeserver unreachable during discovery | `IpcError` (retriable); pane shows a retryable error state | Only a total failure errors; partial-source failures degrade |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/data/known-bots.json` -- verify localparts cover the discoverable Networks (source b/c join key); extend seed if a tested Network is missing.
- `src-tauri/crates/keeper-core/src/bridges/discovery.rs` -- NEW; the discovery engine: fetch protocols, probe MXIDs, scan rooms, and a **pure** `merge_discovery(...)` mapping per-Network evidence → `BridgeStatus`. Impure Matrix I/O in a thin shell; pure merge unit-tested.
- `src-tauri/crates/keeper-core/src/bridges/mod.rs` -- register `discovery` submodule; expose a `discover(client, catalog, known_bots) -> Result<BridgeDiscoveryVm, BridgeError>` entry (or via `AccountManager`).
- `src-tauri/crates/keeper-core/src/bridge.rs` -- reuse `parse_bridge_network_name` for `m.bridge` `protocol.id` extraction (make the needed helper reachable from `bridges::discovery`).
- `src-tauri/crates/keeper-core/src/account.rs` -- add `AccountManager::discover_bridges(&self, account_id) -> Result<BridgeDiscoveryVm, CoreError>`: lock the map, clone the `Client`, run discovery.
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `BridgeDiscoveryVm { homeserver, networks }`, `DiscoveredBridgeVm { networkId, status }`, `BridgeStatus { LoggedIn | NotLoggedIn | Configured }` (ts-rs `#[ts(export)]`, camelCase) + round-trip test.
- `src-tauri/crates/keeper-core/src/error.rs` -- extend `BridgeError` with a discovery variant (unknown account / homeserver failure) distinct from `Data`.
- `src-tauri/crates/keeper/src/ipc.rs` -- add `#[tauri::command] async fn bridge_discover(state, account_id)`; map the new `BridgeError` arm in `to_ipc_error` (retriable for transport).
- `src-tauri/crates/keeper/src/lib.rs` -- register `bridge_discover` in `invoke_handler`.
- `src/lib/ipc/client.ts` -- add `bridgeDiscover(accountId)` wrapper + re-export `BridgeDiscoveryVm`/`DiscoveredBridgeVm`/`BridgeStatus`.
- `src/hooks/use-bridge-discovery.ts` -- NEW; per-account discovery fetch (loading/error/data), unmount-guarded like `use-bridge-catalog.ts`.
- `src/components/layout/bridges-pane.tsx` -- per account: fetch discovery; render a card per discovered Network (join catalog by `networkId`); loading/error states; empty state "No bridges found on {homeserver}." + `COMPANION_STACK_DOCS_URL` link (mirror `SSS_DOC_URL` `<a target="_blank" rel="noreferrer">`).
- `src/components/bridges/bridge-card.tsx` -- accept a `status: BridgeStatus`; render the discovery status word + dot (Connected / Action needed / Not set up) in place of the placeholder; keep the volatile ack gate + primary-action stub.
- `src/lib/bridges.ts` (or colocated) -- `COMPANION_STACK_DOCS_URL` constant (repo docs; single point to update) + a `BridgeStatus → label` map; colocated `*.test.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `vm.rs` -- define `BridgeDiscoveryVm`, `DiscoveredBridgeVm`, `BridgeStatus` (ts-rs export, camelCase); add round-trip/export test.
- [x] `error.rs` -- add a `BridgeError` discovery variant (e.g. `Discovery(String)` and/or `AccountNotFound`); keep `Data` unchanged.
- [x] `bridges/discovery.rs` -- implement the three-source engine + pure `merge_discovery`: call `client.send(get_protocols::v3::Request::new())` (tolerate failure → empty); scan `client.joined_rooms()` for `m.bridge` `protocol.id` portals and `is_direct()`/`direct_targets()` bot DMs against `known-bots.json` localparts; probe `client.send(get_profile::v3::Request::new(mxid))` only for still-unfound Networks (`Ok` → present, `client_api_error_kind()==Some(NotFound)` → absent, other/None → skip+log); catalog-gate; derive status.
- [x] `bridges/mod.rs` + `account.rs` -- register submodule; add `AccountManager::discover_bridges(account_id)` returning `BridgeDiscoveryVm` (homeserver = account server name).
- [x] `bridge.rs` -- expose the `m.bridge` `protocol.id` extraction for reuse by discovery without duplicating parse logic.
- [x] `ipc.rs` + `lib.rs` -- add and register `bridge_discover`; map the new error arm in `to_ipc_error`.
- [x] `bridges/discovery.rs` (tests) -- unit-test the I/O matrix on the pure merge: portal→logged in, bot-DM-no-portal→not logged in, protocol-only→configured, unfound→absent, uncatalogued→dropped, and the merge precedence (portal beats DM beats protocol/mxid).
- [x] `client.ts` / `use-bridge-discovery.ts` -- typed `bridgeDiscover(accountId)` wrapper + per-account fetch hook.
- [x] `bridges.ts` (+ test) -- `COMPANION_STACK_DOCS_URL` + `BridgeStatus`→label helper.
- [x] `bridges-pane.tsx` (+ test) -- replace the static catalog projection with per-account discovery: discovered cards (catalog-joined), loading/error, and the "No bridges found on {homeserver}." + docs-link empty state.
- [x] `bridge-card.tsx` (+ test) -- render the discovery status word + dot from the `status` prop; keep ack gate.

**Acceptance Criteria:**
- Given a homeserver with mautrix-whatsapp and mautrix-telegram registered, when discovery runs for a connected Account, then both Bridges appear keyed Network × Account with a status derived from the merged `thirdparty/protocols` + known-bot MXID probe + bot-DM/portal scan, with no bot MXID named by the user (FR-25, AD-16).
- Given a homeserver on which discovery finds no catalog bridges, when the Bridges view renders, then it shows "No bridges found on {homeserver}." with a companion-stack docs link, and no cards (FR-25, UX-DR13).
- Given multiple Accounts, when the pane renders, then discovery runs per Account and cards are keyed Network × Account.
- Given a homeserver that does not implement `thirdparty/protocols`, when discovery runs, then it still returns bridges found via the known-bot probe and room scan rather than erroring.
- Given `bun run check:all`, when run, then Biome + tsc + vitest + rustfmt + clippy (`-D warnings`) + cargo-nextest all pass and `BridgeDiscoveryVm.ts` / `DiscoveredBridgeVm.ts` / `BridgeStatus.ts` are generated under `src/lib/ipc/gen/`.

## Spec Change Log

_No bad_spec loopbacks: the review produced only localized patches, applied directly to the diff._

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 1, low 1)
- defer: 0
- reject: 8: (high 0, medium 0, low 8)
- addressed_findings:
  - `[medium]` `[patch]` `bridges/discovery.rs`: `fetch_protocol_ids` swallowed *every* `thirdparty/protocols` error as "endpoint unsupported", so a genuinely unreachable homeserver returned `Ok(empty)` and the pane falsely rendered the honest "No bridges found on {homeserver}." empty state — contradicting the spec I/O-matrix "Transport failure → IpcError (retriable)" row and leaving the frontend's `syncUnavailable` retry path unreachable. Added the pure `protocols_error_degrades(Option<&ErrorKind>)` classifier (only `M_NOT_FOUND` / `M_UNRECOGNIZED` degrade; a transport failure with no client-API kind, or any other errcode, returns `BridgeError::Discovery`), made `fetch_protocol_ids` return `Result`, propagated it from `discover`, and added a unit test pinning `NotFound`/`Unrecognized` → degrade and `None` (transport) → fatal. Now "couldn't check" surfaces a retriable error instead of masquerading as "no bridges", and the dead-server case short-circuits before the bot probes.
  - `[low]` `[patch]` `bridges/discovery.rs`: `bot_network_for` re-read `client.user_id()` per DM target (a redundant lock/read) and silently no-op'd on a mid-scan `None`, inconsistent with `discover`'s top-level hard error for the identical precondition. Resolved the account server name once in `discover` and threaded `&ServerName` down through `scan_rooms` into `bot_network_for`, making it a pure lookup.
- rejected (noise / unreachable / design-approved): `mxid_resolves → Configured` "should be NotLoggedIn" (the spec's stated derivation: a resolving bot MXID with no DM/portal is *configured on the server*, not user-engaged), `configured → "Not set up"` label "contradiction" (a sanctioned epic state word; honest from the user's *setup* POV — server-present ≠ user-connected), unbounded sequential `get_profile` probes (≤13 fast lookups only on a reachable no-protocols/no-rooms homeserver; the transport-error fix removes the dead-server timeout case — residual note), frontend `.catch` assumes `IpcError` shape (the `client.ts::invoke` wrapper already normalizes any non-envelope rejection into a valid `IpcError`, so `message`/`retriable` are always present), `room_bridge_protocol_id` first-event nondeterminism for multi-bridge rooms (rare; still marks a genuinely-bridged network logged-in; pre-existing Story 3.8 single-shot pattern), triplicated catalog-gating + frontend re-gate "silent drop" and the all-uncatalogued empty-`div` (both unreachable: `merge_catalog` only emits catalog networks and the frontend joins the *same* embedded catalog data, so `catalogFor` never misses), state-not-reset-on-accountId-change flash (unreachable: `AccountBridges` is keyed by `accountId`, so an account change remounts with fresh state rather than mutating props).

### 2026-07-05 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 3
- reject: 13: (high 0, medium 0, low 13)
- addressed_findings:
  - none
- deferred (new ledger entries; real, not trivially fixable — routed to `deferred-work.md`):
  - `[medium]` source (c) bot-DM detection gated on `is_direct() == Ok(true)`, which self-hosted mautrix/Beeper management rooms often don't flag `m.direct` → `NotLoggedIn` can silently fail to trigger on the target homeservers. Spec-prescribed approach (`is_direct()`/`direct_targets()`), untested against a mock `Client`; needs a broader management-room heuristic in a later story.
  - `[medium]` sources (b)+(c-DM) hard-code the account's own `server_name`, so bots on a separate appservice/bridge domain (`@whatsappbot:bridge.example.org` under `@me:example.org`) are invisible — under-reports on standard self-hosted mautrix topologies; documented in code but not as a spec Boundary.
  - `[medium]` the impure discovery shell (`fetch_protocol_ids`/`scan_rooms`/`bot_network_for`/`probe_network_bots`) has no mock-`Client` coverage — only the pure merge/classifier are tested; a documented tradeoff, but it is exactly where the two completeness risks above live.
- rejected (noise / unreachable / design-approved / already-triaged in prior pass): `get_profile Ok → Configured` on servers that 200 every profile (Matrix spec 404s unknown users; `Ok` = present is the correct reading, prior-pass design), non-`NotFound`/`Unrecognized` errcode (rate-limit/forbidden) aborts discovery instead of degrading (the `protocols_error_degrades` classifier deliberately surfaces a *retriable* `Discovery` error for these — sanctioned), hook assumes `IpcError` rejection shape (prior-pass reject: `client.ts::invoke` normalizes every rejection), `bridge-card` enum-drift no fallback (unreachable: `BridgeStatus.ts` is ts-rs-generated from the Rust enum, so a 4th value can't reach the union without a compile break), empty-string error alert (backend guarantees non-empty `Discovery`/`AccountNotFound` messages), multi-protocol portal first-wins (prior-pass reject), `homeserver` copy not length-bounded (account's own trusted server name, Matrix-grammar-bounded), `matrix` catalog dead-card / test uses a fabricated VM (test proves the join wiring; `matrix` correctly never discovers — cosmetic), `retry` callback unmemoized (used only as an `onClick`, no dep-array consumer — latent footgun, not a defect), error UI doesn't distinguish `AccountNotFound` vs transport (the `AccountNotFound` arm is near-unreachable from the pane — account came from the store), `Client` clone survives a mid-flight sign-out (low-likelihood, bounded; frontend `cancelled` guard + keyed remount cover the observable path).

## Design Notes

- **This story replaces 6.1's static per-account catalog projection with real discovery** (6.1 Design Note is explicit). The Bridges pane now shows only Networks actually discovered on the homeserver; the 6.1 catalog supplies presentation (glyph/name/tier badge/ack copy) for each discovered Network via a `networkId` join. Browsing/adding a not-yet-present Network is Wizard/bbctl territory (6.7/6.8), out of scope here.
- **Status vs health.** Discovery status (`logged in`/`not logged in`/`configured`) is the setup/login state derived once from evidence. Live connection health (degraded/disconnected, 60 s surfacing) is Story 6.5 and still uses the placeholder health dot — do not conflate. The card shows a discovery status word; 6.5 later layers live health.
- **Ruma APIs (matrix-sdk 0.18 / ruma-client-api 0.24):** `client.send(matrix_sdk::ruma::api::client::thirdparty::get_protocols::v3::Request::new())` → `Response { protocols: BTreeMap<String, Protocol> }`; keys are protocol ids. Use `get_profile::v3::Request::new(user_id)` (NOT the `#[deprecated]` `get_display_name`) for the MXID probe. Bot MXID = `@{localpart}:{server_name}` from `client.user_id()?.server_name()`. Inspect failures with `err.client_api_error_kind()` (see `auth.rs::map_login_error`).
- **Merge is a pure function** over per-Network evidence `{ in_protocols, mxid_resolves, has_bot_dm, has_portal }` → `Option<BridgeStatus>`; keep all Matrix I/O out of it so the precedence (portal > bot-DM > protocol/mxid > absent) is unit-tested without a homeserver. Probe MXIDs only for Networks not already found by protocols/rooms to bound round-trips.
- **Docs link:** no hosted companion stack exists (docs-only per research). Point `COMPANION_STACK_DOCS_URL` at the real repo docs (`https://github.com/tgorka/keeper/tree/main/docs`) via the established `<a target="_blank" rel="noreferrer">` pattern; it is the single constant to repoint when a dedicated companion-stack page lands.

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` pass (no deprecated `get_display_name`).
- `bun run test:rust` -- expected: cargo-nextest green incl. new `discovery` merge tests + VM round-trip; ts-rs exports `BridgeDiscoveryVm.ts` / `DiscoveredBridgeVm.ts` / `BridgeStatus.ts`.
- `bun run check` -- expected: Biome + tsc + vitest pass incl. new bridges-pane (empty/loading/error/discovered), bridge-card status, and `bridges.ts` tests.

## Auto Run Result

Status: done

**Summary:** Shipped zero-config, per-Account bridge discovery. A new `keeper-core` `bridges::discovery` module merges three sources — (a) `GET /_matrix/client/v3/thirdparty/protocols`, (b) a known-bot MXID existence probe (`get_profile` over `known-bots.json` localparts), and (c) a joined-room scan for `m.bridge` portals (reusing `bridge::room_bridge_protocol_id`) and known-bot DMs (`is_direct` + `direct_targets`) — into a per-Network `BridgeStatus` (loggedIn / notLoggedIn / configured) via a **pure** `merge_discovery` with fixed precedence (portal > bot-DM > protocol/mxid > absent). The result is catalog-gated (only 6.1-catalog Networks surface; uncatalogued protocols logged and dropped) and exposed over a one-shot `bridge_discover(accountId)` Tauri command. The Bridges pane replaces 6.1's static catalog projection with real per-Account discovery: a card per discovered Network (joined to the catalog for glyph/name/tier badge/ack copy), a per-account loading state, a retriable error state, and an honest "No bridges found on {homeserver}." + companion-stack docs-link empty state. No login, `BridgeTransport` trait, or live health (Stories 6.3/6.4/6.5) — discovery only observes state.

**Files changed:**
- `src-tauri/crates/keeper-core/src/bridges/discovery.rs` — new discovery engine: pure `merge_discovery`/`merge_catalog` + impure shell (`fetch_protocol_ids`, `scan_rooms`, `probe_network_bots`, `bot_network_for`) + the `protocols_error_degrades` transport-vs-unsupported classifier; unit tests for the merge matrix and the classifier.
- `src-tauri/crates/keeper-core/src/bridge.rs` — added the pure `parse_bridge_protocol_id` + `room_bridge_protocol_id` reader (dedup'd with the existing label reader) for source (c).
- `src-tauri/crates/keeper-core/src/bridges/mod.rs` — register the `discovery` submodule; re-export `discover`.
- `src-tauri/crates/keeper-core/src/account.rs` — `AccountManager::discover_bridges(account_id)` (clones the live `Client`, runs discovery).
- `src-tauri/crates/keeper-core/src/vm.rs` — `BridgeStatus`, `DiscoveredBridgeVm`, `BridgeDiscoveryVm` (ts-rs export) + round-trip test.
- `src-tauri/crates/keeper-core/src/error.rs` — `BridgeError::AccountNotFound` + `BridgeError::Discovery`.
- `src-tauri/crates/keeper/src/{ipc,lib}.rs` — `bridge_discover` command + `to_ipc_error` arms (`AccountNotFound` → internal/non-retriable, `Discovery` → syncUnavailable/retriable) + handler registration.
- `src/lib/ipc/gen/{BridgeStatus,DiscoveredBridgeVm,BridgeDiscoveryVm}.ts` — generated bindings.
- `src/lib/ipc/client.ts` — `bridgeDiscover()` wrapper + re-exports.
- `src/hooks/use-bridge-discovery.ts` — per-account, unmount-guarded one-shot fetch hook with retry.
- `src/lib/bridges.ts` (+ test) — `COMPANION_STACK_DOCS_URL` + `BRIDGE_STATUS_LABEL`.
- `src/components/layout/bridges-pane.tsx` (+ test) — per-account discovery projection; loading/error/empty/discovered states.
- `src/components/bridges/bridge-card.tsx` (+ test) — `status` prop → discovery status word/dot; kept the 6.1 placeholder health dot + volatile ack gate.

**Review findings breakdown:** 2 patches applied (1 medium — transport-failure classification so an unreachable homeserver surfaces a retriable error instead of a false "no bridges found"; 1 low — thread the server name to remove a redundant per-target `client.user_id()` read); 0 deferred; 8 rejected as unreachable / design-approved / already-handled (see Review Triage Log). No intent gaps, no spec-repair loopbacks.

**Verification (all re-run independently after the review patches):**
- `bun run check:rust` — PASS (rustfmt clean + clippy `-D warnings`; no deprecated `get_display_name`).
- `bun run test:rust` — PASS (466 tests, up from 465: the new classifier test).
- `bun run check` — PASS (Biome + tsc + 614 vitest tests + core-tauri-free guard).

**Residual risks:** The impure discovery shell (protocol fetch/classification, room scan, bot probes) is covered only by the pure `merge_discovery`/`merge_catalog`/`protocols_error_degrades` unit tests, not by an end-to-end mock-`Client` test — the highest-risk branch (transport-vs-unsupported classification) is now unit-tested, but the room-scan/probe I/O is not. On a reachable homeserver that implements neither `thirdparty/protocols` nor has bridge rooms yet, source (b) issues up to ~13 sequential `get_profile` probes (fast on a reachable server; the dead-server timeout case is short-circuited by the protocols transport-error fix) — a candidate for bounded concurrency later. Login, the `BridgeTransport` trait, and live health remain out of scope by design (Stories 6.3/6.4/6.5); the card's primary action and live-health dot stay 6.1 placeholders. `known-bots.json` localparts are the discovery join key — a Network missing a localpart entry can only be found via sources (a)/(c).

## Auto Run Result — Follow-up Review Pass (2026-07-05)

Status: done

**Scope:** Independent follow-up review requested by the orchestrator (prior pass had `followup_review_recommended: true`). Blind Hunter + Edge Case Hunter re-run in parallel on the full baseline→HEAD diff. No code changed in this pass.

**Triage:** 0 intent_gap · 0 bad_spec · 0 patch · 3 defer · 13 reject. No patches were warranted and no spec-repair loopback was triggered — the change is coherent as shipped.

**Deferred (3 new `deferred-work.md` entries — real, spec-prescribed or documented-tradeoff limitations, not trivially fixable):**
1. Bot-DM detection is gated on `is_direct() == Ok(true)`; self-hosted mautrix/Beeper management rooms frequently aren't `m.direct`-flagged, so `NotLoggedIn` can under-report on target homeservers. Needs a broader management-room heuristic later.
2. Sources (b)+(c-DM) hard-code the account's own `server_name`, so bots on a separate appservice/bridge domain are invisible under standard self-hosted topologies. Needs multi-domain probing or an explicit spec Boundary.
3. The impure discovery shell has no mock-`Client` coverage (only the pure merge/classifier are unit-tested) — a documented tradeoff, but exactly where risks 1–2 live; a mock-`Client` integration test would verify management-DM→`NotLoggedIn` / portal→`LoggedIn` end-to-end.

**Rejected (13, all low-consequence):** `get_profile Ok`-as-present (correct Matrix-spec reading), rate-limit/forbidden→retriable-error (sanctioned classifier design), hook `IpcError`-shape assumption (normalized by `invoke`), enum-drift/empty-error/homeserver-length/matrix-dead-card/unmemoized-retry/undifferentiated-error-UI/mid-flight-clone (unreachable via ts-rs codegen + backend guarantees, or latent/cosmetic), multi-protocol-portal-first-wins (prior-pass reject). Full rationale in the Review Triage Log follow-up entry.

**Verification:** No code files were modified in this pass (working-tree changes are documentation only: this spec + `deferred-work.md` + `sprint-status.yaml`). The compiled code is byte-identical to the prior pass's committed HEAD, so the prior verification stands: `bun run check:rust` PASS, `bun run test:rust` PASS (466 tests), `bun run check` PASS (Biome + tsc + 614 vitest + core-tauri-free guard). No re-run needed — nothing changed to invalidate it.

**Follow-up recommendation:** `false`. This pass made no review-driven code changes; the three residual concerns are captured as deferred work for a focused later story, not follow-up-review material.
