---
title: 'Native Bridge Login via Provisioning API'
type: 'feature'
created: '2026-07-05'
status: 'done'
baseline_revision: '59488d51706aaf1ad45452fbca52527435c39750'
final_revision: 'a2af983263049f74b6f9367f5ee9801cca1d2840'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-6-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** The Bridges surface (6.1/6.2) discovers bridges and shows honest per-Network status, but the card's primary action is still a stub (volatile ack gate → no-op). A user who clicks **Connect** cannot actually log a Network in from inside keeper — they must drop into a raw Bridge Bot chat and type `!wa login`, exactly the setup-cliff this epic exists to remove (FR-26).

**Approach:** Introduce a `BridgeTransport` trait in `keeper-core` with its first impl, `Provisioning`, that drives the **mautrix bridgev2 HTTP+JSON provisioning API** (`/_matrix/provision/v3/login/{flows,start,step,cancel}`) authenticated with the account's own Matrix access token as Bearer. A generic driver translates the server's login state machine into a `BridgeLoginVm` streamed over a Tauri `Channel` (modeled on `verification_subscribe`), with `bridge_login_start` / `bridge_login_submit` / `bridge_login_cancel` commands. The frontend renders a native login **Sheet** stepper (choosing method → waiting → QR panel *or* code-entry → success/failure), wiring the Bridge card's Connect action to open it. QR is rendered in Rust to an SVG string (reusing Story 3.2's `qrCodeSvg` → `<img src="data:image/svg+xml,…">` pattern).

## Boundaries & Constraints

**Always:**
- The `Provisioning` transport is the first impl of a `BridgeTransport` trait shaped so 6.4's `BotDriver` becomes a drop-in behind the same driver — the emitted stepper states are transport-agnostic (a login must look identical whichever transport powered it).
- Auth uses the account's live Matrix access token (`client.access_token()`) as `Authorization: Bearer`; **the token never crosses IPC** — only rendered `BridgeLoginVm` state (phase, instruction copy, QR SVG, non-secret field descriptors, verbatim error) reaches the frontend.
- The provisioning base URL is resolved by a **data-driven, ordered probe** (versioned `data/provisioning.json` candidate templates with `{server}` substitution; first `GET …/v3/login/flows` that authenticates wins) — per AD-16 "config key + probe order, an implementation detail inside the transport". The candidate-building is a pure, unit-tested function.
- Every login state renders as a **distinct** native state; a provisioning failure surfaces the bridge's **own error message verbatim** with Retry — keeper never guesses at unparseable output.
- The QR panel sits on a mandatory white card ≥ 240 px with quiet zone in **both** themes, a per-network instruction line, and a live state word; QR expiry (a fresh `display_and_wait` from the long-poll) regenerates in place with a subtle "QR refreshed" note. On `complete` the state flips to "Linked ✓" in bridge-healthy green with an auto-advance (~1.5 s).
- No `.unwrap()` / bare `.expect()` in Rust production paths; transport/HTTP/serde failures become `BridgeError::Provisioning(String)` (retriable) carrying the bridge's verbatim message; individual failures are logged via `tracing`.
- Do not add `async-trait` (the codebase deliberately avoids it — see `auth.rs`): the trait uses native async fns dispatched **statically** via a generic driver (`drive_login<T: BridgeTransport>`), and must satisfy clippy `-D warnings` (`async_fn_in_trait`).

**Block If:**
- Driving a native login for a target Network would require shipping a webview/cookie-harvest engine or a passkey (WebAuthn) ceremony (the bridgev2 `cookies` / `webauthn` step types) — those are a separate story, not a decision to improvise here.

**Never:**
- No `BotDriver` transport, bot-command parsing, or the "Open Bridge Bot chat" escape-hatch wiring — that is Story 6.4 (the failure-state copy may *name* the bot chat honestly, but no navigation is wired here).
- No live health state machine, 60 s polling, or re-login prompts (Story 6.5); no `list-logins` / `logout` / `set-relay` trait methods (added when 6.5/6.6 exercise them — defining unused ones now is dead code).
- No Wizard integration (Story 6.8) and no new-chat/identifier resolution (Story 6.6).
- Native rendering of the `cookies` (browser-extension) and `webauthn` (passkey) step types: on encountering one, the stepper enters a distinct **unsupported-method** failure state with honest copy, never a half-built webview or a fake success.
- No shared-secret provisioning auth (requires an admin secret the user does not hold) — Matrix-access-token auth only.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Single-flow QR login | Bridge returns one flow; `start` → `display_and_wait{type:"qr", data}` | Stepper: waiting → QR panel (SVG on white card + instruction + state word); long-poll returns `complete` → Success "Linked ✓" auto-advance | n/a |
| Multiple flows | `GET /login/flows` returns 2+ flows | `choosingMethod` phase lists flows; user picks → `bridge_login_submit(ChooseFlow)` → `start` | n/a |
| Code / phone entry | step is `user_input{fields:[phone_number],[2fa_code]}` | `codeEntry` phase renders labeled fields (InputGroup for codes); submit posts body keyed by field `id` → next step | Field `pattern` (if any) validated client-side before submit |
| QR expiry / refresh | long-poll returns a **new** `display_and_wait{qr}` before `complete` | QR panel regenerates in place; `qrRefreshed: true` drives a subtle "QR refreshed" note | n/a |
| Provisioning failure | any step returns a non-2xx / bridge error body | `failure` phase shows the bridge's message **verbatim** + Retry | `BridgeError::Provisioning(msg)`; retriable |
| Unsupported method | step type is `cookies` or `webauthn` | distinct unsupported-method failure state with honest copy (names Bridge Bot chat as the manual path); no webview | Not an error toast — a rendered terminal state |
| Base URL unresolved | no `provisioning.json` candidate authenticates `…/v3/login/flows` | `failure` phase: "Couldn't reach a provisioning API for {network}." + Retry | `BridgeError::Provisioning`; retriable |
| Cancel / close | user closes the Sheet or presses Esc mid-flow | `bridge_login_cancel` aborts the driver task and POSTs `/login/cancel/{loginId}` best-effort; session removed | n/a |
| Unknown account/session | `bridge_login_start("bogus", …)` or submit with a stale `sessionId` | Command returns an `IpcError` | `AccountNotFound` / session-miss |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/data/provisioning.json` -- NEW; versioned ordered base-URL candidate templates (`{server}` substitution), embedded via `include_str!` like `known-bots.json`.
- `src-tauri/crates/keeper-core/src/bridges/transport/mod.rs` -- NEW; `pub trait BridgeTransport` (native async fns: `login_flows`, `login_start(flow_id, login_id)`, `login_step(login_id, step_id, step_type, body)`, `login_cancel(login_id)`) + the internal `LoginStep`/`LoginFlow`/`LoginField` types (serde, discriminated on `type` incl. `cookies`/`webauthn`/`complete`).
- `src-tauri/crates/keeper-core/src/bridges/transport/provisioning.rs` -- NEW; `Provisioning` impl over `reqwest` (bearer auth, base-URL probe using `resolve_candidates`), serde of the bridgev2 wire shapes (note `login_id` is `#[serde(flatten)]` alongside the step). Pure `resolve_candidates(server, doc) -> Vec<String>` unit-tested.
- `src-tauri/crates/keeper-core/src/bridges/login.rs` -- NEW; the generic driver `drive_login<T: BridgeTransport>(transport, sink, input_rx)` running the state loop + pure `step_to_vm(step, refreshed) -> BridgeLoginVm` and `qr_svg(data) -> String` (via `qrcode` crate, SVG feature — same as `verification`). Unit-test the translation matrix + QR SVG.
- `src-tauri/crates/keeper-core/src/bridges/mod.rs` -- register `transport` + `login` submodules.
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `BridgeLoginVm { networkId, phase, instruction?, qrSvg?, qrRefreshed, fields, flows, error? }`, `BridgeLoginPhase { ChoosingMethod|Waiting|Qr|CodeEntry|Success|Failure }`, `LoginFieldVm`, `LoginFlowVm`, `BridgeLoginInput { ChooseFlow{flowId} | Fields{values} }` (ts-rs `#[ts(export)]`, camelCase) + round-trip test.
- `src-tauri/crates/keeper-core/src/error.rs` -- add `BridgeError::Provisioning(String)` (retriable); keep existing arms.
- `src-tauri/crates/keeper-core/src/account.rs` -- add `BridgeLoginSink` type + per-account login-session registry (`session_id` via `NEXT_SUBSCRIPTION_ID`, holding the input `mpsc::Sender` + task handle); `start_bridge_login`, `submit_bridge_login`, `cancel_bridge_login`.
- `src-tauri/crates/keeper/src/ipc.rs` -- add `#[tauri::command]` `bridge_login_start(state, account_id, network_id, channel: Channel<BridgeLoginVm>) -> u64`, `bridge_login_submit(state, account_id, session_id, input)`, `bridge_login_cancel(state, account_id, session_id)`; map `BridgeError::Provisioning` in `to_ipc_error` (syncUnavailable/retriable).
- `src-tauri/crates/keeper/src/lib.rs` -- register the three commands in `invoke_handler`.
- `src/lib/ipc/client.ts` -- `startBridgeLogin(accountId, networkId, onState)`, `submitBridgeLogin(accountId, sessionId, input)`, `cancelBridgeLogin(accountId, sessionId)` wrappers (reuse the `subscribe<T>` helper) + re-export new VM types.
- `src/hooks/use-bridge-login.ts` -- NEW; manages start/submit/cancel + current-`BridgeLoginVm` state, unmount/close cleanup (mirror `use-encryption-statuses` + `use-bridge-discovery` guards).
- `src/components/bridges/bridge-login-sheet.tsx` -- NEW; `Sheet` rendering the state machine per phase (RadioGroup for method choice, QR panel white card, InputGroup code entry, success/failure). Colocated `*.test.tsx` covering each phase.
- `src/components/bridges/bridge-card.tsx` -- wire the Connect primary action (after the volatile ack gate) to open the login Sheet instead of the no-op `proceed()` stub.
- `src/lib/bridges.ts` (+ test) -- `BridgeLoginPhase → state word` label map (Connecting / Waiting / Scan QR / Enter code / Linked ✓ / Couldn't connect).

## Tasks & Acceptance

**Execution:**
- [x] `vm.rs` -- define `BridgeLoginVm`, `BridgeLoginPhase`, `LoginFieldVm`, `LoginFlowVm`, `BridgeLoginInput` (ts-rs export, camelCase) + round-trip/export test.
- [x] `error.rs` -- add `BridgeError::Provisioning(String)`; leave `Data`/`AccountNotFound`/`Discovery` unchanged.
- [x] `data/provisioning.json` -- versioned ordered candidate templates (default `https://{server}/_matrix/provision`).
- [x] `bridges/transport/mod.rs` -- `BridgeTransport` trait (native async, statically dispatched) + serde `LoginStep`/`LoginFlow`/`LoginField` wire types (`login_id` flattened; `type`-tagged incl. `cookies`/`webauthn`/`complete`).
- [x] `bridges/transport/provisioning.rs` -- `Provisioning` reqwest impl (bearer = account access token; `resolve_candidates` probe of `…/v3/login/flows`; step path `…/step/{loginId}/{stepId}/{stepType}`) + unit-test `resolve_candidates`.
- [x] `bridges/login.rs` -- `drive_login<T: BridgeTransport>` loop + pure `step_to_vm` + `qr_svg`; unit-test the I/O matrix translations (qr, user_input, complete→success, cookies/webauthn→unsupported, error→verbatim failure, qr-refresh flag) and the QR SVG.
- [x] `bridges/mod.rs` -- register `transport` + `login` submodules.
- [x] `account.rs` -- login-session registry + `start_bridge_login` / `submit_bridge_login` / `cancel_bridge_login` (resolve live `Client`, build `Provisioning`, spawn `drive_login`, feed input, abort on cancel).
- [x] `ipc.rs` + `lib.rs` -- add & register `bridge_login_start` / `_submit` / `_cancel`; map `Provisioning` in `to_ipc_error`.
- [x] `client.ts` + `use-bridge-login.ts` -- typed wrappers + the start/submit/cancel hook with cleanup.
- [x] `bridges.ts` (+ test) -- `BridgeLoginPhase` → state-word label map.
- [x] `bridge-login-sheet.tsx` (+ test) -- the Sheet stepper: choosing-method, waiting, QR panel (white card ≥240px both themes), code entry, success auto-advance, failure verbatim + Retry, unsupported-method state.
- [x] `bridge-card.tsx` (+ test) -- Connect (post-ack) opens the login Sheet.

**Acceptance Criteria:**
- Given a Bridge exposing the bridgev2 provisioning API, when the user clicks Connect on its card, then the login Sheet drives the provisioning JSON state machine natively (choosing method → waiting → QR panel or code-entry → success/failure), each state rendered distinctly, and the transport is the `Provisioning` impl of the `BridgeTransport` trait (FR-26, AD-16).
- Given the WhatsApp QR flow, when the QR renders, then it sits on a white card ≥ 240 px with quiet zone in both themes, with the per-network instruction line and a live state word; a `complete` step flips the state to "Linked ✓" in bridge-healthy green with auto-advance, and a fresh `display_and_wait` regenerates the QR in place with a "QR refreshed" note (FR-26, UX-DR8).
- Given a provisioning failure, then the failure state shows the Bridge's own error message verbatim with Retry (FR-26).
- Given a `cookies`/`webauthn` step type, when the driver reaches it, then a distinct unsupported-method state renders (no webview, no fake success) rather than the flow silently stalling.
- Given the account's Matrix access token, when the transport authenticates, then it is sent only as an HTTP Bearer header from Rust and never appears in any `BridgeLoginVm` crossing IPC (NFR secret containment).
- Given `bun run check:all`, when run, then Biome + tsc + vitest + rustfmt + clippy (`-D warnings`) + cargo-nextest all pass and `BridgeLoginVm.ts` / `BridgeLoginPhase.ts` / `LoginFieldVm.ts` / `LoginFlowVm.ts` / `BridgeLoginInput.ts` are generated under `src/lib/ipc/gen/`.

## Spec Change Log

_No bad_spec loopbacks: the review produced only localized patches, applied directly to the diff._

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 6: (high 0, medium 2, low 4)
- defer: 0
- reject: 12: (high 0, medium 0, low 12)
- addressed_findings:
  - `[medium]` `[patch]` cancel path was a no-op contract violation: `cancel_bridge_login` only `task.abort()`'d and `BridgeTransport::login_cancel` was dead code, while four doc comments (login.rs, account.rs, ipc.rs, client.ts) falsely claimed a best-effort `/login/cancel` POST — a direct deviation from the spec I/O-matrix "Cancel/close" row that leaked a server-side login session on every cancel/close. Made `Provisioning` `Clone`, threaded a shared `login_id` slot into `drive_login` (populated after `login_start`), stored a transport clone + the slot on `LoginSession`, and had `cancel_bridge_login` **and** the graceful-shutdown drain detached-spawn `login_cancel(&id)` before aborting (natural-completion reaper deliberately does not). Doc comments corrected; slot-population asserted in the driver test.
  - `[medium]` `[patch]` `bridge-login-sheet.tsx`: `CodeEntryPanel`/`ChoosingMethodPanel` `useState` initialized once, so a second sequential `user_input` step (e.g. Telegram phone→2FA) reused stale field values/keys. Keyed both panels by their field/flow id signature to force a remount on a new step; regression test re-renders with a different field set and asserts the reset.
  - `[low]` `[patch]` `use-bridge-login.ts`: the shared `cancelledRef` was reset to `false` at the top of each effect run, defeating a prior in-flight run's guard so a rapid Retry could orphan the previous backend session. Replaced with a per-run local `cancelled` flag (the `use-encryption-statuses` pattern).
  - `[low]` `[patch]` `provisioning.rs`: `parse_step_response` read the non-2xx error body unbounded and surfaced it verbatim into the VM/DOM. Added `MAX_ERROR_MESSAGE_CHARS = 2000` + a char-boundary `cap_message`, applied to both the JSON and raw-text branches; test pins the truncation.
  - `[low]` `[patch]` `bridge-login-sheet.tsx`: field `pattern` was compiled unanchored (`.test` substring-matches), so `[0-9]{6}` accepted `123456x`. Anchored to `^(?:…)$`; test rejects the trailing-garbage case and accepts the clean one.
  - `[low]` `[patch]` `login.rs`: `display_network_name` title-cased the raw id → "Whatsapp"/"Linkedin" in the QR instruction copy. Now resolves the canonical name from the 6.1 `catalog()` by `network_id` (falls back to title-case only if absent); test asserts "WhatsApp".
- rejected (noise / unreachable / speculative): URL path segments not percent-encoded (bridgev2 `login_id`/`step_id`/`flow_id` are server-generated URL-safe opaque ids); no `display_and_wait` tight-loop cap (a compliant bridgev2 server long-polls — only a non-compliant bridge on the user's own homeserver could spin); channel-close leaves the Sheet on a spinner (unreachable — the Bridges pane is keyed by `accountId`, so sign-out remounts and the hook cleanup cancels; the driver has no non-terminal exit but sink/input-closed, both meaning the frontend is already tearing down); buffered stale input auto-applied to a later step (unreachable — the Sheet exposes no submit affordance during a QR/waiting long-poll); long-poll exceeding the 120 s step timeout surfaces a spurious failure (bridgev2 long-polls return/rotate within the window, and the failure is Retry-able); QR-refresh flag lost across a non-QR intermediate step (cosmetic); `#[serde(other)] Unknown` could swallow a shape-drifted `complete` into unsupported-method (speculative wire drift on a stable success shape); the base-URL probe success predicate is "2xx + parseable" not "has flows" (unreachable with the single default candidate); token leaking via a reqwest error `Display` (no token in the URL — only the server name — and the 2000-char cap now bounds it); numeric `inputMode` on alphanumeric 2fa codes (a hint, never blocks input); `Cookies`/`Webauthn` variants absorbing unknown fields (by design — they map to the unsupported-method state regardless).

### 2026-07-05 — Review pass (follow-up)

Independent follow-up review (Blind Hunter + Edge Case Hunter) recommended by the prior pass. 20 findings raised (2 duplicated across reviewers → 18 unique); every one re-verified against the actual source.

- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 1, medium 0, low 0)
- defer: 0
- reject: 17: (high 0, medium 0, low 17)
- addressed_findings:
  - `[high]` `[patch]` The provisioning base-URL probe sent the account's live Matrix **access token** (`Authorization: Bearer`) to `https://{server_name}/_matrix/provision`, where `{server}` was substituted with the bare MXID `server_name` (`account.rs:1299` `client.user_id().server_name()`). Under `.well-known` delegation the `server_name` host can be a different host — potentially operated by a different party — than the token-issuing homeserver, so the probe (which sends the Bearer header even on a failing candidate) can leak the credential to an unintended host. Fixed by deriving the probe host from the **resolved homeserver** (`client.homeserver()` host+port), the C-S host the token already belongs to and where the bridgev2 provisioning API is co-located — both more secure and more likely correct (matching `auth.rs`'s resolved-homeserver discipline; unusual deployments still extend `provisioning.json`). `resolve_candidates` stays a pure function of whatever server string is passed, so its unit test is unaffected; `bun run check:rust` + `test:rust` (499) + `check` (626) all green.
- rejected (verified unreachable / by-design / already-owned): connect probe blocks `bridge_login_start` up to the 30 s `SHORT_TIMEOUT` on an unreachable host (by design — surfaces the error synchronously with a closable Sheet + Retry, bounded, single default candidate); N hidden `BridgeLoginSheet`/`useBridgeLogin` instances per pane (gated on `active && attempt !== 0`, no session started — latent, not a live bug); stray `start()` re-fire after success (verified false — the open-effect keys on stable `start` + `open`, fires once per open transition); post-terminal snapshot flips success back (verified false — the driver emits success then terminates and the Tauri Channel is ordered); Sheet unmount during the 1.5 s success window leaves it re-opening (verified false — `loginOpen` is card-local `useState(false)`, reset on remount); optional `user_input` field blocks submit (the bridgev2 `LoginField` wire shape carries no required/optional discriminator — all-required is the only well-defined behavior); numeric `inputMode` on alphanumeric codes (a keyboard hint, never blocks input — already rejected in the prior pass); malformed `pattern` silently accepted (the safe fail-open choice — never block on a bridge bug); `login_id`-slot Esc race between `login_start` resolving and the slot write (sub-millisecond window, best-effort cancel semantics already owned by the prior pass); QR data-URI (verified NOT a defect — mirrors the verification precedent); empty-`flows` probe accepts a 2xx host (mooted by the homeserver-host fix — a generic 200 `{}` from the C-S host path is implausible, and prior pass rejected it); detached `login_cancel` not awaited on shutdown (best-effort by design, prior pass owned it); reaper/insert race leaving a stale session entry (theoretical only — the driver's first act is a network await, so it cannot reap before the ~12-line synchronous insert; consequence is one idle map entry reclaimed at account teardown; matches the file's established spawn-then-register pattern); buffered stale input auto-applied to a later step (unreachable — the Sheet exposes no submit affordance during a QR/waiting long-poll); unvalidated `flow_id` in `ChooseFlow` (unreachable from the UI, which offers only `vm.flows`; graceful bridge-error + Retry if it ever occurred); identical field-id set on a sequential `user_input` step keeps prior values (cosmetic re-prompt pre-fill; a robust fix needs a VM step discriminator for negligible benefit).

## Design Notes

- **Trait without async-trait.** `auth.rs` documents the codebase's no-`async-trait` stance. Define `BridgeTransport` with native async fns and dispatch statically: `drive_login<T: BridgeTransport>(transport: T, …)`. `account.rs` picks the concrete transport (always `Provisioning` in 6.3) before calling the generic driver, so 6.4 adds a `BotDriver` branch with zero driver changes and no trait object. To keep clippy (`-D warnings`) happy about `async_fn_in_trait`, give the trait methods `-> impl Future<Output = …> + Send` return signatures (or a scoped, documented `#[allow]`).
- **Session model mirrors verification.** `bridge_login_subscribe`-equivalent is folded into `bridge_login_start` (clicking Connect *is* the initiation, unlike verification's passive wait). The running driver task holds an input `mpsc::Receiver`; `bridge_login_submit` pushes a `BridgeLoginInput` into it (flow choice or field values); `bridge_login_cancel` aborts the task and best-effort POSTs `/login/cancel`. Sessions are keyed by a `u64` from the existing `NEXT_SUBSCRIPTION_ID`.
- **QR is Rust-rendered SVG.** Reuse Story 3.2's exact pattern: `qrcode::QrCode(data).render::<svg::Color>()...` → an SVG string in the VM; the frontend renders `<img src={`data:image/svg+xml,${encodeURIComponent(qrSvg)}`}>` (see `device-verification-dialog.tsx:145`). No JS QR lib, no `dangerouslySetInnerHTML`, no base64 media over IPC.
- **bridgev2 wire shape (from mautrix/go `bridgev2/login.go`):** step path is `…/login/step/{loginId}/{stepId}/{stepType}` (the step *type* is the final segment). Responses `#[serde(flatten)]` `login_id` beside the `type`-tagged step. `display_and_wait` is a server-side long-poll (generous reqwest timeout; a returned fresh `display_and_wait` = QR rotation). `complete` carries `user_login_id` (terminal). Auth = `Authorization: Bearer {matrix access token}` (the `AllowMatrixAuth` mode).
- **VM shape follows `VerificationFlowVm`:** one struct with a `phase` enum + optional payload fields (`qrSvg`, `fields`, `flows`, `error`), so the frontend renders a familiar phase-switch. `LoginFieldVm.fieldType` carries the provisioning field type (phone_number / 2fa_code / password / token / …) so the Sheet can pick an input treatment (segmented InputGroup for codes, masked for password).

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` (no `async_fn_in_trait` warning, no `.unwrap()`).
- `bun run test:rust` -- expected: cargo-nextest green incl. new `login`/`transport` translation + `resolve_candidates` + QR-SVG tests and the VM round-trip; ts-rs exports the five new `.ts` bindings.
- `bun run check` -- expected: Biome + tsc + vitest pass incl. `bridge-login-sheet` (all phases), `bridge-card` (Connect opens Sheet), and `bridges.ts` label tests.

**Manual checks (if no CLI):**
- Live provisioning against a real bridge cannot be exercised unattended; the transport's impure HTTP shell is covered only by the pure `resolve_candidates` / `step_to_vm` / `qr_svg` unit tests — live base-URL correctness and long-poll behavior are a documented residual risk (as with 6.2's discovery shell).

## Auto Run Result

Status: done

**Summary:** Shipped fully native bridge login. A new `BridgeTransport` trait in `keeper-core` (native async fns, static dispatch — no `async-trait`, so Story 6.4's `BotDriver` is a drop-in) has its first impl, `Provisioning`, driving the mautrix **bridgev2 HTTP+JSON provisioning API** (`/_matrix/provision/v3/login/{flows,start,step,cancel}`) authenticated with the account's own Matrix access token as `Authorization: Bearer` (the token never crosses IPC). The base URL is resolved by a data-driven ordered probe (`data/provisioning.json` `{server}` templates; first `…/v3/login/flows` that authenticates wins). A generic `drive_login` translates each provisioning `LoginStep` into a `BridgeLoginVm` streamed over a Tauri `Channel` (modeled on `verification_subscribe`), via `bridge_login_start` / `bridge_login_submit` / `bridge_login_cancel`. The frontend renders a native login **Sheet** stepper — choosing method (RadioGroup) → waiting → QR panel (Rust-rendered SVG on a mandatory white card ≥ 240 px, both themes, with a "QR refreshed" note on rotation) *or* code-entry (InputGroup, client-side `pattern`-validated) → success ("Linked ✓", bridge-healthy green, ~1.5 s auto-advance) / failure (bridge error **verbatim** + Retry). The Bridge card's Connect action (post volatile-ack) opens it. `cookies`/`webauthn` step types render a distinct honest **unsupported-method** state (no webview, no fake success) that names the Bridge Bot chat as the manual path.

**Files changed:**
- `src-tauri/crates/keeper-core/data/provisioning.json` — new versioned ordered base-URL candidate templates.
- `src-tauri/crates/keeper-core/src/bridges/transport/mod.rs` — new `BridgeTransport` trait + bridgev2 wire types (`LoginFlow`/`LoginField`/`LoginStep`/`DisplayData`/`LoginStepResponse`, `login_id` flattened, `type`-tagged incl. `cookies`/`webauthn`/`complete`/`#[serde(other)] Unknown`).
- `src-tauri/crates/keeper-core/src/bridges/transport/provisioning.rs` — new `Provisioning` reqwest impl: `Clone`, base-URL probe (`resolve_candidates`), bearer auth, verbatim-error extraction with a 2000-char cap, best-effort `login_cancel`.
- `src-tauri/crates/keeper-core/src/bridges/login.rs` — new generic `drive_login<T: BridgeTransport>` state loop + pure `step_to_vm` + `qr_svg` (SVG, ≥240px, quiet zone) + catalog-sourced display name; unit + scripted-driver tests.
- `src-tauri/crates/keeper-core/src/bridges/{mod,data}.rs` — register `transport`/`login`; `ProvisioningDoc` + cached `provisioning()`.
- `src-tauri/crates/keeper-core/src/vm.rs` — `BridgeLoginVm`/`BridgeLoginPhase`/`LoginFieldVm`/`LoginFlowVm`/`BridgeLoginInput` (ts-rs export) + round-trip/no-secret-leak tests.
- `src-tauri/crates/keeper-core/src/error.rs` — `BridgeError::Provisioning` (retriable).
- `src-tauri/crates/keeper-core/src/account.rs` — `LoginSession` registry (transport clone + `login_id` slot) + `start_/submit_/cancel_bridge_login`; graceful-shutdown drain posts best-effort cancel.
- `src-tauri/crates/keeper/src/{ipc,lib}.rs` — `bridge_login_start/_submit/_cancel` commands + `Provisioning`→`syncUnavailable` mapping + registration.
- `src/lib/ipc/gen/{BridgeLoginVm,BridgeLoginPhase,BridgeLoginInput,LoginFieldVm,LoginFlowVm}.ts` — generated bindings.
- `src/lib/ipc/client.ts` — `startBridgeLogin`/`submitBridgeLogin`/`cancelBridgeLogin` wrappers + re-exports.
- `src/hooks/use-bridge-login.ts` — start/submit/cancel hook with per-run cancel guard + cleanup.
- `src/lib/bridges.ts` (+ test) — `BRIDGE_LOGIN_PHASE_LABEL`.
- `src/components/bridges/bridge-login-sheet.tsx` (+ test) — the Sheet stepper (all phases; step-keyed panels; anchored pattern validation).
- `src/components/bridges/bridge-card.tsx` (+ test) — Connect opens the login Sheet.

**Review findings breakdown:** 6 patches applied (2 medium — the unimplemented best-effort `/login/cancel` contract violation/session leak, and the frontend stale-field-state bug on sequential steps; 4 low — hook retry-race session leak, unbounded verbatim error body, unanchored pattern regex, mis-capitalized network name); 0 deferred; 0 intent gaps; 0 bad_spec loopbacks; 12 rejected as unreachable/speculative/cosmetic (see Review Triage Log).

**Verification (all re-run independently after the patches):**
- `bun run check:rust` — PASS (rustfmt clean + clippy `-D warnings`; no `async_fn_in_trait`, no `.unwrap()`).
- `bun run test:rust` — PASS (499 tests, up from 498).
- `bun run check` — PASS (Biome 198 files + tsc + 626 vitest tests + core-tauri-free guard); five ts-rs bindings emitted under `src/lib/ipc/gen/`.

**Follow-up review recommended:** `true`. The final pass changed backend session-lifecycle behavior (detached best-effort `login_cancel` on cancel and on graceful-shutdown drain, new `LoginSession` fields, a `Clone` transport) and a frontend concurrency guard — behavioral, moderately complex changes across the teardown path that benefit from an independent look.

### 2026-07-05 — Follow-up review pass

**Summary:** Ran the independent follow-up review the prior pass recommended (Blind Hunter + Edge Case Hunter, both at session model capability, no shared context). One high-severity security patch applied; 17 other findings verified and rejected as unreachable, by-design, or already-owned.

**Change applied:** `src-tauri/crates/keeper-core/src/account.rs` `start_bridge_login` now derives the provisioning-probe host from `client.homeserver()` (resolved C-S homeserver host + port) instead of the bare MXID `client.user_id().server_name()`. This prevents the account's live Matrix access token — sent as `Authorization: Bearer` on the probe request, even on a failing candidate — from reaching a `server_name` host that, under `.well-known` delegation, can differ from (and be operated by a different party than) the token-issuing homeserver. The host is the C-S endpoint the token already belongs to and where the bridgev2 provisioning API is co-located, so the fix is both more secure and more likely functionally correct; it mirrors the resolved-homeserver discipline already used in `auth.rs`. `resolve_candidates` remains a pure function of the passed server string (its unit test is unaffected).

**Verification (re-run after the patch):**
- `bun run check:rust` — PASS (rustfmt clean + clippy `-D warnings`).
- `bun run test:rust` — PASS (499 tests).
- `bun run check` — PASS (Biome 198 files + tsc + 626 vitest + core-tauri-free guard).

**Follow-up review recommended:** `false`. The pass made a single localized, well-understood host-source correction in one function, fully covered by the existing green gates and adding no new behavior surface — not enough to warrant another independent review.

**Residual risks (unchanged):** Live provisioning against a real bridge cannot be exercised unattended — the transport's impure HTTP shell (base-URL probe, long-poll, step POSTs, `login_cancel`) is covered only by the pure `resolve_candidates`/`extract_error_message`/`step_to_vm`/`qr_svg` unit tests and the scripted-`FakeTransport` driver tests; live base-URL correctness, long-poll timing, and the best-effort cancel round-trip are documented residual risks (as with 6.2's discovery shell). Base-URL resolution ships a single default candidate (`https://{server}/_matrix/provision`); deployments whose provisioning API lives elsewhere (separate appservice host, Beeper/hungryserv proxy path) will need an added candidate — the `provisioning.json` data file is the single point to extend. `cookies`/`webauthn` (browser/passkey) login methods remain out of scope by design (honest unsupported-method state). `BotDriver`, live health, and the Bridge Bot chat escape-hatch navigation are Stories 6.4/6.5.
