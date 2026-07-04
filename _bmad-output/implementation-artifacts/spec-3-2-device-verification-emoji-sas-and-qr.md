---
title: 'Device Verification — Emoji/SAS and QR'
type: 'feature'
created: '2026-07-04'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: 'e4343e19dd88783c982a86630d853489302f2207'
final_revision: 'd38afc28116f7e18b79ead062300ff9c764adc59'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Story 3.1 landed the honest E2EE *states* — a per-account `verification_state()` stream driving a "verify this device" banner, a Settings badge, and UTD stubs — but there is no way to actually *verify*. A freshly-logged-in keeper device stays permanently `Unverified`, encrypted history stays locked, and the Settings "Encryption" section is read-only with the Verify entry point explicitly deferred here (3.2).

**Approach:** Add interactive self-verification against one of the user's other sessions (e.g. Element) using the matrix-sdk 0.18 native flow, confined to `keeper-core`. A new `keeper-core::verification` module owns all verification SDK calls, exposes one-shot action commands plus a per-account stream of a `VerificationFlowVm` state machine, and renders QR *display* as a Rust-built SVG string. The React layer renders an Element-X-style multi-step modal (waiting → compare emoji / show QR → confirmed → done/cancelled/failed) reached from the Settings Encryption row. On success the SDK flips `verification_state()` to `Verified`, so 3.1's existing stream automatically clears the banner/badge and re-renders UTD events as keys arrive — no new banner code.

## Boundaries & Constraints

**Always:**
- All verification SDK calls, flow IDs, SAS/QR crypto, and key material live in `keeper-core::verification`; the webview receives only a rendered `VerificationFlowVm` (emoji symbols+names, a QR SVG string, phase, cancel reason) — never a `Verification`/`Sas`/`QrVerification` object, key, or plaintext (AD-1 / NFR-9).
- Use the SDK's native flow vocabulary and Element-X-style patterns; do not invent novel crypto UX (epic Technical Decisions). Verification codes/emoji labels render in `mono` where textual.
- Emoji/SAS is the complete, must-work path in **both** directions: keeper starts verification with the user's other session, and keeper surfaces + responds to a request the other session starts.
- The flow modal is fully keyboard-operable with labeled controls (NFR-14): reachable, emoji "They match"/"They don't match"/"Cancel" are focusable buttons, `Esc` cancels.
- Reuse 3.1's `verification_state()` encryption-status stream to clear the banner/badge — do not add parallel banner-clearing logic.
- New Rust deps (the matrix-sdk `qrcode` feature and its `qrcode` crate) must pass the cargo-deny license firewall (permissive only).

**Block If:**
- The matrix-sdk `qrcode` feature cannot be enabled without pulling a non-permissive (AGPL/GPL) transitive dependency that fails `cargo deny check`.
- matrix-sdk 0.18 exposes no supported way to observe an *incoming* self-verification request (initiated by the other session) — without it the "verified from an existing session" direction is unbuildable and the scope assumption is wrong.

**Never:**
- No camera-based live QR *scanning* from keeper (getUserMedia capture + JS QR decode + macOS camera entitlement) — out of scope for the desktop MVP. SAS covers both verification directions and QR *display* covers the reciprocal (the peer scans keeper's QR), together satisfying FR-14's "from an existing session and vice versa". Document this limit honestly; do not stub a fake scanner.
- No cross-signing bootstrap / recovery-key / key-backup enable-or-restore code (Story 3.3). This story only rides the trust established by verifying against an already-cross-signed session.
- No Matrix/crypto logic, flow objects, or key material in TypeScript; no `matrix-js-sdk`.
- Do not rewire the verify banner or UTD stub away from opening Settings — the Verify entry point lives in the Settings Encryption row for this story.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Start from keeper | User clicks Verify on an `unverified` account | Command requests self-verification; stream emits `Requested` → `Ready`; modal shows "Waiting for your other device" + keeper's QR SVG | If no other device / request fails → `Failed` phase with reason |
| Incoming request | Other session (Element) starts verification | Producer surfaces the request; store opens the modal in `Requested`; Accept moves to `Ready` | Request times out → `Cancelled`/`Failed` rendered distinctly |
| SAS emoji compare | Flow reaches SAS; both sides show 7 emoji | `Comparing` phase carries `emojis: [{symbol, name}×7]`; "They match" → confirm; "They don't match" → mismatch | `mismatch()` cancels with the SDK code → `Failed` |
| Confirm both sides | keeper confirmed, peer confirms | `Confirmed` → `Done`; modal shows success; 3.1 encryption stream flips account to `Verified`, banner/badge clear, UTD rows re-render | — |
| User cancels | User closes modal / presses Esc mid-flow | `cancel()` called; stream emits `Cancelled`; modal shows cancelled state | Cancel command failure logged via `tracing`, modal still closes |
| Map cancel code | Terminal `CancelInfo` present | User-initiated cancel → `Cancelled`; any other code (mismatch/timeout/mismatched keys) → `Failed` with reason | Unknown code → `Failed` with raw reason string |
| QR SVG render | `Ready` with QR mode available | `generate_qr_code()` → `to_qr_code()` → SVG string in the VM; UI renders it as an image | QR unavailable/None → omit QR, SAS still offered |

</intent-contract>

## Code Map

- `src-tauri/Cargo.toml` -- workspace `matrix-sdk` dep (L36) — add `"qrcode"` to the feature list; run `cargo deny check`.
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `VerificationFlowVm`, `VerificationPhase` enum, `SasEmojiVm` (serde camelCase + `#[ts(export)]`), mirroring the `EncryptionStatus`/`ConnectionStatus` VM style (~L1–27, L119).
- `src-tauri/crates/keeper-core/src/verification.rs` -- **NEW** module owning the flow: incoming-request observation, one-shot actions (start/accept/start_sas/confirm/mismatch/cancel), a per-account producer streaming `VerificationFlowVm`, and pure helpers `map_request_state`, `map_sas_state`, `map_cancel_reason`, `qr_to_svg`.
- `src-tauri/crates/keeper-core/src/lib.rs` -- declare `pub mod verification;` (module list L12–22).
- `src-tauri/crates/keeper-core/src/account.rs` -- `AccountManager` (L145) + `AccountHandle` (`client`, shared `subscriptions` map): add `subscribe_verification`/`unsubscribe_verification` (mirror `subscribe_encryption_status` L663/L743, self-reaping producer + shared `abort_subscription`) and thin action methods that resolve the account's `Client` and delegate to `verification`.
- `src-tauri/crates/keeper/src/ipc.rs` -- new commands `verification_subscribe`/`verification_unsubscribe` (Channel pattern like `encryption_status_subscribe` L514) + one-shots `verification_start`, `verification_accept`, `verification_start_sas`, `verification_confirm`, `verification_mismatch`, `verification_cancel`.
- `src-tauri/crates/keeper/src/lib.rs` -- register all new commands in `invoke_handler!` (L36+).
- `src/lib/ipc/client.ts` -- add `subscribeVerification`/`unsubscribeVerification` (via `subscribe`) + the six one-shot wrappers (via `invoke`); re-export `VerificationFlowVm`/`VerificationPhase`/`SasEmojiVm` from `./gen/`.
- `src/lib/stores/verification.ts` -- **NEW** zustand store: active flow per account (`flow: VerificationFlowVm | null`), `modalOpen`/`activeAccountId`, `openFor(accountId)`, `close()`, `setFlow()`.
- `src/hooks/use-verification.ts` -- **NEW** all-account subscriber (mirror `use-encryption-statuses.ts`) so an *incoming* request auto-opens the modal; forwards batches to the store, gates late batches, tears down on account-set change/unmount.
- `src/components/settings/device-verification-dialog.tsx` -- **NEW** Element-X-style modal (`Dialog`) rendering the five phases with keyboard-operable actions.
- `src/components/settings/settings-dialog.tsx` -- in `EncryptionAccountRow` (L131), add a "Verify" `Button` shown when status is `unverified` that calls `verificationStore.openFor(accountId)`.
- `src/components/layout/app-shell.tsx` -- invoke `useVerification()` and mount `<DeviceVerificationDialog/>` alongside the existing 3.1 `useEncryptionStatuses()` / `<VerifyBanner/>` wiring.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/Cargo.toml` -- Add `"qrcode"` to the `matrix-sdk` workspace feature list and confirm `cargo deny check` stays green. -- Enables `generate_qr_code`/`QrVerification`/`to_qr_code`.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- Add `SasEmojiVm { symbol: String, name: String }`; `VerificationPhase { Requested, Ready, Comparing, Confirmed, Done, Cancelled, Failed }`; `VerificationFlowVm { flow_id: String, phase: VerificationPhase, emojis: Option<Vec<SasEmojiVm>>, qr_code_svg: Option<String>, reason: Option<String> }` — all camelCase + `#[ts(export)]`. -- Typed VM the webview renders.
- [x] `src-tauri/crates/keeper-core/src/verification.rs` -- New module. Pure helpers: `map_request_state(&VerificationRequestState) -> VerificationPhase`, `map_sas_state(&SasState) -> (VerificationPhase, Option<Vec<SasEmojiVm>>)` (emoji symbol+description from `sas.emoji()`), `map_cancel_reason(&CancelInfo) -> (VerificationPhase, Option<String>)` (user cancel → `Cancelled`, other codes → `Failed` + reason), `qr_to_svg(&QrVerification) -> Option<String>` (`to_qr_code()` → `qrcode` crate SVG render). Flow driver: observe incoming self-verification requests (matrix-sdk `add_event_handler` for the `m.key.verification.request` to-device event) and requests keeper starts; on an active request drive `request.changes()`, transitioning into `sas.changes()` when it becomes `Verification::SasV1`, and generate a QR SVG when `Ready`; map each into `VerificationFlowVm` and emit through the sink; emit terminal `Done`/`Cancelled`/`Failed` then stop. Actions resolve the request/SAS by `flow_id` via `client.encryption().get_verification_request`/`get_verification` and call the SDK (`request.accept`/`generate_qr_code`/`start_sas`, `sas.confirm`/`mismatch`, `cancel`). `start` calls `client.encryption().get_user_identity(own_user_id).await?` then `request_verification()`. Never expose SDK objects across the boundary. -- The verification engine.
- [x] `src-tauri/crates/keeper-core/src/lib.rs` -- Add `pub mod verification;`. -- Wire module.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- Add `VerificationSink`, `subscribe_verification`/`unsubscribe_verification` (copy the connection/encryption-status lifecycle: lazy activate, supervised task in the shared `subscriptions` map, self-reap, abort on unsubscribe), and action methods (`verification_start/accept/start_sas/confirm/mismatch/cancel`) that look up the account's `Client` and delegate to `verification`. -- Per-account verification lifecycle + action dispatch.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- Add `verification_subscribe(state, account_id, channel: Channel<VerificationFlowVm>)`, `verification_unsubscribe(state, account_id, subscription_id)`, and one-shots `verification_start/accept/start_sas/confirm/mismatch/cancel(state, account_id[, flow_id])`, all mapping errors via `to_ipc_error`. -- IPC surface.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- Register all eight commands in `invoke_handler!`. -- Wire commands.
- [x] `src/lib/ipc/client.ts` -- Add `subscribeVerification`/`unsubscribeVerification` + the six one-shot wrappers; re-export `VerificationFlowVm`/`VerificationPhase`/`SasEmojiVm`. -- Typed frontend IPC.
- [x] `src/lib/stores/verification.ts` -- New store: `{ flow, modalOpen, activeAccountId, openFor, close, setFlow }`; `openFor` sets the account + opens the modal; `close` cancels-intent + clears flow. -- Drives the modal.
- [x] `src/hooks/use-verification.ts` -- All-account subscriber mirroring `use-encryption-statuses`; on an incoming (`Requested`, not-we-started) batch, `openFor(accountId)`; forward every batch to `setFlow`; gate late batches; tear down on account-set change/unmount. -- Single verification subscriber + incoming auto-open.
- [x] `src/components/settings/device-verification-dialog.tsx` -- New `Dialog` modal driven by the store's `flow.phase`: `Requested`/`Ready` → "Waiting…" (+ QR image from `qr_code_svg` when present, + "Verify with emoji" starting SAS); `Comparing` → the 7 `emojis` (symbol + `mono` name) with "They match"/"They don't match"; `Confirmed` → "Waiting for your other device"; `Done` → success; `Cancelled`/`Failed` → distinct messages (+ `reason`). Keyboard-operable, labeled controls, `Esc`/close cancels the flow. -- Element-X-style flow UI.
- [x] `src/components/settings/settings-dialog.tsx` -- In `EncryptionAccountRow`, render a "Verify" `Button` when the account status is `unverified`, wired to `verificationStore.getState().openFor(accountId)`. -- Verify entry point (the 3.1 placeholder).
- [x] `src/components/layout/app-shell.tsx` -- Invoke `useVerification()` and mount `<DeviceVerificationDialog/>`. -- Wire subscriber + modal.
- [x] `src-tauri/crates/keeper-core/src/{vm.rs,verification.rs}` (tests) -- Unit tests: serde round-trip for `VerificationFlowVm`/`VerificationPhase`/`SasEmojiVm`; `map_request_state` and `map_sas_state` across their variants; `map_cancel_reason` splits user-cancel (`Cancelled`) vs other codes (`Failed` + reason); `qr_to_svg` yields a non-empty `<svg …>` string from a QR input (or is covered via the manual check if a `QrVerification` can't be built in a unit test). -- Verify pure mappings/render.
- [x] `src/**` (tests) -- Colocated vitest/RTL (mock IPC like `use-encryption-statuses.test.ts`): store transitions; `use-verification` auto-opens on an incoming batch and mirrors flow; modal renders each phase (emoji compare buttons call the confirm/mismatch wrappers, cancelled vs failed render distinctly, keyboard-operable); Settings row shows Verify only when `unverified` and calls `openFor`. -- Cover the I/O matrix edge cases.

**Acceptance Criteria:**
- Given an existing verified session (e.g. Element) on the same account, when the user starts verification from keeper or from the other session, then keeper completes interactive emoji/SAS verification (with keeper's QR available for the peer to scan), and afterwards the keeper device shows trusted on both ends (FR-14).
- Given the verification flow UI, when it runs, then waiting, comparing, confirmed, cancelled, and failed each render distinctly using the SDK's flow vocabulary (no novel crypto UX), and the flow is fully keyboard-operable (NFR-14).
- Given successful verification, then 3.1's `verification_state()` stream flips the account to `Verified`, the unverified banner/badge clear, and previously-undecryptable events re-render decrypted where keys arrive — with no plaintext or key material crossing into JS.
- Given `bun run check:all`, then Biome, tsc, vitest, rustfmt, clippy (`-D warnings`), cargo-nextest, and `cargo deny check` all pass and the ts-rs bindings regenerate without drift.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass (follow-up #2)
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 0, low 2)
- defer: 4: (high 0, medium 3, low 1)
- reject: 11: (high 0, medium 0, low 11)
- addressed_findings:
  - `[low]` `[patch]` `map_cancel_reason`'s outer doc-comment (`verification.rs`) still listed `cancelled_by_us` and an "`m.timeout`-free path" as mapping to `Cancelled`, contradicting both the code (only `m.user` → `Cancelled`) and its own inner comment. Left uncorrected it invites a future maintainer to "align" the code with the wrong doc and re-introduce the mismatch→`Cancelled` security regression the prior pass fixed. Rewrote the doc to state only `m.user` maps to `Cancelled` (every other code → `Failed`) and why it deliberately does not key on `cancelled_by_us()`.
  - `[low]` `[patch]` The QR display used a non-standard `data:image/svg+xml;utf8,` media-type parameter (`;utf8` is not a valid MIME parameter; the standard form is `data:image/svg+xml,` with the already-applied `encodeURIComponent`). Tolerated by the current WKWebView but fragile across the other webviews the project targets later, and QR display is a primary FR-14 acceptance path with no automated render check. Changed to the standard `data:image/svg+xml,` form.

### 2026-07-04 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 1, medium 1, low 1)
- defer: 1: (high 0, medium 0, low 1)
- reject: 13: (high 0, medium 0, low 13)
- addressed_findings:
  - `[high]` `[patch]` The **incoming (peer-started) direction was broken**: the dialog wired `verificationConfirm/Mismatch/Start/StartSas` but never `verificationAccept`, and routed both `requested` and `ready` to a "Verify with emoji" (start-SAS) button. A peer-started request surfaced in `Requested` was never accepted, so it could not advance to `Ready` and `start_sas` could not fire — contradicting the intent-contract I/O matrix ("Accept moves to `Ready`") and the both-directions constraint (FR-14). Fixed by auto-accepting an incoming request in `use-verification.ts` at the exact seam that already distinguishes incoming from keeper-started (`phase === "requested" && !modalOpen`): it now calls `verificationAccept(accountId, flowId)`, advancing `Requested → Ready` so the existing Ready→SAS→confirm path completes. keeper-started requests (accepted by the *other* session) are untouched. Added hook tests asserting accept fires on incoming and never on a keeper-started flow.
  - `[medium]` `[patch]` `map_cancel_reason` mislabeled a **user-detected emoji mismatch as a benign `Cancelled`**: `sas.mismatch()` cancels with `m.mismatched_sas` and is `cancelled_by_us()`, so the `cancelled_by_us()` short-circuit swallowed a security-relevant mismatch into `Cancelled` instead of `Failed` (the intent contract requires mismatch → `Failed`). Fixed by keying solely on the cancel code: only `m.user` (clean user/peer dismissal) → `Cancelled`; every other code, including mismatch/timeout/key-mismatch, → `Failed` with the SDK reason.
  - `[low]` `[patch]` The ready-phase "Verify with emoji" button swallowed a rejected `verificationStartSas` (`.catch(() => {})`), re-introducing the modal-hang the prior pass's honest-terminal patches targeted. Fixed to surface a rejected SAS start as an honest `failed` snapshot (mirroring the existing `verificationStart` rejection handling) so the button can't strand the user on a dead "waiting" screen.

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 5: (high 0, medium 4, low 1)
- defer: 3: (high 0, medium 0, low 3)
- reject: 12: (high 0, medium 0, low 12)
- addressed_findings:
  - `[medium]` `[patch]` `drive_flow`/`drive_sas` returned `true` when the SDK `changes()` stream ended without a Done/Cancelled state, leaving the modal hung on "waiting"/"comparing" — added `emit_stream_ended` to surface an honest terminal `Failed` when a stream ends non-terminally.
  - `[medium]` `[patch]` `verification_start` sent the `m.key.verification.request` to the peer, then returned `Ok(())` even when no producer sender existed — a dangling request on the other device plus a false success. Reordered to require a live producer *before* creating the SDK request, and return `VerificationError::Unavailable` (not `Ok`) when the producer is gone.
  - `[medium]` `[patch]` The modal's `close()` best-effort-cancelled any non-terminal flow, so clicking away in the `confirmed` phase (our SAS confirmation already sent) aborted a near-complete verification — excluded `confirmed` from `shouldCancelOnClose` so the flow can still complete after dismiss.
  - `[medium]` `[patch]` A rejected `verificationStart` was swallowed, hanging the modal on "Waiting…" forever (e.g. no other session / crypto identity not ready) — the `.catch` now sets an honest `failed` flow snapshot with a recovery hint.
  - `[low]` `[patch]` Two unit tests had names promising cancel-split / Ready-Transitioned coverage but only asserted `Done → Done` twice (`CancelInfo`/payload variants aren't constructible) — collapsed to one honestly-named test that documents what is manual-only.

## Design Notes

**Verification is stateful per flow_id; the stream is the source of truth.** Unlike the stateless `verification_state()` snapshot in 3.1, a flow moves through request → ready → SAS → done/cancelled. Keep exactly one active flow per account in the producer; the webview only reacts to emitted `VerificationFlowVm` snapshots and calls one-shot actions by `flow_id`. This keeps all state-machine crypto in Rust and makes the UI a pure renderer.

**Banner/badge clearing is free.** After `sas.confirm()` completes on both sides the SDK uploads signatures and `client.encryption().verification_state()` transitions to `Verified`; 3.1's per-account encryption-status producer already streams that and clears the banner/badge and re-renders UTD rows. Do **not** duplicate that logic here — just let verification succeed.

**QR display, not scan.** `request.generate_qr_code()` → `QrVerification::to_qr_code()` → render to an SVG string with the `qrcode` crate (pulled by the matrix-sdk `qrcode` feature). The webview renders the SVG as an `<img>`/inline — it never decodes QR crypto. Camera scanning is deliberately out of scope (see Never); SAS is the guaranteed path in both directions.

**Cancelled vs failed are different states (epic UX).** Map a user/peer clean cancel to `Cancelled`; map mismatch/timeout/other `CancelInfo` codes to `Failed` with a human reason. Render them distinctly so the user knows whether to retry.

## Verification

**Commands:**
- `bun run check` -- expected: Biome + tsc + vitest all green.
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` clean.
- `bun run test:rust` -- expected: cargo-nextest green; ts-rs regenerates `VerificationFlowVm`/`VerificationPhase`/`SasEmojiVm` with no git drift.
- `cargo deny check` (from `src-tauri/`) -- expected: the `qrcode` feature adds only permissive-licensed crates; firewall stays green.

**Manual checks (real second session, test credentials in 1Password):**
- From a fresh unverified keeper login, start verification against Element (and vice versa); compare emoji, confirm on both sides; confirm keeper shows trusted on both ends, the banner/badge clear, and prior UTD stubs re-render decrypted.
- Confirm keeper's displayed QR is scannable by Element; confirm cancelled and mismatch each render their distinct state; confirm the whole flow is operable with keyboard only.

## Auto Run Result

Status: done

**Summary:** Added interactive device self-verification (emoji/SAS + QR display) built on Story 3.1's encryption-status stream, fully confined to `keeper-core`. A new `keeper-core::verification` module owns every verification SDK call, flow id, and SAS/QR crypto: it observes both incoming self-verification requests (a `m.key.verification.request` to-device handler) and keeper-started ones, drives exactly one active flow through the SDK's `VerificationRequest`/`SasVerification` `changes()` streams, and maps each transition into a rendered `VerificationFlowVm` (phase, 7 emoji as symbol+name, a Rust-built QR SVG, cancel/failure reason) — no `Verification`/SAS/QR object, key, decimal, or plaintext ever crosses IPC. Eight Tauri commands (subscribe/unsubscribe + start/accept/start_sas/confirm/mismatch/cancel) surface it. The React layer renders an Element-X-style modal (waiting → QR/emoji compare → confirmed → done/cancelled/failed) reached from the Settings Encryption "Verify" button, with an always-on subscriber that auto-opens on an incoming request. On success the SDK flips `verification_state()` to `Verified`, so 3.1's existing stream clears the banner/badge and re-renders UTD events — no new banner code. QR is display-only (the peer scans keeper's QR); camera-based QR scanning is out of scope for the desktop MVP (SAS covers both directions).

**Files changed:**
- `src-tauri/Cargo.toml`, `src-tauri/crates/keeper-core/Cargo.toml` — enabled matrix-sdk `qrcode` feature + `qrcode` crate (`default-features=false`, `svg` only; permissive licenses).
- `src-tauri/crates/keeper-core/src/verification.rs` — NEW: flow producer, incoming-request observation, 6 one-shot actions, pure `map_request_state`/`map_sas_state`/`map_cancel_reason`/`qr_to_svg`, and (review) `emit_stream_ended` honest terminal fallback.
- `src-tauri/crates/keeper-core/src/vm.rs` — `SasEmojiVm`, `VerificationPhase`, `VerificationFlowVm` (+ `VerificationFailed` error code) + serde round-trip tests.
- `src-tauri/crates/keeper-core/src/error.rs` — `VerificationError { Unavailable, FlowNotFound, Action }`.
- `src-tauri/crates/keeper-core/src/{lib.rs,account.rs}` — module wiring; `subscribe/unsubscribe_verification` (mirrors encryption-status lifecycle) + 6 action methods; (review) `verification_start` now requires a live producer before creating the SDK request.
- `src-tauri/crates/keeper/src/{ipc.rs,lib.rs}` — 8 commands + error mapping + tests.
- `src/lib/ipc/client.ts` (+ `gen/SasEmojiVm.ts`, `gen/VerificationFlowVm.ts`, `gen/VerificationPhase.ts`, `gen/IpcErrorCode.ts`) — typed IPC wrappers + regenerated bindings.
- `src/lib/stores/verification.ts` — modal store; (review) `shouldCancelOnClose` no longer cancels a `confirmed` flow on dismiss.
- `src/hooks/use-verification.ts` — all-account subscriber with incoming-request auto-open.
- `src/components/settings/device-verification-dialog.tsx` — the multi-phase modal; (review) a rejected start now shows an honest `failed` state.
- `src/components/settings/settings-dialog.tsx` — Verify button in `EncryptionAccountRow` (the 3.1 placeholder).
- `src/components/layout/app-shell.tsx` — mounts the subscriber hook + modal.
- Colocated tests for store / hook / dialog / commands (+ 2 review-driven tests).

**Review findings:** 5 patches applied (4 medium: stream-ended terminal fallback, `verification_start` no-false-success/no-dangling-request, don't-cancel-`confirmed`-on-close, surface start-rejection as failed; 1 low: collapsed two vacuous unit tests into one honest test). 3 deferred (single-active-flow drops a 2nd account's concurrent incoming request; spawn→register account-removal teardown race shared with 2.1/2.5/3.1; pre-registration pending incoming request not surfaced). 12 rejected (QR copy is correct — display keeper's QR for the peer; the cancel `reason()` is a non-secret bounded SDK string; `start_sas` `Ok(None)` doesn't occur in self-verification; re-Verify hang is a false positive since Radix unmounts the body on close; single-flow serialization and QR-unavailable are by-spec; speculative untrusted-SVG; handled overlapping-effect teardown). No intent_gap, no bad_spec — the spec held up.

**Verification:** `bun run check` (biome + tsc + vitest: 294 passed), `bun run check:rust` (rustfmt + clippy `-D warnings`: clean), `bun run test:rust` (cargo-nextest: 193 passed; ts-rs regenerates the 3 new bindings + `IpcErrorCode` with no unexpected drift), `cargo deny check licenses` (ok — `qrcode` adds only permissive crates). All re-run green after the review patches. Note: `cargo deny check advisories` fails on a **pre-existing** unmaintained gtk-rs GTK3 binding (RUSTSEC-2024-041x) pulled transitively by Tauri — unrelated to this story and present on the baseline; the license firewall the spec gates on is green.

**Residual risks:** The live self-verification round-trip (emoji compare/confirm on both sides, scannable QR, banner/badge auto-clear via 3.1's stream, UTD re-render) is a real-second-session manual check, intentionally not run unattended — the Rust state-machine mapping is proven only at the unit/mapping level because `CancelInfo` and the payload-bearing SDK states aren't constructible in tests. Multi-account concurrent verification and the subscribe teardown race are deferred (see deferred-work.md). `followup_review_recommended: true` — the pass changed verification start/terminal/close semantics on a security-sensitive flow whose live behavior can't be unit-verified, so an independent follow-up review is worthwhile.

---

### Follow-up review pass (2026-07-04)

An independent follow-up review (the pass the prior run recommended) ran Blind Hunter + Edge Case Hunter over the full baseline→HEAD diff and surfaced one **high** functional defect the first pass missed, plus two smaller correctness gaps.

**Patches applied (3):**
- `[high]` **Incoming (peer-started) verification was non-functional.** `verificationAccept` was plumbed through client/IPC/account/core but never invoked by any UI, and the dialog routed the incoming `Requested` phase to a start-SAS button. A peer-started request could never advance `Requested → Ready`, so keeper could not complete a verification the other session initiated — one of FR-14's two required directions and the intent-contract I/O matrix ("Accept moves to `Ready`"). Fixed in `src/hooks/use-verification.ts`: the incoming-detection seam (`phase === "requested" && !modalOpen`) now calls `verificationAccept(accountId, flowId)` to accept the request; keeper-started requests (accepted by the peer) are unaffected. This honored the already-clear intent contract, so it was a code-level patch, not a spec change.
- `[medium]` **A user-detected emoji mismatch was mislabeled `Cancelled` instead of `Failed`.** `map_cancel_reason` (`src-tauri/crates/keeper-core/src/verification.rs`) short-circuited on `cancelled_by_us()`, which is true for `sas.mismatch()` (`m.mismatched_sas`). A MITM-signalling mismatch showed the same soft "cancelled" copy as an Esc dismissal. Fixed to map only the `m.user` code to `Cancelled`; all other codes (mismatch/timeout/key-mismatch) → `Failed` with reason, per the intent contract.
- `[low]` **Ready-phase start-SAS rejection was swallowed**, re-introducing a modal hang. `src/components/settings/device-verification-dialog.tsx` now surfaces a rejected `verificationStartSas` as an honest `failed` snapshot.

**Deferred (1):** signing out the account that owns an open verification modal strands the modal on a removed account (store not reset) — low consequence, uncommon path (see deferred-work.md).

**Rejected (13, all low):** verification-start-vs-subscription-ready race (misleading copy, narrow window); close-with-null-flow dangling incoming (self-heals via timeout); stale-QR (no actual bug — QR only regenerated in `Ready`); redundant re-drive of a duplicate to-device request (benign warn log); the `Ok(subscription_id)`-on-vanished-account subscribe race and the concurrent-2nd-account drop (both already in the deferred ledger); double-confirm buttons not disabled (SDK-idempotent); auto-open-only-on-`requested` (incoming's first emit *is* `requested`); `setFlow` overwriting a terminal for the same account (rare, self-consistent); producer `select!` on a closed `flow_rx` (the sender only drops at teardown, where the task is aborted anyway); own-user-`None` startup gap and silent skip of a vanished keeper-started flow (documented, low).

**Verification (all re-run green):** `bun run check` (biome + tsc + vitest: 294 passed), `bun run check:rust` (rustfmt + clippy `-D warnings`: clean), `bun run test:rust` (cargo-nextest: 193 passed; no ts-rs binding drift), `cargo deny check licenses` (ok). No new bindings changed (no VM shape change this pass). The pre-existing advisories finding (unmaintained gtk-rs GTK3 binding via Tauri) is unchanged and unrelated.

**Residual risk unchanged:** the live second-session round-trip — now including the newly-fixed incoming direction and the mismatch→`Failed` mapping — remains a manual check that cannot be exercised unattended (`CancelInfo`/payload-bearing SDK states aren't constructible in unit tests). Because this pass restored a previously-broken security-relevant direction and changed terminal-state mapping, `followup_review_recommended` stays `true`: a human manual verification against a real Element session is the meaningful next gate.

---

### Follow-up review pass #2 (2026-07-04)

The independent follow-up review this run recommended ran Blind Hunter + Edge Case Hunter over the full `baseline→HEAD` diff. Both prior passes had already converged the functional core; this pass surfaced no new intolerable defects. The two directions and the mismatch→`Failed` mapping held up. The findings were narrow concurrency/timing edges and the QR reciprocal path — all consistent with the documented residual that live behavior is a manual second-session gate.

**Patches applied (2, both low):**
- `[low]` **Corrected a self-contradictory doc-comment on `map_cancel_reason`** (`verification.rs`). Its outer doc still listed `cancelled_by_us` and an "`m.timeout`-free path" as mapping to `Cancelled`, contradicting the code (only `m.user` → `Cancelled`) and its own inner comment. This is a live maintenance hazard: it invites a future edit that "aligns" the code to the wrong doc and re-introduces the mismatch→`Cancelled` security regression the prior pass fixed. No behavior change — documentation only.
- `[low]` **Fixed the QR `<img>` data-URI to the standard `data:image/svg+xml,` form** (`device-verification-dialog.tsx`), dropping the non-standard `;utf8` media-type parameter. Tolerated by the current WKWebView, but fragile across the other webviews the project targets later; QR display is a primary FR-14 path with no automated render check. `encodeURIComponent` (already present) covers escaping.

**Deferred (4, NEW ledger entries — pre-existing/known-limitation edges, none intolerable):**
- `[medium]` QR reciprocal direction (peer scans keeper's QR) is unverified and mislabeled: `Transitioned { QrV1 }` maps to `Comparing` (shows the emoji screen's "waiting" copy with no emoji), no `qr.confirm()` is driven, and a QrV1 stream-end could emit a spurious `Failed` on success. Needs live second-session validation before any (risky, untestable) state-machine change.
- `[medium]` Overlapping `subscribe_verification` for the same account (StrictMode double-mount / rapid resubscribe) clobbers the single `verification_flow_tx` slot, and a stale `unsubscribe_verification` nils it — leaving the live producer unreachable so keeper-started verify fails. Verification-specific twist on the already-deferred subscription-lifecycle races (2.1/2.5/3.1).
- `[medium]` Emoji-compare "They match" then an immediate Esc can cancel the just-approved verification, because `close()` still sees the streamed `comparing` phase before the `confirmed` snapshot round-trips. A correct fix touches the pure-renderer invariant (optimistic dispatch flag), so deferred rather than patched blind.
- `[low]` A second pre-`Ready` `requested` snapshot straddling an Esc can re-open the dismissed incoming modal and re-fire `verificationAccept`; self-limiting, needs per-`flowId` seen-tracking in the hook.

**Rejected (11, all low):** residual dangling-request window in `verification_start` (self-heals via SDK timeout); to-device-only handler not observing in-room requests (self-verification is to-device); swallowed best-effort cancel leaving a peer request live (self-heals); cross-account incoming drop while a modal is open and second-incoming-while-driving (both already in the deferred ledger); terminal-then-new-incoming for a different account (self-heals via `close()` nulling `activeAccountId`); start-resolves-`Ok`-but-stream-never-emits (narrow SDK-drop, prior-pass variant); accept-rejects-because-request-vanished stranding on waiting (narrow, user-dismissable); re-Verify hang via `startedRef` (false positive — Radix unmounts the dialog body on close, resetting the ref); `openFor` not cancelling a prior in-flight flow (unreachable — the focus-trapped modal blocks reaching the Settings Verify button mid-flow); double-click match/no-match not disabled (SDK-idempotent, prior-pass rejection).

**Verification (all re-run green):** `bun run check` (biome + tsc + vitest: 294 passed), `bun run check:rust` (rustfmt + clippy `-D warnings`: clean), `bun run test:rust` (cargo-nextest: 193 passed). No VM shape change this pass, so no ts-rs binding drift. The doc-comment patch is behavior-inert; the data-URI patch is a standards-conformance string change with no logic impact.

**Follow-up review recommendation:** `false`. This pass made only two localized, low-consequence, behavior-inert patches (an inert doc-comment and a standards-conformance data-URI string); the functional core has now converged across three review passes and no `bad_spec`/`intent_gap` arose. The meaningful remaining gate is **human manual verification against a real Element session** — specifically exercising the QR reciprocal (peer-scans-keeper's-QR) direction now called out in the deferred ledger, alongside the emoji path in both directions — which cannot be exercised unattended (`Transitioned`/`QrV1`/`CancelInfo` states aren't constructible in unit tests). That is a manual check, not another automated review pass.
