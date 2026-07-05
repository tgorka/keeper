---
title: 'Bridge Bot Fallback Driver'
type: 'feature'
created: '2026-07-05'
status: 'done'
baseline_revision: '676655ef855b6c6a09d36559d9e1de3a6d415b17'
final_revision: 'cab9fbabe8f670427d53017d4bd24e1e3ebd8a1d'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** Native bridge login (6.3) only works when the bridge exposes the mautrix **bridgev2 HTTP provisioning API**. On legacy deployments that lack it, keeper has no native path, so the user is dumped into the raw Bridge Bot chat to type `!wa login` by hand — the exact setup-cliff this epic removes (FR-27). And even where provisioning works, there is still no in-app way to reach the raw Bridge Bot chat as a manual escape hatch (UX-DR19).

**Approach:** Add `BotDriver`, the second `BridgeTransport` impl (the seam 6.3 built for exactly this), which drives a bridge login by sending **Bridge Bot chat commands** to the bot's DM room and parsing the bot's reply events into the *same* `LoginStepResponse` wire shapes the generic `drive_login` already consumes — so the emitted stepper states are indistinguishable from the provisioning path, with zero driver changes. `account.rs` prefers `Provisioning` and falls back to `BotDriver` only when no provisioning API is present. Separately, wire the **"Open Bridge Bot chat"** escape hatch: a `bridge_bot_room` command that resolves-or-creates the bot DM and returns its room id, surfaced in the Bridge card's Manage menu and in the login Sheet's failure state, navigating straight to that room.

## Boundaries & Constraints

**Always:**
- `BotDriver` implements the existing `BridgeTransport` trait unchanged (native async fns, static dispatch — no `async-trait`, no trait object); the generic `drive_login`, `step_to_vm`, and `BridgeLoginVm` are **not modified**. A login must look identical whichever transport powered it.
- Every Bridge Bot command send and every wait-for-reply is **bounded by a timeout** (a bot that never answers surfaces an honest `BridgeError::Bot` failure with Retry, never an infinite spinner). A `display_and_wait` poll uses a generous long-wait (the user is scanning/acting) and on timeout re-emits the current display rather than failing; a `user_input` submit uses a shorter reply timeout.
- The reply→step **classification is a pure, unit-tested function** over a normalized bot reply, plus a **data-driven** per-network command table (`data/bot-commands.json`, `include_str!` like `known-bots.json`) carrying the login/cancel command strings and a `default` entry so every bridgev2 bot works without a per-network row. Bridge-specific knowledge lives in versioned data, never hardcoded logic.
- Unparseable / error bot output surfaces the bot's **raw reply verbatim** as `BridgeError::Bot(String)` (retriable, bounded in length like the provisioning error cap) — keeper never guesses at output it can't classify.
- The bot MXID is resolved from `known-bots.json` localparts on the account's own `server_name` (the discovery precedent); the bot DM room is found among joined direct rooms or created via `client.create_dm`. Command messages are sent directly via the SDK room API (control messages, like verification events) — not the composer send-gate.
- `BotDriver` and `Provisioning` are both `Clone`; `LoginSession` holds a `LoginTransport` enum over the two so an explicit cancel / graceful-shutdown drain can best-effort `login_cancel` (send the bot's cancel command) on whichever transport powered the session.
- The "Open Bridge Bot chat" escape hatch is reachable from the Bridge card **Manage** menu, and the login Sheet **failure** state offers it as a button; both resolve the bot room via `bridge_bot_room` then `setView("inbox")` + `selectRoom({accountId, roomId})`.

**Block If:**
- Making the two paths indistinguishable would require driving a bot login method keeper cannot render natively at all (e.g. the bot ONLY offers a browser-cookie or passkey ceremony) — that is the already-shipped unsupported-method state, not a new decision. (No genuine unattended blocker is anticipated; this story composes existing seams.)

**Never:**
- Do **not** add `list_logins` / `logout` / `set_relay` trait methods or their bot commands' code paths: 6.3 (done, frozen) deferred those trait methods to Stories 6.5/6.6 ("defining unused ones now is dead code"), and the project forbids dead code (`clippy -D warnings`). `BotDriver` implements only the login operations `drive_login` exercises; the others gain `BotDriver` impls when 6.5/6.6 introduce the trait methods + their IPC. The `data/bot-commands.json` schema MAY carry their command strings as data (no code), but no trait method or IPC for them here.
- No live health state machine, 60 s polling, or re-login prompts (Story 6.5); no new-chat / identifier resolution (6.6); no Wizard integration (6.8).
- No decoding of a bot's QR **image** to recover a payload: a bot reply that presents QR only as an `m.image` (no extractable scannable payload) maps to the existing honest unsupported-in-bot-mode failure state (which already names the Bridge Bot chat) — never a blank QR panel, never a fake success. Native image-QR rendering is out of scope.
- No changes to the provisioning transport's wire behavior; no new `BridgeLoginVm` fields, no new `BridgeLoginPhase` variants.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| No provisioning API | `Provisioning::connect` resolves no base URL (`Ok(None)`) | `start_bridge_login` builds `BotDriver` (resolve/create bot DM) and drives the same `drive_login`; user sees identical stepper | If bot MXID unresolvable → `BridgeError::Bot` (retriable) |
| Provisioning present | probe authenticates (`Ok(Some)`) | drive with `Provisioning` (unchanged 6.3 path); no bot fallback | n/a |
| Provisioning transport error | probe candidate errors (`Err`) | surface the error; **no** silent bot fallback | `BridgeError::Provisioning` (retriable) |
| Text/code login | bot reply is a prompt ("Enter the code…") | classifier → `UserInput{fields:[inferred type]}` → `CodeEntry`; user submits → sent as a bot message → next reply | field type inferred (code/phone/password/text) |
| Payload QR login | bot reply carries a scannable QR **payload** string | classifier → `DisplayAndWait{Qr{data}}` → native QR panel (existing SVG render), indistinguishable | n/a |
| Image-only QR | bot reply presents QR only as `m.image`, no payload | classifier → unsupported-in-bot-mode `Failure` naming the Bridge Bot chat + escape hatch | not an error — a rendered terminal state |
| Login success | bot reply matches a success pattern | classifier → `Complete` → `Success` "Linked ✓" auto-advance | n/a |
| Unparseable / bot error | reply matches no rule, or is a bot error line | `Failure` shows the bot's message **verbatim** + Retry | `BridgeError::Bot(msg)`, length-capped, retriable |
| Bot silent | no reply within the reply timeout (user_input submit / start) | `Failure`: "The bridge bot didn't respond." + Retry | `BridgeError::Bot`; retriable |
| Cancel / close | user closes the Sheet mid-flow | `cancel_bridge_login` best-effort sends the bot cancel command, then aborts the task | n/a |
| Open Bridge Bot chat | Manage menu / failure-state button | `bridge_bot_room(account,network)` resolves/creates the bot DM → `setView("inbox")` + `selectRoom` | unresolvable → `BridgeError::Bot`; surfaced inline |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/data/bot-commands.json` -- NEW; versioned data-driven bot login protocol: a `default` entry (`loginCommand`, `cancelCommand`) + optional per-network overrides. Embedded via `include_str!`.
- `src-tauri/crates/keeper-core/src/bridges/data.rs` -- add `BotCommandsDoc` + cached `bot_commands()` loader (mirror `known_bots()` / `provisioning()`).
- `src-tauri/crates/keeper-core/src/bridges/transport/bot.rs` -- NEW; `BotDriver` (`Clone`) impl of `BridgeTransport`: holds the live `Client` + bot DM `Room` + `network_id` + resolved `BotProtocol` + internal synthetic step/cursor state. Sends commands via the SDK room API, awaits the next bot reply (timeout), classifies it. Plus the **pure** `classify_bot_reply(reply: &BotReply, proto: &BotProtocol) -> LoginStep` and `BotReply` normalization — unit-tested I/O matrix.
- `src-tauri/crates/keeper-core/src/bridges/transport/mod.rs` -- register `pub mod bot;`. Trait itself unchanged.
- `src-tauri/crates/keeper-core/src/bridges/mod.rs` -- add `resolve_bot_room(client, network_id) -> Result<Room, BridgeError>` (find bot DM among joined direct rooms via known-bots localparts on the account server, else `client.create_dm`), shared by `BotDriver` construction and the `bridge_bot_room` command.
- `src-tauri/crates/keeper-core/src/error.rs` -- add `BridgeError::Bot(String)` (retriable); keep existing arms.
- `src-tauri/crates/keeper-core/src/bridges/transport/provisioning.rs` -- change `connect` to return `Result<Option<Provisioning>, BridgeError>`: `Ok(None)` = no provisioning base URL resolved (→ bot fallback), `Ok(Some)` = connected, `Err` = real transport error. Probe-outcome classification stays pure/tested.
- `src-tauri/crates/keeper-core/src/account.rs` -- `LoginSession.transport` becomes `LoginTransport { Provisioning(Provisioning), Bot(BotDriver) }` (with a `login_cancel` match). `start_bridge_login` selects the transport (provisioning-first, bot fallback on `Ok(None)`) and spawns the matching `drive_login` monomorphization; `cancel_bridge_login` / shutdown drain call the enum's `login_cancel`. Add `bridge_bot_room(account_id, network_id) -> Result<String, CoreError>`.
- `src-tauri/crates/keeper/src/ipc.rs` -- add `#[tauri::command] bridge_bot_room(state, account_id, network_id) -> String`; map `BridgeError::Bot` in `to_ipc_error` (syncUnavailable / retriable).
- `src-tauri/crates/keeper/src/lib.rs` -- register `bridge_bot_room`.
- `src/lib/ipc/client.ts` -- add `bridgeBotRoom(accountId, networkId): Promise<string>`.
- `src/components/bridges/bridge-card.tsx` (+ test) -- add a **Manage** dropdown (`dropdown-menu`) with "Open Bridge Bot chat" → `bridgeBotRoom` then `setView("inbox")` + `selectRoom`.
- `src/components/bridges/bridge-login-sheet.tsx` (+ test) -- in the `failure` phase add an "Open Bridge Bot chat" button doing the same navigation + closing the Sheet.

## Tasks & Acceptance

**Execution:**
- [x] `error.rs` -- add `BridgeError::Bot(String)` (retriable); leave existing arms unchanged.
- [x] `data/bot-commands.json` + `data.rs` -- versioned bot-command doc (`default` + optional per-network) + cached `bot_commands()` loader with a load test.
- [x] `provisioning.rs` -- `connect` → `Result<Option<Provisioning>, BridgeError>` (`Ok(None)` = no base URL resolved); adjust the probe to classify "no candidate authenticated" as `Ok(None)` and a candidate transport error as `Err`; keep `resolve_candidates` pure + update the 6.3 caller.
- [x] `bridges/transport/bot.rs` -- `BotDriver` (`Clone`) `BridgeTransport` impl + pure `classify_bot_reply` + `BotReply` normalization; unit-test the full I/O matrix (prompt→user_input with inferred field type, payload-qr→qr, image-only→unsupported, success→complete, error/unparseable→verbatim `Bot` failure, silent→timeout `Bot` failure) and a scripted-fake driver test proving BotDriver drives `drive_login` to the same VM phases as a Provisioning script.
- [x] `bridges/mod.rs` + `transport/mod.rs` -- register `bot` module; add shared `resolve_bot_room`.
- [x] `account.rs` -- `LoginTransport` enum + `LoginSession` field swap; provisioning-first / bot-fallback selection in `start_bridge_login`; enum `login_cancel` on cancel + shutdown drain; `bridge_bot_room`. Unit-test the transport-selection decision (pure helper over the connect outcome).
- [x] `ipc.rs` + `lib.rs` -- `bridge_bot_room` command + `BridgeError::Bot` → `syncUnavailable` mapping + registration.
- [x] `client.ts` -- `bridgeBotRoom` wrapper.
- [x] `bridge-card.tsx` (+ test) -- Manage menu with "Open Bridge Bot chat" → resolves the room + navigates.
- [x] `bridge-login-sheet.tsx` (+ test) -- failure-state "Open Bridge Bot chat" escape-hatch button → resolves + navigates + closes.

**Acceptance Criteria:**
- Given a bridge with no provisioning API, when the user clicks Connect, then `start_bridge_login` selects `BotDriver` (the second `BridgeTransport` impl) which drives the login by sending Bridge Bot commands and parsing replies with timeouts, and the user sees the *same* stepper states as the provisioning path — indistinguishable, with no changes to `drive_login`/`step_to_vm`/`BridgeLoginVm` (FR-27, AD-16).
- Given any bridge, when the user opens the card Manage menu or hits a login failure, then "Open Bridge Bot chat" is offered and navigates directly to the bot's DM room (`bridge_bot_room` → `setView("inbox")` + `selectRoom`), keeping the raw bot reachable as the manual escape hatch (FR-27, UX-DR19).
- Given unparseable bot output, then the stepper fails with the bot's raw reply shown verbatim (length-capped) via `BridgeError::Bot`, never a guess (FR-27).
- Given a bridge that *does* expose the provisioning API, then `Provisioning` is still used (no bot fallback) and a genuine provisioning transport error surfaces without silently switching to the bot.
- Given `bun run check:all`, then Biome + tsc + vitest + rustfmt + clippy (`-D warnings`, no `async_fn_in_trait`, no `.unwrap()`) + cargo-nextest all pass.

## Spec Change Log

_No bad_spec loopbacks: the review produced only localized patches, applied directly to the diff._

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 7: (high 0, medium 5, low 2)
- defer: 0
- reject: 16: (high 0, medium 0, low 16)
- addressed_findings:
  - `[medium]` `[patch]` `bot.rs` reply-race: the reply event handler was registered *after* `send_command` returned, so a bot reply landing during the send round-trip was missed → a spurious 30 s "didn't respond" failure. Refactored to **arm the listener before the send** (`arm_reply_listener` + `send_and_await`/`send_and_step`).
  - `[medium]` `[patch]` `bot.rs` handler leak on the cancel path: `remove_event_handler` only ran after `rx.recv()` completed, so aborting the driver task mid-await (the common Sheet-close → `cancel_bridge_login` → `task.abort()` path this feature supports) skipped it and leaked a handler for the client's lifetime. Added an RAII `HandlerGuard` that removes the handler on drop (covers timeout *and* abort).
  - `[medium]` `[patch]` `bot.rs` `extract_qr_payload` treated any ≥20-char space-free token as a scannable QR payload, so a bare web URL or a Matrix identifier (`@…:…`/`!…:…`/`#…:…`) rendered as a bogus, unscannable QR panel (and pre-empted the prompt rule). Now excludes `http(s)://` web URLs and Matrix ids while keeping custom-scheme payloads (e.g. Signal's `sgnl://…`); regression test added.
  - `[medium]` `[patch]` `bot.rs` `normalize_message` discarded an `m.image`'s caption body, so an image whose caption carries the scannable payload could never render a native QR (contradicting the classifier's image-with-payload contract, which was only reachable from a synthetic `BotReply`). Now keeps the trimmed caption; test drives a real `ImageMessageEventContent` through to a native QR.
  - `[medium]` `[patch]` `bot.rs` `is_success_line` matched the loose bare `"logged in as"`, risking a **false Success** on instructional copy ("once you're logged in as…") before the login actually completed. Dropped that marker (specific terminal phrasings remain); negative test added.
  - `[low]` `[patch]` `bot.rs` `login_step`'s `_ =>` wildcard swallowed an unexpected `step_type` into a 120 s display wait. Now matches `"display_and_wait"` explicitly and surfaces a truly-unknown type as an honest `BridgeError::Bot`.
  - `[low]` `[patch]` `bridge-card.tsx` / `bridge-login-sheet.tsx`: the "Open Bridge Bot chat" escape hatch swallowed a resolve failure with only `console.error`, leaving a dead button. Now surfaces a `toast.error` (the app's established `sonner` pattern) so an unresolvable bot is honest.
- rejected (inherent / by-design / documented residual risk / unreachable): credentials (2FA/password) sent as bot-chat messages — **inherent** to bot-driven login (a bot's only input channel is a message in the user's own DM; masking is only shoulder-surfing protection); broad substring error-markers and multi-event / split replies — the spec's **documented data-tunable grammar residual risk** (live bot grammars vary; the classifier + `bot-commands.json` ship tunable without code); `login` vs per-network command tuning (same residual risk — `bot-commands.json` is the tuning point); bot MXID composed on the account's own `server_name` (matches the existing discovery precedent; genuinely-different-server bridges are a broader concern than this story); navigate-to-not-yet-synced room shows a transient blank pane (converges on sync; pre-existing `selectRoom`/conversation-pane behavior); `bridge_bot_room` could create a second DM if directness state hasn't synced (low-probability extra empty room); `BTreeMap` value-order on multi-field submit (single-field classifier only — unreachable); uncapped QR `data` (mooted by `qr_svg`'s unencodable → honest-failure path — never reaches the DOM as text); `send_command` SDK-error interpolation (no access token in a room-send error); `display_and_wait` has no overall poll cap (parity with the accepted provisioning long-poll — cancel/Sheet-close stops it); mutex-poison silently ignored (only reachable after a panic; best-effort clone); React state-update after unmount on a late resolve (harmless warning); `create_dm` room the bot hasn't joined yet (mautrix bots auto-accept DM invites).

### 2026-07-05 — Follow-up review pass
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 0
- reject: 15: (high 0, medium 0, low 15)
- addressed_findings:
  - `[low]` `[patch]` `account.rs` cancel-orphan on a slow bot first reply: `drive_login` records the `login_id` only *after* `login_start` succeeds, but `BotDriver::login_start` sends the login command to the bot chat **before** awaiting the reply — so a reply timeout left the slot `None` and both cancel sites (`cancel_bridge_login` + the shutdown drain) skipped `login_cancel` entirely, orphaning a login that was already initiated on the bot. Replaced `LoginTransport::login_cancel(&str)` with `cancel_recorded(Option<String>)`: the provisioning arm stays gated on a real id (none = no `/login/start` succeeded = nothing to cancel), the bot arm fires its id-agnostic, idempotent cancel command even with no recorded id. No change to `drive_login`/`step_to_vm`/`BridgeLoginVm`.
- rejected (documented residual risk / by-design / covered by the prior pass): poll-to-poll reply-loss window and first-reply-only/multi-event consumption — the impure await-next-reply shell + multi-event grammar the spec already documents as residual risk (the listener is armed continuously through the actual scan wait; the inter-step handoff is a sub-millisecond seam); broad substring `is_error_line`/`is_success_line`/`extract_qr_payload`/`infer_field_type` markers — the documented data-tunable grammar residual risk (the prior pass already patched the concrete URL/Matrix-id/`logged in as` cases); provisioning `connect` degrading a non-5xx (4xx/transport-error) candidate to the bot fallback — by-design (degrading to the working fallback is more user-helpful than dead-ending, and the account's own Matrix session token cannot expire independently of the session); `bridge_bot_room` creating the DM from a read-looking Manage click — by-design (`resolve-or-create` per the intent contract); `create_dm` error interpolating the composed bot MXID — MXID is not secret material (the `Bot` contract forbids only tokens/session material); classifier not data-driven per-network (`_proto` unused) — the intent contract scopes data to the login/cancel command strings and keeps the classifier as pure logic; English-only markers (no i18n) — scope note, not a regression; mutex-poison re-emit — already rejected in the prior pass (reachable only after a panic); scripted-driver tests bypassing the concurrency machinery — an assurance note (feeds the follow-up recommendation, not a code defect); empty `user_input` submission sending an empty message — low, and identical to the provisioning path (drive_login governs submission).

## Design Notes

- **Drop-in transport, no driver change.** 6.3 built `drive_login<T: BridgeTransport>` for exactly this: `account.rs` picks the concrete transport, so `BotDriver` monomorphizes a second instantiation with zero changes to the driver, `step_to_vm`, or the VM. Verify by asserting a scripted `BotDriver` (over a fake reply source) yields the same `BridgeLoginPhase` sequence as the equivalent `Provisioning` script.
- **BotDriver models the provisioning state machine over chat.** `login_flows` returns a single synthetic flow (bots present one login path → `drive_login` auto-starts, no `ChoosingMethod`). `login_start` sends the data-driven login command and awaits+classifies the first reply, synthesizing an opaque `login_id`/`step_id` (internal counters). `login_step` for `user_input` sends the field value(s) as a bot message then awaits the next reply; for `display_and_wait` it awaits the next bot reply (long timeout; re-emit current display on timeout). `login_cancel` best-effort sends the cancel command. Internal cursor/step state lives behind an `Arc<Mutex<…>>` so the transport stays `Clone` + `Send`.
- **Pure classifier + data table are the tested core; the Matrix shell is residual risk.** Exactly the 6.2/6.3 discipline: `classify_bot_reply` + `bot_commands()` are pure and fully unit-tested; the impure shell (room resolve/create, `room.send`, await-next-reply-with-timeout) cannot be exercised against a live bot unattended and is a documented residual risk. Real bots' reply grammars vary by bridge/version — the command table + classifier ship data-driven so tuning needs no code change.
- **Image-only QR is honestly out of scope.** A bot that shows its QR only as an image can't be re-rendered natively without decoding the image; that maps to the existing unsupported-method `Failure` (which already names the Bridge Bot chat), made actionable here by the real escape hatch. In practice modern QR bridges expose the provisioning API (→ native via 6.3); `BotDriver` is the legacy fallback, more often text/code/phone flows.
- **Escape-hatch navigation reuses shipped stores.** `bridge_bot_room` returns the room id; the frontend calls `primaryViewStore.setView("inbox")` then `roomsStore.selectRoom({accountId, roomId})` — the same primitives other surfaces use to open a room.

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` (no `async_fn_in_trait`, no `.unwrap()`).
- `bun run test:rust` -- expected: cargo-nextest green incl. new `bot` classifier + scripted-driver tests, the `connect` `Ok(None)` classification, `bot_commands()` load, and the transport-selection decision test.
- `bun run check` -- expected: Biome + tsc + vitest pass incl. `bridge-card` (Manage → Open Bridge Bot chat navigates) and `bridge-login-sheet` (failure escape-hatch navigates).

**Manual checks (if no CLI):**
- Live bot-chat login cannot be exercised unattended; the impure Matrix shell (bot DM resolve/create, command send, await-reply timeouts) and live reply-grammar correctness are documented residual risks (as with 6.2's discovery and 6.3's provisioning shells).

## Auto Run Result

Status: done

**Summary:** Shipped the Bridge Bot fallback driver. `BotDriver` is the second `BridgeTransport` impl (the seam Story 6.3 built) — it drives a bridge login over the raw **Bridge Bot chat**: sends a data-driven login command to the bot's DM room and parses the bot's reply events into the *same* `LoginStepResponse` wire shapes the generic `drive_login` already consumes, so the stepper states are indistinguishable from the provisioning path with **zero changes** to `drive_login`/`step_to_vm`/`BridgeLoginVm`. `account.rs` prefers `Provisioning` and falls back to `BotDriver` only when no provisioning API is present (`Provisioning::connect` → `Ok(None)`); a genuine provisioning transport error surfaces without a silent fallback. The reply→step classifier (`classify_bot_reply`) and the command table (`bot-commands.json` + `bot_commands()`) are pure and fully unit-tested; the impure Matrix shell (bot DM resolve/create, `room.send`, await-next-reply-with-timeout via an armed event handler) is a documented residual risk. The **"Open Bridge Bot chat"** escape hatch — a `bridge_bot_room` command that resolves-or-creates the bot DM — is wired into the Bridge card's **Manage** menu and the login Sheet's **failure** state, navigating straight to the room (`setView("inbox")` + `selectRoom`).

**Files changed:**
- `src-tauri/crates/keeper-core/data/bot-commands.json` — new data-driven login/cancel command protocol (`default` + optional per-network overrides).
- `src-tauri/crates/keeper-core/src/bridges/transport/bot.rs` — new `BotDriver` (`Clone`) + pure `classify_bot_reply` + `BotReply` normalization; armed-before-send reply listener with an RAII `HandlerGuard`; full I/O-matrix + scripted-driver tests.
- `src-tauri/crates/keeper-core/src/bridges/data.rs` — `BotCommandsDoc`/`BotProtocol` + cached `bot_commands()` + `protocol_for` + validator.
- `src-tauri/crates/keeper-core/src/bridges/transport/{mod,provisioning}.rs` — register `bot`; `Provisioning::connect` → `Result<Option<Provisioning>, _>` (`Ok(None)` = no API → bot fallback) with a pure `classify_no_connection`.
- `src-tauri/crates/keeper-core/src/bridges/mod.rs` — `resolve_bot_mxid` + `resolve_bot_room` (find-or-create bot DM).
- `src-tauri/crates/keeper-core/src/error.rs` — `BridgeError::Bot` (retriable).
- `src-tauri/crates/keeper-core/src/account.rs` — `LoginTransport { Provisioning, Bot }` enum + provisioning-first/bot-fallback selection + `bridge_bot_room`.
- `src-tauri/crates/keeper/src/{ipc,lib}.rs` — `bridge_bot_room` command + `Bot` → `syncUnavailable` mapping + registration.
- `src/lib/ipc/client.ts` — `bridgeBotRoom` wrapper.
- `src/components/bridges/bridge-card.tsx` (+ test) — Manage menu "Open Bridge Bot chat".
- `src/components/bridges/bridge-login-sheet.tsx` (+ test) — failure-state escape-hatch button.

**Review findings breakdown:** 7 patches applied (5 medium — the reply-race listener ordering, the handler leak on cancel/abort, the QR false-positive from URLs/Matrix ids, the dropped image caption, and the false-success marker; 2 low — the wildcard step-type swallow and the silent escape-hatch failure); 0 deferred; 0 intent gaps; 0 bad_spec loopbacks; 16 rejected as inherent / by-design / documented residual risk / unreachable (see Review Triage Log).

**Verification (all re-run after the patches):**
- `bun run check:rust` — PASS (rustfmt clean + clippy `-D warnings`; no `async_fn_in_trait`, no `.unwrap()`).
- `bun run test:rust` — PASS (525 tests, up from 522).
- `bun run check` — PASS (Biome 199 files + tsc + 628 vitest + core-tauri-free guard).

**Follow-up review recommended:** `true`. The final pass changed the reply-handler concurrency/teardown path (arm-before-send + RAII removal on the cancellable-task path) and tightened the pure classifier (QR/success/normalization) — behavioral changes across the await/cancel lifecycle that benefit from an independent look, mirroring 6.3's follow-up recommendation after its session-lifecycle changes.

**Residual risks:** Live bot-chat behavior cannot be exercised unattended — the impure Matrix shell (bot DM resolve/create, `room.send`, await-next-reply-with-timeout) and live reply-grammar correctness are covered only by the pure `classify_bot_reply`/`bot_commands()`/`classify_no_connection` unit tests and the scripted-driver "same phases as Provisioning" contract test. Real bots' reply grammars and login commands vary by bridge/version; the classifier and `bot-commands.json` ship data-tunable so tuning needs no code change. Image-only QR (no extractable payload) is honestly out of scope (routes to the escape hatch). The bot MXID is composed on the account's own `server_name` (matching discovery); bridges whose bot lives on a different server would need broader work. `list-logins`/`logout`/`set-relay` over the bot arrive with Stories 6.5/6.6 when those trait methods exist.

---

### 2026-07-05 — Follow-up review pass (Auto Run Result addendum)

Status: done

An independent follow-up review (Blind Hunter + Edge Case Hunter, per the prior pass's `followup_review_recommended: true`) surfaced ~19 findings against the await/cancel lifecycle and the pure classifier. After verifying each against the code, **1 was a genuine new defect and got patched; the other 15 (deduped) were rejected** as documented residual risk of the impure Matrix shell, by-design per the intent contract, or already covered by the first pass — confirming the shipped implementation holds.

**Patch applied (`account.rs`, low):** cancel-orphan on a slow bot first reply. Because `BotDriver::login_start` sends the bot login command **before** awaiting the reply while `drive_login` records the `login_id` only *after* `login_start` returns `Ok`, a first-reply timeout left the slot `None` and both cancel paths (`cancel_bridge_login` + the shutdown drain) skipped `login_cancel`, orphaning a login already initiated on the bot. `LoginTransport::login_cancel(&str)` → `cancel_recorded(Option<String>)`: provisioning stays gated on a real login id; the bot arm fires its id-agnostic, idempotent cancel command even with none. `drive_login`/`step_to_vm`/`BridgeLoginVm` untouched.

**Verification (re-run after the patch):**
- `bun run check:rust` — PASS (rustfmt clean + clippy `-D warnings`; no `.unwrap()`).
- `bun run test:rust` — PASS (525 tests).
- `bun run check` — PASS (Biome 199 files + tsc + 628 vitest + core-tauri-free guard).

**Follow-up review recommended:** `false`. This pass made a single localized, low-consequence fix to best-effort cancel dispatch and otherwise confirmed the implementation; no further independent look is warranted.
