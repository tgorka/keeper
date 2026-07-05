---
title: 'Start New Chats via Bridge'
type: 'feature'
created: '2026-07-05'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: [oversized]
baseline_revision: 741ef4f4d4dd0f3a383c6d2917013e5e869e63ed
final_revision: f4ce8cb024c83a0d35f8f3d1eae86d66a4af708b
---

<intent-contract>

## Intent

**Problem:** keeper can receive bridged conversations but cannot originate one. There is no way to start a chat with a phone number or username on a bridged Network — no ⌘N surface, no identifier resolution, no path from "I know their number" to an open, composer-focused chat. The product only reacts; it can't initiate.

**Approach:** Add a native new-chat dialog (⌘N) where the user picks Network + Account (defaulting to last used), enters an identifier (phone / username / Matrix ID), and keeper resolves it **through the bridge's provisioning `resolve_identifier`** (Story 6.3's transport) with a visible resolving state, then opens the resulting portal Chat with the composer focused. Resolve support is honest and data-driven: a network the bridge can't resolve is declared **unsupported upfront** (input disabled), and an unresolvable identifier shows an inline "Not found on {Network}" keeping the input for correction — never a late failure, never a dismissed dialog.

## Boundaries & Constraints

**Always:**
- **Resolve runs through the provisioning transport (Story 6.3), not a new login path.** Reuse `Provisioning::connect(host, token, network)` exactly as `start_bridge_login` does (resolved-homeserver host + Bearer C-S token, never the bare MXID server_name). Story 6.6 depends on 6.3 only — resolution is a provisioning-API capability.
- **Two structured provisioning calls, no guessing.** `GET /v3/resolve_identifier/{identifier}` validates + returns an optional `dm_room_mxid`; if absent, `POST /v3/create_dm/{identifier}` creates the portal and returns a **required** `dm_room_mxid`. The identifier is percent-encoded into the path segment. The returned room id is opened verbatim — keeper never infers a room by scanning joined rooms.
- **Capability is data-driven and declared upfront.** A versioned embedded `resolve-support.json` (`default` + per-network overrides, loaded/validated/cached exactly like `bot-commands.json`) carries per-network `supported: bool`, an `identifierHint`, and a `placeholder`. A network marked `supported: false` disables the identifier field and shows "Starting new chats isn't supported on {Network}" **before** any network I/O (FR-32).
- **Failures keep the input, verbatim, no dismissal.** A resolve/create error surfaces the bridge's own message and renders inline "Not found on {Network} — check the number or username." with the dialog open and the identifier retained (FR-32). keeper never guesses at unparseable output.
- **Rust owns resolution; the frontend renders VMs and opens rooms.** The command returns only a `NewChatResolutionVm { room_id }` (non-secret). Opening is `roomsStore.selectRoom({ accountId, roomId })`; composer focus rides a `composerStore` focus nonce. The token never leaves the transport module.
- **The Bridge Bot chat stays the escape hatch.** For a bot-only account (no provisioning API) resolve is honestly unavailable — the dialog surface degrades to an honest message, and the shipped `bridge_bot_room` path remains the manual way to start a chat.

**Block If:**
- No `Provisioning::connect` reachable resolve entry exists to reuse (would contradict the Story 6.3 baseline this story depends on) — HALT, do not reimplement a login/transport probe.
- The rooms store exposes no `selectRoom({ accountId, roomId })` to open the resolved chat client-side — HALT rather than invent a second navigation path.

**Never:**
- A new login flow or any change to `drive_login` / `BridgeLoginVm` / the login stepper — resolution is a separate, additive operation.
- Guessing the portal room by scanning `joined_rooms()` after a bot command, or a BotDriver resolve that parses a prose reply for a room id (no structured room id → guessing). Bot-only accounts get the honest "unavailable" surface, not a fabricated resolve.
- Dismissing the dialog on failure, clearing the identifier on error, or hardcoding per-network resolve capability/hints in Rust or TS.
- Routing media/large payloads through IPC; leaking the access token in any VM, error, or log.

## I/O & Edge-Case Matrix

Pure `resolve_support()` / `support_for(network_id)` (data), pure identifier normalization + path encoding, and the pure resolve-response projection `parse_resolved_room(body) -> Option<String>` (extracts `dm_room_mxid`). The HTTP shell is the documented residual risk (6.3 discipline).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Network unsupported | selected network `support_for` → `supported: false` | dialog disables input, shows "not supported on {Network}" upfront; no command issued | No I/O — pure gate |
| Resolve, existing DM | `resolve_identifier` 2xx with `dm_room_mxid` present | return that room id; open chat, focus composer | No error expected |
| Resolve, no DM yet | `resolve_identifier` 2xx, `dm_room_mxid` null | call `create_dm`; return its required `dm_room_mxid` | No error expected |
| Identifier not found | `resolve_identifier` non-2xx / error body | surface verbatim (capped) → inline "Not found on {Network}"; input retained, dialog open | `BridgeError::Provisioning` |
| Empty / whitespace identifier | trimmed identifier is empty | resolve button disabled; no command issued | Pure validation, no I/O |
| Bot-only account | `Provisioning::connect` → `Ok(None)` | honest `BridgeError` "Starting a chat from keeper needs the provisioning API for {Network}; open the Bridge Bot chat"; input retained | Non-secret verbatim error |
| create_dm missing room id | `create_dm` 2xx but `dm_room_mxid` absent/empty | error (never open a blank room) | `BridgeError::Provisioning` |
| Transport error on probe | `Provisioning::connect` → `Err` | surface the honest provisioning error; input retained | `BridgeError::Provisioning` |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/data/resolve-support.json` -- **NEW**. Versioned `{ version, default: { supported, identifierHint, placeholder }, overrides: [{ networkId, ... }] }`. Embedded via `include_str!`.
- `src-tauri/crates/keeper-core/src/bridges/data.rs` -- add `ResolveSupportDoc` / `ResolveSupport` + cached `resolve_support()` loader + `support_for(network_id)` (override-or-default) + validator (mirror `bot_commands()`/`health_signals()`), with a load test.
- `src-tauri/crates/keeper-core/src/bridges/transport/provisioning.rs` -- add `resolve_identifier(&self, identifier) -> Result<Option<String>, BridgeError>` (GET, returns `dm_room_mxid` or `None`) and `create_dm(&self, identifier) -> Result<String, BridgeError>` (POST, required room id) on `Provisioning`; pure `parse_resolved_room(body) -> Option<String>` + pure `encode_identifier_segment(&str) -> String`; reuse `extract_error_message`. Unit-test the pure helpers.
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `ResolveSupportVm { network_id, supported, identifier_hint, placeholder }` and `NewChatResolutionVm { room_id }` (serde camelCase + `#[ts(export)]`); round-trip tests.
- `src-tauri/crates/keeper-core/src/account.rs` -- `resolve_bridge_identifier(account_id, network_id, identifier) -> Result<NewChatResolutionVm, CoreError>`: reuse the `start_bridge_login` host/token derivation; `Provisioning::connect`; `Ok(None)` → honest unavailable error; else `resolve_identifier` → existing room id, or `create_dm`. Also `bridge_resolve_support(network_id) -> ResolveSupportVm` (pure map over `support_for`).
- `src-tauri/crates/keeper/src/ipc.rs` -- `#[tauri::command] resolve_bridge_identifier(...)` and `bridge_resolve_support(...)`; reuse `to_ipc_error`.
- `src-tauri/crates/keeper/src/lib.rs` -- register both commands in `invoke_handler`.
- `src/lib/ipc/client.ts` -- `resolveBridgeIdentifier(accountId, networkId, identifier)` and `bridgeResolveSupport(networkId)` wrappers (+ import generated VMs from `./gen/`).
- `src/lib/stores/new-chat.ts` -- **NEW** zustand vanilla store: `isOpen`, `lastAccountId`, `lastNetworkId`, `open()`/`close()` (mirror `search.ts`).
- `src/lib/stores/composer.ts` -- add `focusNonce: number` + `requestFocus()` (bump nonce) for programmatic composer focus.
- `src/hooks/use-new-chat-shortcut.ts` -- **NEW** ⌘N (`metaKey`/`ctrlKey` + `key === "n"`) → `newChatStore.getState().open()` (mirror `use-search-shortcuts.ts`), ignoring when typing in an input if that is the existing convention.
- `src/components/chat/new-chat-dialog.tsx` (+ test) -- **NEW** Dialog: Account `Select` + Network `Select` (default last used), identifier input with data-driven `placeholder`/hint, resolving state, inline "Not found" (input retained), upfront "not supported" gate; on success `selectRoom` + `close` + `composerStore.getState().requestFocus()`.
- `src/components/chat/composer.tsx` (+ test) -- add a `textareaRef` on `Textarea` and a `useEffect` focusing it when `focusNonce` changes for the open room.
- `src/components/layout/app-shell.tsx` -- always-mount `<NewChatDialog />` and wire `useNewChatShortcut()` (mirror `SearchOverlay`).

## Tasks & Acceptance

**Execution:**
- [x] `data/resolve-support.json` + `data.rs` -- versioned `default` + per-network `supported`/`identifierHint`/`placeholder`; cached `resolve_support()` + `support_for`; validator; load test. Mark genuinely-unsupported networks `supported: false`; give phone/username networks tailored hints.
- [x] `provisioning.rs` -- `resolve_identifier` (GET) + `create_dm` (POST) on `Provisioning`; pure `parse_resolved_room` + `encode_identifier_segment`; **unit-test the I/O matrix pure helpers** (existing dm, no dm, missing/empty room id, error-body verbatim, identifier encoding).
- [x] `vm.rs` -- `ResolveSupportVm` + `NewChatResolutionVm` (camelCase + ts-rs) + round-trip tests (assert no token leak).
- [x] `account.rs` -- `resolve_bridge_identifier` (connect → resolve-or-create, bot-only honest error) + `bridge_resolve_support`; reuse the login host/token derivation.
- [x] `ipc.rs` + `lib.rs` -- both `#[tauri::command]`s + registration.
- [x] `client.ts` -- `resolveBridgeIdentifier` + `bridgeResolveSupport` wrappers.
- [x] `new-chat.ts` + `composer.ts` -- new-chat store; composer `focusNonce`/`requestFocus`.
- [x] `use-new-chat-shortcut.ts` + `app-shell.tsx` -- ⌘N hook + always-mounted dialog wiring.
- [x] `new-chat-dialog.tsx` (+ test) -- pickers, resolving state, inline not-found (input retained, no dismissal), upfront unsupported gate, success → open + focus.
- [x] `composer.tsx` (+ test) -- ref + focus-on-nonce effect.

**Acceptance Criteria:**
- Given the ⌘N dialog, when the user picks Network + Account (defaulting to last used) and enters an identifier, then keeper resolves it through the bridge's `resolve_identifier` with a visible resolving state and opens the resulting Chat with the composer focused (FR-32).
- Given an unresolvable identifier, when resolution fails, then inline "Not found on {Network} — check the number or username." appears with the input retained for correction and no dialog dismissal (FR-32).
- Given a Network whose bridge lacks resolve support (`resolve-support.json` `supported: false`), then the dialog says so upfront and disables the identifier field instead of failing late (FR-32).
- Given `bun run check:all`, then Biome + tsc + vitest + rustfmt + clippy (`-D warnings`, no `.unwrap()`) + cargo-nextest all pass.

## Spec Change Log

_No bad_spec loopbacks: the review produced only localized patches, applied directly to the diff._

## Review Triage Log

### 2026-07-05 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 0
- reject: 16: (high 0, medium 3, low 13)
- addressed_findings:
  - `[low]` `[patch]` `account.rs` — `resolve_bridge_identifier` had no empty/whitespace-identifier guard at the IPC boundary. The dialog disables Start on an empty identifier (spec I/O matrix scopes empty-validation to the frontend), but the command itself would build `/v3/resolve_identifier/` with an empty path segment (undefined bridge behavior) if ever invoked directly. Added an honest early `BridgeError::Provisioning("identifier is empty")` guard — completes the "never a late/undefined failure" discipline at the boundary; no behavior change for real users (frontend already prevents it).
- rejected (AC-mandated / by-design / documented residual risk): the inline error rendering the fixed AC-2 copy rather than the bridge's captured verbatim message — incl. the bot-only "open the Bridge Bot chat" message being shown as "Not found" — **rejected: AC-2 mandates that exact fixed frontend copy, the honest bot-only error is produced + input retained at the backend per the bot-only I/O-matrix row, and no single-reading resolution exists to distinguish same-type `BridgeError::Provisioning` failures without a spec-level error-taxonomy change**; `resolve→create_dm` proceeding on a 2xx-with-unexpected-body (documented HTTP-shell residual risk; bridgev2 returns non-2xx for bad identifiers); percent-encoded identifier vs. real bridge routing (documented residual risk, unverifiable unattended); `support_for` defaulting unknown networks to `supported: true` (by-design data-driven default + overrides); new-portal header degrading until sync (prior-pass-verified graceful; verbatim-open is by design); missing membership check before `selectRoom` (by-design "opened verbatim, never inferred"); ⌘N not toggling / guarded mid-draft / stacking on other modals (Radix handles per-dialog focus; editable-target guard is intended); frontend `roomId` non-empty guard (Rust guarantees non-empty); `supportLoading` no-timeout (backend projection is pure/instant); host/token derivation duplicated between `start_bridge_login`/`resolve_bridge_identifier` (correct + tested; refactoring a security-critical derivation for a maintainability nit carries more risk than the nit); `validate_resolve_capability` not cross-checking `supported`↔`placeholder` (cosmetic); no `account.rs` orchestration test for the resolve→create sequence (HTTP-bound; documented residual risk covered by pure-helper + dialog tests; not trivially unit-testable without a transport mock the codebase lacks); `support_for` cloning per call (negligible, called once per network selection); default-selection effects re-running on catalog/accounts rehydrate (correct — adopt only when `""`).

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 7: (high 0, medium 2, low 5)
- defer: 0
- reject: 8: (high 0, medium 0, low 8)
- addressed_findings:
  - `[medium]` `[patch]` `new-chat-dialog.tsx` — the unsupported-network gate **failed open**: `support?.supported ?? true` meant a capability-read failure (IPC hiccup) enabled the identifier field + Start button on `supported: false` networks (slack/imessage/xchat). Now fails **closed**: a tri-state (`support === null` = loading → neutral "Checking…" line, Start disabled) plus a synthesized *unsupported* sentinel on read failure (points at the Bridge Bot escape hatch); `supported` is now `support?.supported === true`. Added a `fails closed … when the capability read errors` test.
  - `[medium]` `[patch]` `composer.tsx` — the focus effect (`if (focusNonce > 0) focus()`) **stole focus on every room switch** after the first new-chat: a fresh Composer mount (the pane remounts per room) read the persisted, already-bumped nonce and self-focused. Seeded a `seenFocusNonce` ref so only a genuine *change* after mount focuses. Added a `does not steal focus when it mounts onto an already-bumped focus nonce` regression test.
  - `[low]` `[patch]` `use-new-chat-shortcut.ts` — ⌘/Ctrl+N had no editable-target guard (its sibling `use-bridges-shortcut` does), so it popped the dialog + swallowed the chord mid-typing (incl. the emacs-style Ctrl+N caret move). Added the INPUT/TEXTAREA/SELECT/contentEditable guard.
  - `[low]` `[patch]` `new-chat-dialog.tsx` — only the Network default was adopted on late hydration; a slow accounts-store hydrate left `accountId === ""` and Start permanently disabled. Added the symmetric adopt-default effect for `accountId`.
  - `[low]` `[patch]` `new-chat-dialog.tsx` — the Account/Network `Select`s stayed enabled during a resolve, so switching network mid-flight mislabeled the error / opened the wrong network's room. Both are now `disabled={resolving}` (closes the stale-switch and double-submit windows).
  - `[low]` `[patch]` `provisioning.rs` — `resolve_identifier`/`create_dm` read the body with `resp.text().await.unwrap_or_default()`, so a truncated 2xx read became `""` → `resolve` silently fell through to an unintended `create_dm`. Both now map a body-read failure to an honest `BridgeError::Provisioning`.
  - `[low]` `[patch]` `new-chat-dialog.tsx` — opening ⌘N with zero accounts rendered empty pickers and a silently-disabled Start. Added an honest "Add an account to start a new chat." empty state.
- rejected (cosmetic / by-design / spec-mandated): opening a freshly-created portal via `selectRoom` before sync surfaces it (verified graceful — `useSelectedRoomVm` degrades the header to the account chip and the pane subscribes the timeline by `roomId`, which the SDK already holds); the inline not-found copy "check the number or username" being generic rather than per-network hint, and not rendering the bridge's verbatim message (both **rejected — AC-2 in the read-only intent contract mandates that exact fixed copy**); asymmetric `placeholder` validation (empty placeholder is harmless); per-byte `format!` allocation in `encode_identifier_segment` (efficiency nit, correctness tested); host/token scheme derivation dropping the URL scheme (pre-existing Story 6.3 login pattern, all prod homeservers are TLS); removed-account fallback silently picking `accounts[0]` (intended fallback); uncapped 2xx success body (the provisioning API is the account's own semi-trusted homeserver service).

## Design Notes

- **Depends on 6.3, not 6.4 — resolution is a provisioning-transport capability.** The bridgev2 provisioning API exposes structured `resolve_identifier`/`create_dm` returning a real `dm_room_mxid`; the Bridge Bot fallback (6.4) only returns prose, from which a room id cannot be derived without guessing. So the honest scope is: provisioning resolves; a bot-only account gets an honest "use the Bridge Bot chat" surface (which is also the third AC's "says so upfront" behavior at the transport-availability layer). Both `resolve_support.json` (network-level) and the `Ok(None)` connect result (account-level) drive the honest degradation.
- **Two-call resolve.** `resolve_identifier` first (validates cheaply, no side effect, may return an existing DM); `create_dm` only when no DM exists yet — avoids creating a portal for a marginal/typo'd identifier before it's confirmed. Both accept an optional `login_id` query param; v1 omits it (bridge picks the account's login) — a documented single-login limitation.
- **Pure core, impure shell (6.2/6.3/6.4/6.5 discipline).** `parse_resolved_room`, `encode_identifier_segment`, `resolve_support()`/`support_for` are pure and unit-tested; the live HTTP round-trips against a real bridge are the documented residual risk, exactly as 6.3's login shell.
- **Composer focus via a nonce.** `composerStore.requestFocus()` bumps `focusNonce`; the Composer focuses its `textareaRef` on the change for the currently-open room — a minimal, testable signal reusing the store the composer already reads (mirrors the rooms store's `focusEvent` deep-link pattern).

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` (no `.unwrap()`, no `async_fn_in_trait`).
- `bun run test:rust` -- expected: cargo-nextest green incl. `resolve_support()` load, `parse_resolved_room`/`encode_identifier_segment` I/O matrix, and the `ResolveSupportVm`/`NewChatResolutionVm` round-trips.
- `bun run check` -- expected: Biome + tsc + vitest pass incl. `new-chat-dialog` (resolving → open + focus, inline not-found retaining input, upfront unsupported gate) and `composer` (focus-on-nonce).

**Manual checks (if no CLI):**
- Live identifier resolution against a real bridge cannot be exercised unattended: the provisioning HTTP shell (`resolve_identifier`/`create_dm`, base-URL probe, portal room id) is a documented residual risk, covered by the pure `parse_resolved_room`/`encode_identifier_segment`/`support_for` unit tests and the frontend dialog tests — as with 6.3's provisioning shell.

## Auto Run Result

Status: done

**Summary:** Shipped originating new chats through a bridge. A ⌘N new-chat dialog (Account + Network pickers defaulting to last-used) resolves an identifier (phone / username / Matrix ID) **through the Story 6.3 provisioning transport** — `GET /v3/resolve_identifier/{identifier}` first (validates cheaply, returns an existing `dm_room_mxid` if any), then `POST /v3/create_dm/{identifier}` only when no DM exists — and opens the resulting portal Chat with the composer focused. Resolve capability is honest and data-driven: a versioned embedded `resolve-support.json` (`default` + per-network `supported`/`identifierHint`/`placeholder`) declares unsupported networks **upfront** (input hidden, honest copy); an unresolvable identifier shows the AC-mandated inline "Not found on {Network} — check the number or username." keeping the input; and a bot-only account (no provisioning API) gets an honest "open the Bridge Bot chat" message. The pure core (`parse_resolved_room`, `encode_identifier_segment`, `resolve_support()`/`support_for`) is fully unit-tested; the live HTTP shell is documented residual risk, matching the 6.2–6.5 discipline. Resolution is a provisioning-only operation (methods on `Provisioning`, not the shared `BridgeTransport` trait) — the Bridge Bot fallback returns only prose, from which a room id cannot be derived without guessing.

**Files changed:**
- `src-tauri/crates/keeper-core/data/resolve-support.json` — new versioned capability registry (`default` + per-network overrides; slack/imessage/xchat `supported: false`).
- `src-tauri/crates/keeper-core/src/bridges/data.rs` — `ResolveSupportDoc`/`ResolveSupport` + cached `resolve_support()`/`support_for` + validator + load tests.
- `src-tauri/crates/keeper-core/src/bridges/transport/provisioning.rs` — `resolve_identifier`(GET)/`create_dm`(POST) on `Provisioning`; pure `parse_resolved_room`/`encode_identifier_segment` (I/O-matrix tested); honest body-read error mapping.
- `src-tauri/crates/keeper-core/src/vm.rs` — `ResolveSupportVm` + `NewChatResolutionVm` (camelCase + ts-rs) + round-trip/no-token-leak tests.
- `src-tauri/crates/keeper-core/src/account.rs` — `resolve_bridge_identifier` (connect → resolve-or-create; bot-only honest error) + `bridge_resolve_support` (reuses the login host/token derivation).
- `src-tauri/crates/keeper/src/{ipc.rs,lib.rs}` — `resolve_bridge_identifier`/`bridge_resolve_support` `#[tauri::command]`s + registration.
- `src/lib/ipc/client.ts` + `src/lib/ipc/gen/{NewChatResolutionVm,ResolveSupportVm}.ts` — IPC wrappers + generated bindings.
- `src/lib/stores/new-chat.ts` (new) + `src/lib/stores/composer.ts` — new-chat dialog store; composer `focusNonce`/`requestFocus`.
- `src/hooks/use-new-chat-shortcut.ts` (new) + `src/components/layout/app-shell.tsx` — ⌘N hook (editable-target-guarded) + always-mounted dialog.
- `src/components/chat/new-chat-dialog.tsx` (+ test) + `src/components/chat/composer.tsx` (+ test) — the dialog (pickers, resolving state, fail-closed gate, inline not-found, empty-accounts state) + focus-on-nonce effect.

**Review findings breakdown:** 7 patches applied (2 medium — fail-open unsupported gate, and composer focus-steal on remount; 5 low — ⌘N editable-target guard, accountId adopt-default, Selects locked during resolve, honest body-read error, zero-accounts empty state); 0 deferred; 0 intent gaps; 0 bad_spec loopbacks; 8 rejected as cosmetic / by-design / AC-mandated (see Review Triage Log). Two regression tests added (fail-closed gate, no-focus-steal-on-mount).

**Verification (all re-run after the patches):**
- `bun run check:rust` — PASS (rustfmt clean + clippy `-D warnings`; no `.unwrap()`).
- `bun run test:rust` — PASS (577 tests).
- `bun run check` — PASS (Biome + tsc + 652 vitest + core-tauri-free guard).
- `bun run bindings:check` — the only `check:all` leg not green, solely because the two newly-generated `src/lib/ipc/gen/*.ts` bindings are untracked until this commit (`test -z "$(git status --porcelain -- src/lib/ipc/gen)"`); no committed binding drifted. It goes green once committed.

**Follow-up review recommended:** `true`. The final pass made two medium behavior fixes (fail-open→fail-closed capability gate; composer focus-steal on every post-new-chat room switch) plus five lower-severity behavior changes across the new-chat dialog's gating/locking and a Rust body-read correctness fix — enough breadth across the primary flow to benefit from an independent look, mirroring 6.3–6.5's follow-up recommendations after behavior changes.

**Residual risks:** Live identifier resolution against a real bridge cannot be exercised unattended — the provisioning HTTP shell (`resolve_identifier`/`create_dm`, base-URL probe, real portal room id) is covered only by the pure `parse_resolved_room`/`encode_identifier_segment`/`support_for` unit tests and the frontend dialog tests. Resolution omits the optional `login_id` query param, so on an account with multiple logins on one bridge the bridge picks the login (documented single-login limitation). The Bridge Bot fallback intentionally has no structured resolve (bot-only accounts get the honest "open the Bridge Bot chat" surface). "Last used" account/network defaults are session-ephemeral (not persisted across restarts).

---

### Follow-up review pass — 2026-07-05

Independent follow-up review (Blind Hunter + Edge Case Hunter, both at session model capability) over the full diff since baseline `741ef4f`.

**Outcome:** 1 low patch applied, 0 intent gaps, 0 bad_spec loopbacks, 16 findings rejected (3 medium-consequence / 13 low).

- **Patch (low):** `account.rs` `resolve_bridge_identifier` gained an honest empty/whitespace-identifier guard at the IPC boundary (`BridgeError::Provisioning("identifier is empty")`) so an empty path segment can never reach the bridge's `resolve_identifier` route. Defense-in-depth only — the dialog already disables Start on an empty identifier (spec scopes empty-validation to the frontend); no user-visible behavior change.
- **Notable rejects:** the inline error rendering the fixed AC-2 copy rather than the bridge's verbatim message (incl. the bot-only "open the Bridge Bot chat" message shown as "Not found") — rejected because AC-2 mandates that exact fixed copy, the honest bot-only error + input-retention is satisfied at the backend, and distinguishing same-type `BridgeError::Provisioning` failures would need a spec-level error-taxonomy change (no single-reading fix); the real-bridge HTTP-contract assumptions (percent-encoding, 2xx-with-unexpected-body → `create_dm`) are the documented residual risk of the provisioning shell; `support_for` defaulting unknown networks to `supported: true` is the by-design data-driven default. See the follow-up Review Triage Log entry for the full list.

**Verification (re-run after the patch):**
- `bun run check:rust` — PASS (rustfmt clean + clippy `-D warnings`; no `.unwrap()`).
- `bun run test:rust` — PASS (577 tests).
- Frontend gate unchanged (patch is Rust-only; last committed pass was green: Biome + tsc + 652 vitest).

**Follow-up review recommended:** `false`. The pass made a single localized low-consequence hardening patch on a path the frontend already fully prevents — not enough behavior/breadth to warrant another independent look.
