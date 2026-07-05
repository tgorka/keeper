---
title: 'Bridge Session Health and Re-Login Prompts'
type: 'feature'
created: '2026-07-05'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: [oversized]
baseline_revision: e248b8bfe3dcc42a627575190736f9f81ac02281
final_revision: 513b25a4d673a01972ae7ee22246075478c90d90
---

<intent-contract>

## Intent

**Problem:** A bridge session can silently die (device unlinked, token expired) and keeper today shows no live signal — messages from that network vanish for days with the user none the wiser. The bridge card health dot is a static placeholder; there is no monitoring, no persistent surfacing, and no one-click fix.

**Approach:** Add a per-session health state machine (Healthy / Degraded / Disconnected), keyed by `(account_id, network_id)`, fed by the bridge's management-room notice events (observed via continuous sync) with a bounded bot-ping liveness fallback, and stream health snapshots to the frontend within 60 s of a change. Surface unhealthy state persistently and unmissably across every surface — card, sidebar roll-up, affected chat rows, and a non-dismissible in-conversation banner — each linking one click into the existing `start_bridge_login` re-login stepper for that exact bridge.

## Boundaries & Constraints

**Always:**
- Health is a **pure, per-session state machine** — `BridgeHealth { Healthy, Degraded, Disconnected }` keyed by `(account_id, network_id)`. The impure Matrix shell only feeds observations in; all state transitions and the sink diff are pure and unit-tested (the 6.2/6.3/6.4 discipline).
- **Detect within 60 s.** Management-room notices arrive via the already-running sync (real-time for the common case); a bounded liveness tick (interval ≤ 60 s) is the fallback for silent deaths. A change reaching the homeserver must reach the UI within 60 s (FR-28, NFR-6).
- **Unhealthy is persistent until resolved** — never a dismissible toast. Driven from one store across: card state-word + dot (pulse twice → steady) with a 3 px disconnected-red left edge, sidebar Bridges worst-state roll-up dot, a health dot on affected chat rows, and a **non-dismissible** inline banner in affected conversations (UX-DR8, UX-DR11).
- **Re-login reuses the shipped entry.** Every "Re-link" surface opens the existing login stepper via `start_bridge_login(account_id, network_id)` (Story 6.3) for that exact bridge — no new login flow, no change to `drive_login`/`step_to_vm`/`BridgeLoginVm`.
- **keeper never guesses.** Only bot output matching the **versioned, data-driven health grammar** changes state; unmatched output is ignored (no state change, no emit). The bot's verbatim reason (trimmed, length-capped, no tokens/session material) may ride along as optional `detail`.
- **Grammar is data, not code.** Disconnected/degraded/healthy markers, the ping command, and the tick cadence live in a versioned embedded JSON (`health-signals.json`), loaded/validated/cached exactly like `bot-commands.json` — tunable per network without code changes.
- **Rust owns the state; the frontend mirrors the stream** and never re-derives health. Only non-secret render data crosses IPC. The sink emits **only on a session-state change** (diffed).
- A chat row / conversation is "affected" iff it matches an unhealthy session on **both** `account_id` **and** the room's stable bridge `network_id` (the machine `protocol.id`, not the display label) — reusing the existing `parse_bridge_protocol_id`.

**Block If:**
- No `start_bridge_login(account_id, network_id)` re-login entry is available to reuse (would contradict the Story 6.3 baseline this story depends on) — HALT, do not reimplement a login flow.
- A room's stable bridge `network_id` cannot be derived client-side for portal rooms (no `parse_bridge_protocol_id` / no MSC2346 `protocol.id`), making "affected rows/conversations" impossible to key reliably — HALT rather than fall back to a fragile display-label match.

**Never:**
- Native OS notifications — the native-notification leg is **Story 10.4**; this story is detection + in-app surfacing only.
- Auto-restart / session supervision / re-login without an explicit user click — surface the prompt only.
- Dismissible or ephemeral unhealthy indicators (a toast that can be lost).
- Guessing health from unparseable output, or hardcoding per-bridge grammar in Rust.
- Continuous/aggressive bot-pinging that spams the management room beyond the bounded fallback cadence.
- Changing `drive_login` / `BridgeLoginVm` / the login stepper; routing media/large payloads through IPC.

## I/O & Edge-Case Matrix

Pure `classify_health_signal(text, &grammar) -> Option<BridgeHealth>` and the per-session state machine `HealthState::apply(obs)`:

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Disconnected notice | mgmt-room notice matching a `disconnected` marker (e.g. "you have been logged out") | classify → `Disconnected`; session flips; snapshot emitted with verbatim (capped) `detail` | No error expected |
| Degraded notice | notice matching a `degraded` marker (e.g. "reconnecting…") | classify → `Degraded`; emitted | No error expected |
| Healthy recovery | ping reply / notice matching a `healthy` marker while `Disconnected`/`Degraded` | → `Healthy`; emitted; `detail` cleared | No error expected |
| Ping timeout below threshold | (N−1) consecutive ping timeouts from `Healthy` | stays previous state (debounce not yet met); no emit | Timeout is a signal, not an error |
| Ping timeout at threshold | Nth consecutive timeout, no intervening healthy signal | → `Disconnected`; emitted | No error expected |
| Unmatched output | notice matching **no** marker | classify → `None`; **no** state change, **no** emit | Ignored — never guessed |
| Idempotent recompute | recomputed snapshot equals previous | `diff_sessions` → false; sink **not** called | No error expected |
| Bootstrap | subscribe; discovery reports a `LoggedIn` session | initial snapshot: session `Healthy`, `last_checked = now` | No error expected |
| Bootstrap non-logged-in | discovery reports `NotLoggedIn`/`Configured` | session is **not** monitored / not emitted as unhealthy (only logged-in sessions have health) | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/bridges/health.rs` -- **NEW**. `BridgeHealth` classification + pure `classify_health_signal`, the per-session `HealthState` machine (debounced) + `apply`, the pure snapshot `diff_sessions`, and the impure `HealthMonitor` shell (per-account: mgmt-room event handler feeding notices + bounded liveness tick with optional bot-ping via the 6.4 send/await, emitting `BridgeHealthSnapshot` on change). Pure core fully unit-tested; live shell = documented residual risk.
- `src-tauri/crates/keeper-core/data/health-signals.json` -- **NEW**. Versioned grammar: `default` + per-network overrides (`disconnectedMarkers`, `degradedMarkers`, `healthyMarkers`, optional `pingCommand`, `tickIntervalSecs`, `enablePing`). Embedded via `include_str!`.
- `src-tauri/crates/keeper-core/src/bridges/data.rs` -- add `HealthSignalsDoc`/`BridgeHealthGrammar` + cached `health_signals()` loader + `grammar_for(network_id)` + validator (mirror `bot_commands()`), with a load test.
- `src-tauri/crates/keeper-core/src/bridges/mod.rs` -- `pub mod health;`.
- `src-tauri/crates/keeper-core/src/bridge.rs` -- add impure `room_bridge_protocol_id(room) -> Option<String>` over the existing pure `parse_bridge_protocol_id` (mirror `room_bridge_network`).
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `BridgeHealth` enum, `BridgeSessionHealthVm { account_id, network_id, network_name, health, last_checked_ms, detail: Option<String> }`, and `BridgeHealthSnapshot { sessions }` (all serde camelCase + ts-rs export). Add `network_id: Option<String>` to `RoomVm` **and** `InboxRoomVm` (sourced from `room_bridge_protocol_id`, mirroring the existing `network` label field).
- `src-tauri/crates/keeper-core/src/account.rs` (RoomVm build, ~line 3669 where `room_bridge_network` is called) -- also resolve `network_id` via `room_bridge_protocol_id` into `RoomVm.network_id`.
- `src-tauri/crates/keeper-core/src/inbox.rs` (~line 552 where `network: room.network.clone()` is copied) -- copy `network_id` through into `InboxRoomVm`.
- `src-tauri/crates/keeper-core/src/account.rs` -- `BridgeHealthSink` type; `subscribe_bridge_health(sink) -> SubscriptionId` at the AccountManager level (bootstrap monitored sessions from discovery across active accounts; spawn per-account `HealthMonitor`s; emit initial snapshot; drain monitors on unsubscribe / shutdown / sign-out).
- `src-tauri/crates/keeper/src/ipc.rs` -- `#[tauri::command] bridge_subscribe_health(state, channel: Channel<BridgeHealthSnapshot>) -> SubscriptionId`; reuse existing `BridgeError` mapping.
- `src-tauri/crates/keeper/src/lib.rs` -- register `bridge_subscribe_health`.
- `src/lib/ipc/client.ts` -- `subscribeBridgeHealth(cb): Promise<number>` (+ matching unsubscribe), following `subscribeInbox`/`subscribeNetworks`.
- `src/lib/stores/bridge-health.ts` -- **NEW** zustand store: subscribe at app init, keyed map `${accountId}:${networkId}` → `BridgeSessionHealthVm`; selectors `useBridgeHealth(accountId, networkId)`, `useWorstBridgeHealth()`.
- `src/components/bridges/bridge-card.tsx` (+ test) -- bind the placeholder health dot to the store; state word (Connected / Action needed / Disconnected); pulse-twice-then-steady on transition to unhealthy; 3 px disconnected-red left edge; last-checked time.
- `src/components/layout/sidebar-pane.tsx` (+ test) -- feed real per-session healths through the existing `worstBridgeHealth()` into the Bridges roll-up dot.
- `src/components/layout/conversation-pane.tsx` (+ test) -- non-dismissible inline banner (reuse the `Alert`/`AlertAction` pattern from `verify-banner.tsx`) when the open room's `(accountId, networkId)` session is unhealthy: "{Network} disconnected — messages may not arrive. Re-link" → opens `BridgeLoginSheet` for that `(accountId, networkId)`.
- `src/components/chat/chat-row.tsx` (+ test) -- health dot on rows whose `(accountId, networkId)` is unhealthy (uses the new `InboxRoomVm.networkId`).
- `src/components/layout/app-shell.tsx` (or the existing subscription init site) -- start/stop the bridge-health subscription in the inbox/networks subscription lifecycle.

## Tasks & Acceptance

**Execution:**
- [x] `data/health-signals.json` + `data.rs` -- versioned grammar (`default` + per-network) + cached `health_signals()`/`grammar_for` loader + validator, with a load test.
- [x] `bridge.rs` -- `room_bridge_protocol_id` impure wrapper over the existing pure `parse_bridge_protocol_id`.
- [x] `vm.rs` -- `BridgeHealth`, `BridgeSessionHealthVm`, `BridgeHealthSnapshot` (serde camelCase + ts-rs); add `network_id: Option<String>` to `RoomVm` + `InboxRoomVm`; round-trip tests.
- [x] `account.rs` (RoomVm build, ~L3669) + `inbox.rs` (~L552) -- populate/copy `network_id` from `room_bridge_protocol_id` alongside the existing `network` label.
- [x] `bridges/health.rs` + `mod.rs` -- `pub mod health`; pure `classify_health_signal`, debounced `HealthState::apply`, pure `diff_sessions`, and the impure `HealthMonitor` (mgmt-room handler + bounded liveness tick/bot-ping via 6.4 send-await + sink emit). **Unit-test the full I/O matrix** (classifier markers, debounce threshold, unmatched→no-emit, idempotent recompute→no-emit, bootstrap logged-in→Healthy, non-logged-in→unmonitored) and a scripted-observation test proving a sequence of observations yields the expected snapshot emissions.
- [x] `account.rs` -- `BridgeHealthSink` + `subscribe_bridge_health` (discovery bootstrap across active accounts, spawn/drain monitors, initial snapshot, cleanup on unsubscribe/shutdown/sign-out).
- [x] `ipc.rs` + `lib.rs` -- `bridge_subscribe_health` `Channel<BridgeHealthSnapshot>` command + registration.
- [x] `client.ts` -- `subscribeBridgeHealth` wrapper (+ unsubscribe).
- [x] `bridge-health.ts` -- zustand store + selectors, subscribing at app init.
- [x] `bridge-card.tsx` (+ test) -- live health dot + state word + pulse-then-steady + red left edge + last-checked.
- [x] `sidebar-pane.tsx` (+ test) -- real worst-state roll-up dot.
- [x] `conversation-pane.tsx` (+ test) -- non-dismissible re-link banner for the open room's unhealthy session → opens the login sheet for that exact bridge.
- [x] `chat-row.tsx` (+ test) -- health dot on affected rows.
- [x] `app-shell.tsx` -- wire the health subscription into the existing subscription lifecycle.

**Acceptance Criteria:**
- Given a logged-in bridge session, when its state changes (device unlinked → the bridge posts a mgmt-room notice, or a silent death caught by the liveness tick), then the per-session state machine reflects Healthy/Degraded/Disconnected and the change reaches the UI within 60 s of it reaching the homeserver (FR-28, NFR-6).
- Given an unhealthy session, when surfaced, then it is persistent until resolved and visible on **every** affected surface simultaneously — card state-word + dot (pulse twice → steady) with a red left edge, the sidebar Bridges roll-up dot rolls up the worst state, affected chat rows show a health dot, and affected conversations show a **non-dismissible** inline banner — all from the one store (FR-28, UX-DR8, UX-DR11).
- Given the card action, the banner, or (later) the prompt, when clicked "Re-link", then the user lands directly in the existing login stepper for that exact `(account_id, network_id)` via `start_bridge_login`, with no change to `drive_login`/`BridgeLoginVm` (FR-28, AD-16).
- Given bot output that matches no grammar marker, then health does not change and no snapshot is emitted (keeper never guesses); the grammar (markers, ping command, cadence) is tunable via `health-signals.json` with no code change.
- Given `bun run check:all`, then Biome + tsc + vitest + rustfmt + clippy (`-D warnings`, no `.unwrap()`) + cargo-nextest all pass.

## Spec Change Log

_No bad_spec loopbacks: the review produced only localized patches, applied directly to the diff._

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 8: (high 2, medium 2, low 4)
- defer: 0
- reject: 7: (high 0, medium 0, low 7)
- addressed_findings:
  - `[high]` `[patch]` `health.rs`/`mod.rs` — the health monitor resolved each session's bot DM via `resolve_bot_room`, which **creates** a DM (`create_dm`) when none exists. Since the subscription starts automatically at app launch, a passive "health observer" was silently creating bot management rooms as a launch side effect. Added a find-only `find_bot_room` (resolve MXID + `find_bot_dm`, never create) and switched `HealthMonitor::spawn` to it — a session with no existing bot DM is left unobserved (stays Healthy) rather than provoking room creation.
  - `[high]` `[patch]` `account.rs` — `subscribe_bridge_health` drained the prior subscription then did slow Matrix I/O (discovery + monitor spawn) **outside any lock**, storing the handle only at the end. Two overlapping subscribes (e.g. a StrictMode double-mount) both spawned monitors and the loser's leaked forever (never drained, handlers still firing). Added a dedicated `bridge_health_subscribe` guard mutex held for the whole subscribe body so drain→build→store is atomic.
  - `[medium]` `[patch]` `health.rs` `ping_once` — treated **any** bot message as `PingReply(Healthy)`, so a bot answering a liveness ping with "you have been logged out" was read as *alive*, masking a real disconnect. The one-shot listener now carries the reply body and classifies it: a `disconnected`/`degraded` reply becomes an explicit unhealthy `Notice` (immediate flip); only a healthy/unmatched reply counts as `PingReply(Healthy)`.
  - `[medium]` `[patch]` `data/health-signals.json` — the bare **default** healthy markers `"connected"`/`"online"` are substrings of negated death phrases ("not connected", "no longer online"), so a real death on any non-whatsapp bridge was misclassified `Healthy`. Hardened the default `disconnectedMarkers` (checked first by precedence) with "not connected", "no longer online", "went offline", "is offline", "unable/could not/couldn't connect", "connection failed", etc.; added a regression test over the real default grammar (and asserting "back online" still reads Healthy).
  - `[low]` `[patch]` `health.rs` `run_liveness_tick` — pinged sessions **serially**, each with a 20 s reply timeout, so several dead sessions would blow the ≤ 60 s detection budget. Now pings all enabled sessions **concurrently** via `join_all`.
  - `[low]` `[patch]` `health.rs` `run_liveness_tick` — with `enable_ping` off (the shipped default) the ticker still woke every interval forever doing nothing. Added an early return when no session enables ping (the real-time mgmt-room handlers still run).
  - `[low]` `[patch]` `health.rs` `HealthState::apply` — a `PingTimeout` only escalated a `Healthy` session; a session stuck `Degraded` ("reconnecting") while the bot went silent never escalated to `Disconnected`. Timeout now promotes `Healthy` **or** `Degraded` at the debounce threshold; added a test.
  - `[low]` `[patch]` `bridge-health.ts` `healthKey` — flattened the Rust `(accountId, networkId)` tuple as `` `${accountId}:${networkId}` ``, which could alias two sessions if an account id ever contained `:`. Both parts are now `encodeURIComponent`-encoded (build + lookup route through the one function, so it stays consistent).
- rejected (cosmetic / by-design / unreachable): `useWorstBridgeHealth` re-derives on every store write (sessions are few; Rust already diffs before emitting) and returns a green roll-up dot when all-healthy (a "bridges present & healthy" dot is a defensible design read, not a defect); `now_ms` doc says "monotonically-increasing" though `SystemTime` isn't (the field is deliberately ignored by `diff_sessions` and only feeds a cosmetic "Checked …" label); empty-string `networkId` collisions in `chat-row`/`conversation-pane` (no monitored session can carry an empty `network_id` — it always comes from a non-empty catalog/`protocol.id`, so `""` never matches); duplicate `network_id` in one account's discovery silently overwriting a BTreeMap entry (discovery already dedupes by `network_id`, and the entries would be identical `LoggedIn` sessions); aggregator `observe` vs `drain` race (the reviewer confirmed it is a safe no-op on an unknown key — no gap).

### 2026-07-05 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 1)
- defer: 5
- reject: 16: (high 0, medium 0, low 16)
- addressed_findings:
  - `[high]` `[patch]` `data/health-signals.json` + `health.rs` — the **whatsapp override** replaces (not merges) the default grammar via `grammar_for`, but its `disconnectedMarkers` omitted the negation guards the prior pass added only to the *default*. Its healthy marker `"connected"` is a substring of negated death phrases, so a real WhatsApp death — "you are not connected", bare "disconnected", "no longer connected" — was misclassified **Healthy** on the flagship bridge (notice leg, ping-independent, live in the shipped config). Hardened the whatsapp `disconnectedMarkers` with the same guards ("not connected", "no longer connected", "disconnected", "went offline", "is offline", "connection lost/failed", "unable/could not/couldn't connect", "not logged in", etc.; disconnected precedence catches them before the healthy substring), and added a `whatsapp_override_does_not_mask_negated_phrases_as_healthy` regression test mirroring the default one. Full bridges suite green (117 tests).

The follow-up review's remaining findings were either already triaged in the first pass (re-flagged: `useWorstBridgeHealth` green roll-up dot, `now_ms` non-monotonic/`0`-on-error cosmetic "Checked" label, duplicate `network_id` overwrite, `observe`/`drain` race, empty-`networkId` collision — all previously rejected with rationale), dormant with the shipped `enablePing: false` (ping double-handler, fabricated timeout `detail`, no ping cap, `MissedTickBehavior`, interval-min — rejected as unreachable), by-design per the intent's 3-state model (no "unknown/unmonitored" state; substring-grammar false-positive surface — data-tunable), or refuted on verification (the re-link `BridgeLoginSheet` teardown on recovery is cancelled cleanly by `useBridgeLogin`'s unmount cleanup calling `cancelBridgeLogin`). Five genuinely-new, non-trivial, story-relevant items were appended to the deferred-work ledger (shipped ping-off silent-death gap, banner↔window-membership coupling, re-subscribe re-bootstrap dropping unhealthy state, doubled per-room `m.bridge` reads, missing `HealthAggregator` boundary test).

## Design Notes

- **Pure core, impure shell — the shipped discipline.** Exactly as 6.2 (discovery merge), 6.3 (provisioning classify), and 6.4 (bot reply classify): `classify_health_signal` + `HealthState::apply` + `diff_sessions` + `health_signals()` are pure and fully unit-tested; the live Matrix shell (mgmt-room event handler, bot-ping send/await-with-timeout, subscription lifecycle) cannot be exercised against a live bot unattended and is a **documented residual risk**. Live bot grammars vary by bridge/version, so the markers/command/cadence ship in `health-signals.json` — tunable without code.
- **Two feed legs, one machine.** Leg 1 (primary, real-time): a Matrix event handler on the bot management room classifies the bot's own notices as they arrive via the running sync — this satisfies the 60 s target for the common "the bridge told us" case. Leg 2 (fallback): a bounded liveness tick (≤ 60 s) issues an optional bot-ping (reusing the 6.4 `BotDriver` send/await) and treats a timeout, after a debounce threshold, as `Disconnected` — covering silent deaths. Both produce a `HealthObservation` fed to the same `HealthState`.
- **Debounce prevents flapping.** A single missed ping is not a disconnect; the state machine requires N consecutive failures (or an explicit disconnected notice) before flipping Healthy→Disconnected, and any healthy signal recovers immediately. Keep the threshold small and the whole transition table pure/tested.
- **Emit only on change.** The monitor holds the last emitted snapshot and calls the sink only when `diff_sessions` reports a real per-session change — no periodic re-emit noise, matching the `NetworksSink` cadence contract.
- **Exact room→session join.** "Affected" is keyed on `(account_id, network_id)` where `network_id` is the room's machine `protocol.id` (via the existing `parse_bridge_protocol_id`), **not** the display label — mautrix `protocol.id`s reconcile directly to the catalog `networkId`, so the join is stable. This is why `InboxRoomVm` gains `network_id`.
- **Re-link reuses shipped primitives.** Surfaces call the same `start_bridge_login(account_id, network_id)` the card already uses; the banner/row/card just open the `BridgeLoginSheet` for that pair — no second login path, staying indistinguishable from a first login (AD-16).
- **Frontend renders CSS tints already in the theme** (`--bridge-healthy/degraded/disconnected`, the `HEALTH_DOT_CLASS`/`worstBridgeHealth` helpers already stubbed in `sidebar-pane.tsx`).

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` (no `async_fn_in_trait`, no `.unwrap()`).
- `bun run test:rust` -- expected: cargo-nextest green incl. the new `classify_health_signal` I/O matrix, the debounced `HealthState` transitions, `diff_sessions` idempotence, the bootstrap logged-in/non-logged-in cases, `health_signals()` load, and the `network_id` VM round-trips.
- `bun run check` -- expected: Biome + tsc + vitest pass incl. `bridge-card` (live dot + state word), `sidebar-pane` (roll-up), `conversation-pane` (non-dismissible banner → opens re-login for the exact bridge), and `chat-row` (affected-row dot).

**Manual checks (if no CLI):**
- Live bridge-session health cannot be exercised unattended: the impure Matrix shell (mgmt-room event handler, bot-ping send/await-with-timeout, subscription lifecycle) and live reply-grammar correctness are documented residual risks, covered only by the pure classifier/state-machine/diff unit tests and the scripted-observation contract test — as with 6.2's discovery, 6.3's provisioning, and 6.4's bot shells.

## Auto Run Result

Status: done

**Summary:** Shipped bridge-session health monitoring + persistent, one-click-fix re-login surfacing. A pure per-session state machine (`BridgeHealth { Healthy, Degraded, Disconnected }`), keyed `(account_id, network_id)`, is fed by two legs: the bridge's own **management-room notice events** (observed in real time via the running sync and classified against a versioned, data-driven grammar) and a bounded **bot-ping liveness fallback** for silent deaths — both feeding a debounced `HealthState`. Health snapshots stream to the frontend over a `Channel<BridgeHealthSnapshot>` (emitted only on a real diff). Unhealthy state is surfaced persistently and unmissably across every surface — the bridge card (live dot + state word + pulse-then-steady + red left edge), the sidebar Bridges worst-state roll-up dot, a health dot on affected chat rows, and a **non-dismissible** in-conversation banner — each opening the shipped `start_bridge_login(account_id, network_id)` re-login stepper for that exact bridge (no change to `drive_login`/`BridgeLoginVm`). The pure core (`classify_health_signal`, debounced `HealthState::apply`, `diff_sessions`, `health_signals()`) is fully unit-tested; the impure Matrix shell is documented residual risk, matching the 6.2/6.3/6.4 discipline. The room→session join keys on the room's stable machine `network_id` (`protocol.id`), added to `RoomVm`/`InboxRoomVm`.

**Files changed:**
- `src-tauri/crates/keeper-core/data/health-signals.json` — new versioned grammar (`default` + `whatsapp` override): disconnected/degraded/healthy markers, ping command, cadence.
- `src-tauri/crates/keeper-core/src/bridges/health.rs` — new: pure `classify_health_signal` (severity precedence, case-insensitive), debounced `HealthState` machine, pure `diff_sessions`/snapshot projection, `HealthAggregator` + `HealthMonitor` (find-only bot-DM resolution, mgmt-room notice handler, concurrent bounded liveness tick). Full I/O-matrix + scripted-observation tests.
- `src-tauri/crates/keeper-core/src/bridges/data.rs` — `HealthSignalsDoc`/`BridgeHealthGrammar` + cached `health_signals()`/`grammar_for` + validator (cadence ≤ 60 s) + tests.
- `src-tauri/crates/keeper-core/src/bridges/mod.rs` — `pub mod health`; find-only `find_bot_room` (never creates a DM).
- `src-tauri/crates/keeper-core/src/vm.rs` — `BridgeHealth`/`BridgeSessionHealthVm`/`BridgeHealthSnapshot` (ts-rs) + `network_id` on `RoomVm`/`InboxRoomVm`.
- `src-tauri/crates/keeper-core/src/{account.rs,inbox.rs}` — populate/copy `network_id`; `subscribe_bridge_health`/`unsubscribe_bridge_health` (serialized), monitor bootstrap/drain wired into shutdown.
- `src-tauri/crates/keeper/src/{ipc.rs,lib.rs}` — `bridge_subscribe_health`/`bridge_unsubscribe_health` `Channel` commands + registration.
- `src/lib/ipc/client.ts` + `src/lib/stores/bridge-health.ts` + `src/hooks/use-bridge-health.ts` — IPC wrappers, mirror store (collision-safe key), app-shell subscription hook.
- `src/components/bridges/bridge-card.tsx`, `src/components/layout/{sidebar-pane,conversation-pane,app-shell}.tsx`, `src/components/chat/chat-row.tsx` (+ tests) — the four surfacing surfaces + non-dismissible banner.

**Review findings breakdown:** 8 patches applied (2 high — passive-observer DM creation, and unserialized-subscribe monitor leak; 2 medium — ping reply not classified, and default-grammar negation masking; 4 low — serial-ping 60 s-budget, idle ticker, Degraded→Disconnected escalation, frontend key collision-safety); 0 deferred; 0 intent gaps; 0 bad_spec loopbacks; 7 rejected as cosmetic / by-design / unreachable (see Review Triage Log).

**Verification (all re-run after the patches):**
- `bun run check:rust` — PASS (rustfmt clean + clippy `-D warnings`; no `.unwrap()`).
- `bun run test:rust` — PASS (560 tests, up from 558 baseline / +2 for the review fixes).
- `bun run check` — PASS (Biome 203 files + tsc + 645 vitest + core-tauri-free guard).

**Follow-up review recommended:** `true`. The final pass made two high-severity fixes to the subscription lifecycle and the passive-observer side effects, plus concurrency changes to the liveness tick and correctness changes to the ping/await path and the classifier grammar — behavioral changes across the await/cancel/subscribe lifecycle that warrant an independent look, mirroring 6.3/6.4's follow-up recommendations after their lifecycle changes.

**Residual risks:** Live bridge-session behavior cannot be exercised unattended — the impure Matrix shell (mgmt-room event handler, bot-ping send/await-with-timeout, subscription lifecycle) and live reply-grammar correctness are covered only by the pure `classify_health_signal`/`HealthState`/`diff_sessions`/`health_signals()` unit tests and the scripted-observation contract test. Real bots' notice grammars and ping/liveness behavior vary by bridge/version; the markers/command/cadence ship data-tunable in `health-signals.json` so tuning needs no code change. Bot-ping liveness is **off by default** (`enablePing: false`) — the shipped fallback relies on the real-time notice handler; enabling ping per network is a data-only change. The native-notification leg of health surfacing is out of scope here and rides Story 10.4. Image-only / non-text bot notices are not classified (text/notice/emote bodies only).

### Follow-up review pass — 2026-07-05

Independent follow-up review (Blind Hunter `bmad-review-adversarial-general` + Edge Case Hunter `bmad-review-edge-case-hunter`, run in parallel without prior context) of the committed 6.5 diff.

**1 patch applied (high):** The `whatsapp` grammar override was missing the negation-guard disconnected markers the first pass added only to the `default` grammar. Because `grammar_for` returns the matching override *instead of* the default (not a merge), a real WhatsApp death — "you are not connected", bare "disconnected", "no longer connected" — matched the healthy `"connected"` substring and classified **Healthy**: the exact substring-masking bug the default's own regression test guards, left open on the flagship bridge and *live in the shipped config* (it rides the real-time notice leg, independent of the disabled ping). Hardened `health-signals.json` whatsapp `disconnectedMarkers` (disconnected precedence now catches the negated phrases) and added a `whatsapp_override_does_not_mask_negated_phrases_as_healthy` regression test mirroring the default one.

**0 intent gaps, 0 bad_spec loopbacks. 5 deferred (new ledger entries), 16 rejected.** Deferred: shipped `enablePing: false` leaves AC1's silent-death detection inert (a product spam-tradeoff decision is owed); banner visibility coupled to window membership (future true-windowing); account-set change re-bootstraps monitors and transiently drops unhealthy state; doubled per-room `m.bridge` state reads on the window-diff hot path; `HealthAggregator` diff-gate/sink boundary lacks a direct unit test. Rejected: already-triaged re-flags (worst-health green dot, `now_ms` cosmetic label, duplicate `network_id`, observe/drain race), dormant ping-path items (double-handler, fabricated `detail`, no cap, tick behavior), by-design per the 3-state model / data-tunable substring grammar, and one refuted on verification — the re-link `BridgeLoginSheet` teardown on recovery is cancelled cleanly by `useBridgeLogin`'s unmount cleanup calling `cancelBridgeLogin`.

**Files changed this pass:**
- `src-tauri/crates/keeper-core/data/health-signals.json` — hardened the whatsapp override `disconnectedMarkers` with the negation guards.
- `src-tauri/crates/keeper-core/src/bridges/health.rs` — added the whatsapp negation-masking regression test.

**Verification:** `bun run check:rust` — PASS (rustfmt clean + clippy `-D warnings`). `cargo test -p keeper-core bridges::` — PASS (117 tests, incl. the new regression). Frontend untouched this pass.

**Follow-up review recommended:** `false` — a single, well-contained, additive grammar hardening (adds disconnected markers only, caught first by precedence) plus a mirrored regression test; no logic/API/lifecycle change, verified green. An independent re-review would add little.

**Residual risks:** unchanged from the first pass, plus the 5 deferred items above. `enablePing: false` remains the shipped default, so silent-death detection relies on the real-time notice leg alone.
