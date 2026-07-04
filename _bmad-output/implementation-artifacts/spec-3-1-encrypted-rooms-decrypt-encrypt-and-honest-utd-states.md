---
title: 'Encrypted Rooms — Decrypt, Encrypt, and Honest UTD States'
type: 'feature'
created: '2026-07-04'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '6a0fb044c110672a2385dd5f4f20fdcf9ec58521'
final_revision: '1a9057bc9bdef59da061d70d83434f26b8ffdf41'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** keeper already compiles matrix-sdk with `e2e-encryption`, so encrypted Rooms transparently decrypt/encrypt through the existing send + timeline paths — but events that *can't* be decrypted yet silently collapse to a blank `TimelineItemVm::Other` row, and a freshly-logged-in unverified device gives the user no honest signal that encrypted history is locked. E2EE is invisible when healthy but also invisible when broken.

**Approach:** Surface the honest states without touching the crypto that already works. Add a `TimelineItemVm::Utd` variant so undecryptable events render an explicit stub instead of nothing, and add a per-account encryption-status stream (mapped from the SDK's `verification_state()`) that drives a dismissible global "verify this device" banner, a collapsed persistent Settings badge, and a read-only Encryption section in Settings. Interactive verification (SAS/QR) is Story 3.2; this story only makes the states visible and routes affordances toward Settings.

## Boundaries & Constraints

**Always:**
- All crypto, keys, and plaintext stay in `keeper-core`; only rendered view models cross IPC. The UTD stub carries no ciphertext/keys/session material — only stable key, sender, timestamp (NFR-9, AD-1).
- New IPC types are `*Vm`/`*Batch` in `keeper-core::vm`, deriving serde (camelCase) + `#[ts(export)]`; commands register in `keeper/src/lib.rs` `invoke_handler!` (AD-7).
- The encryption-status stream mirrors the connection-status lifecycle exactly (lazy activation, supervised producer, self-reaping subscription, per-subscription abort on unsubscribe).
- Rust rules hold: no `.unwrap()`/`.expect()` in production paths, `?` + thiserror, `tracing` for logs, `cargo clippy --all-targets -- -D warnings` clean.
- Timeline diff indices stay aligned: UTD maps to exactly one VM per SDK item, same as every other item.
- The banner shows only on `Unverified`, never on `Unknown` (no nag before crypto has synced), and clears (banner + badge) on `Verified`.

**Block If:**
- Transparent decrypt/encrypt does NOT actually work in an encrypted Room under the current default client settings — i.e. satisfying AC-1 would require bootstrapping cross-signing or enabling key backup (Story 3.2 / 3.3 scope). Do not pull that work forward unattended.
- `client.encryption().verification_state()` does not behave as a per-account `Subscriber<VerificationState>` yielding `Unknown`/`Verified`/`Unverified`.

**Never:**
- No interactive device verification (emoji/SAS, QR) — Story 3.2. No key-backup enable/restore or recovery-key UI — Story 3.3. No cross-signing or backup bootstrap / `EncryptionSettings` changes.
- No Matrix/crypto logic in TypeScript; no keys or plaintext through IPC.
- No cross-restart persistence of banner dismissal (session-scoped only).
- No media/replies/edits/reactions in encrypted rooms (Stories 3.4–3.7).
- No live `Verify` action button in the Settings Encryption section (3.2 adds it); 3.1's section is read-only honest state.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Decrypt healthy | Encrypted Room, room keys present | Incoming event decrypts to `TimelineItemVm::Message`; outgoing send is encrypted and readable by another Matrix client | No error expected |
| UTD event | `MsgLikeKind::UnableToDecrypt(_)` timeline item | Maps to `TimelineItemVm::Utd { key, sender, sender_display_name, timestamp }`; timeline renders the honest stub, never blank | No error; it is a state, not an error |
| Keys arrive later | SDK retries decryption on a prior UTD item | SDK emits `VectorDiff::Set` → forwarded `TimelineOp::Set` → item re-maps UTD→Message; stub is replaced by decrypted text | No error expected |
| Fresh unverified device | account `verification_state()` = `Unverified` | Global banner "Verify this device to read encrypted history" appears | Subscribe failure leaves status unknown → no banner (non-critical) |
| Crypto not synced yet | `verification_state()` = `Unknown` | No banner, no badge (avoid false nag) | No error expected |
| Banner dismissed | user dismisses while still `Unverified` | Banner hidden this session; a persistent badge appears on the Settings entry point | No error expected |
| Device becomes verified | status transitions to `Verified` | Banner and Settings badge both clear | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- `TimelineItemVm` enum (add `Utd`), `ConnectionStatus`/`ConnectionStatusBatch` (template for new `EncryptionStatus`/`EncryptionStatusBatch`) at ~L119/L263.
- `src-tauri/crates/keeper-core/src/timeline.rs` -- `item_to_vm` (L100) maps SDK items; add UTD branch on `MsgLikeKind::UnableToDecrypt`.
- `src-tauri/crates/keeper-core/src/account.rs` -- `subscribe_connection_status`/`run_connection_producer`/`map_connection_status`/`ConnectionSink` (L69, L559, L1005, L1035) are the exact template for the encryption-status equivalents; client build at L851.
- `src-tauri/crates/keeper/src/ipc.rs` -- `connection_status_subscribe`/`_unsubscribe` (L476) template for new commands.
- `src-tauri/crates/keeper/src/lib.rs` -- `invoke_handler!` list (L36+); register new commands.
- `src/lib/ipc/client.ts` -- `subscribeConnectionStatus`/`unsubscribeConnectionStatus` (L287) template + VM re-exports.
- `src/hooks/use-account-statuses.ts` -- exact template for the all-account encryption-status subscriber.
- `src/lib/stores/account-status.ts` -- per-account map store template.
- `src/components/chat/message-bubble.tsx` -- row component pattern for the new UTD stub.
- `src/components/layout/conversation-pane.tsx` -- `toRenderedMessages` (L54) + render loop (L225); render UTD rows (break grouping like `other`, but visible).
- `src/components/layout/app-shell.tsx` -- shell flex column (L41); mount banner + invoke the subscriber hook.
- `src/components/layout/account-footer.tsx` -- Settings opens via local `useState` (L19); host of the persistent verify badge.
- `src/components/settings/settings-dialog.tsx` -- add read-only Encryption section.
- `src/components/ui/{alert,badge,button}.tsx` -- reuse for banner/stub/badge.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- Add `TimelineItemVm::Utd { key, sender, sender_display_name: Option<String>, timestamp: i64 }`; add `EncryptionStatus { Unknown, Verified, Unverified }` and `EncryptionStatusBatch { status }` (mirror `ConnectionStatus`/`ConnectionStatusBatch`, camelCase, `#[ts(export)]`). -- Honest UTD payload + banner signal types.
- [x] `src-tauri/crates/keeper-core/src/timeline.rs` -- In `item_to_vm`, before the message fallthrough, match `TimelineItemContent::MsgLike(m)` where `m.kind` is `MsgLikeKind::UnableToDecrypt(_)` and emit `TimelineItemVm::Utd` (key/sender/sender_display_name/timestamp from the event item); all other non-message kinds still map to `Other`. -- UTD renders a stub, never blank.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- Add `EncryptionSink`, `subscribe_encryption_status`, `unsubscribe_encryption_status`, `run_encryption_status_producer` (over `client.encryption().verification_state()`: emit `map_encryption_status(subscriber.get())` first, then dedup on `subscriber.next().await`), and pure `map_encryption_status(&VerificationState) -> EncryptionStatus`. Copy the connection-status lifecycle verbatim (lazy activate, supervised task, self-reap, abort on unsubscribe). -- Live per-account device-verification signal.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- Add `encryption_status_subscribe(state, account_id, channel: Channel<EncryptionStatusBatch>)` and `encryption_status_unsubscribe(state, account_id, subscription_id)`, mirroring the connection-status commands. -- IPC surface.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- Register both commands in `invoke_handler!`. -- Wire commands.
- [x] `src/lib/ipc/client.ts` -- Add `subscribeEncryptionStatus`/`unsubscribeEncryptionStatus` and re-export `EncryptionStatus`/`EncryptionStatusBatch`. -- Typed frontend IPC.
- [x] `src/lib/stores/encryption-status.ts` -- New zustand store: per-account `statuses: Record<string, EncryptionStatus>` with `setStatus`/`removeAccount`; session-scoped `bannerDismissed: boolean` + `dismissBanner()`; selector hooks `useAnyUnverified()`, `useShowVerifyBanner()` (anyUnverified && !dismissed), `useShowVerifyBadge()` (anyUnverified && dismissed). -- Mirrors the Rust stream; drives banner/badge.
- [x] `src/hooks/use-encryption-statuses.ts` -- All-account subscriber mirroring `use-account-statuses` (subscribe every signed-in account, gate late batches, swallow subscribe failures, tear down on account-set change/unmount). -- Single encryption-status subscriber.
- [x] `src/lib/stores/settings-ui.ts` -- New tiny store `{ settingsOpen, setSettingsOpen }` so the banner/UTD action can open Settings. -- Shared Settings open-state.
- [x] `src/components/chat/utd-stub.tsx` -- New row: honest text "Can't decrypt yet — verify this device or restore key backup" (`Alert` + `AlertAction` "Verify") wired to `setSettingsOpen(true)`. -- The never-blank UTD stub.
- [x] `src/components/layout/verify-banner.tsx` -- New global banner "Verify this device to read encrypted history" (`Alert`, `role="status"`), CTA opens Settings, dismiss button calls `dismissBanner()`. Renders only when `useShowVerifyBanner()`. -- Honest unverified-device banner.
- [x] `src/components/layout/conversation-pane.tsx` -- Render `kind === "utd"` items as `<UtdStub>` in the timeline `<ol>`; update `toRenderedMessages` so UTD items break same-sender runs but are emitted (not skipped like `other`). -- Show UTD stubs inline.
- [x] `src/components/layout/app-shell.tsx` -- Invoke `useEncryptionStatuses()`; mount `<VerifyBanner>` between the titlebar drag band and the panes row. -- Wire subscriber + banner.
- [x] `src/components/layout/account-footer.tsx` -- Replace local settings-open `useState` with `settingsUiStore`; render a small dot `Badge` on the Settings affordance when `useShowVerifyBadge()`. -- Persistent Settings badge; shared open-state.
- [x] `src/components/settings/settings-dialog.tsx` -- Add a read-only "Encryption" section listing each signed-in account's device state (Verified / Not verified) from the encryption-status store, with copy that verifying unlocks encrypted history (no interactive Verify button — 3.2). -- Honest CTA destination.
- [x] `src-tauri/crates/keeper-core/src/{vm.rs,account.rs}` (tests) -- Unit tests: serde round-trip for `EncryptionStatus`, `EncryptionStatusBatch`, `TimelineItemVm::Utd`; `map_encryption_status` for all three `VerificationState` values. If a UTD `TimelineItem` can't be constructed in a unit test, cover the UTD branch via the manual encrypted-room check instead. -- Verify pure mappings.
- [x] `src/**` (tests) -- Colocated vitest/RTL tests: UTD stub renders honest text + working action; banner visible when unverified & not dismissed, dismiss→badge, hidden when verified/unknown; store mirroring + selectors; conversation-pane renders a UTD row from a streamed `utd` item (mock IPC like `conversation-pane.test.tsx`). -- Cover I/O-matrix edge cases.

**Acceptance Criteria:**
- Given an encrypted Room with keys available, when a message arrives and when the user sends, then keeper decrypts incoming and encrypts outgoing transparently in `keeper-core`, another Matrix client in the Room can read keeper's message, and no plaintext or key material crosses into JS.
- Given an event that cannot be decrypted, when the timeline renders it, then an explicit stub "Can't decrypt yet — verify this device or restore key backup" with an inline action appears instead of a blank row.
- Given a freshly-logged-in unverified device, when the app opens, then a global banner "Verify this device to read encrypted history" appears; dismissing it collapses it to a persistent Settings badge (not gone), and the badge/banner both clear once the device is verified.
- Given `bun run check:all`, then Biome, tsc, vitest, rustfmt, clippy (`-D warnings`), and cargo-nextest all pass and the ts-rs bindings regenerate without drift.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 5: (high 0, medium 3, low 2)
- defer: 1: (high 0, medium 0, low 1)
- reject: 11: (high 0, medium 0, low 11)
- addressed_findings:
  - `[medium]` `[patch]` UTD stub rendered as an assertive `role="alert"` live region (via the base `Alert`), so a backfilled encrypted room would fire N interrupting screen-reader announcements — overrode to `role="status"` (polite), matching `VerifyBanner`.
  - `[medium]` `[patch]` The persistent verify badge was driven by the global `useShowVerifyBadge()` yet rendered in every per-account row menu, so a *verified* account's row falsely showed a "Verify this device" badge — added account-scoped `useShowVerifyBadgeForAccount(accountId)` and switched the row menu to it; also made the decorative dot `aria-hidden` (removed the misleading actionable-sounding `aria-label` on a non-interactive span).
  - `[medium]` `[patch]` The Settings Encryption row collapsed `unknown`/pending status to a red "Not verified", contradicting the banner's "never nag on Unknown" rule and false-alarming during initial crypto sync — split into three honest states (Verified / Not verified / "Checking…") with the attention tone reserved for explicit `unverified`.
  - `[low]` `[patch]` Banner dismissal was a single global session flag, so dismissing for one account silently pre-hid the nag for a later or re-regressed unverified device — `setStatus` now clears `bannerDismissed` when an account *newly* enters `unverified`, re-surfacing the banner for a genuinely new verification need (deduped same-status batches do not re-nag).
  - `[low]` `[patch]` The load-bearing "stub self-heals when keys arrive" claim was asserted only in prose — added a conversation-pane test driving a `Set` diff that replaces a `utd` item with a decrypted `message` at the same key and asserts the stub disappears.

## Design Notes

**AC-1 needs no new crypto wiring.** `keeper-core` already inherits `matrix-sdk` with `e2e-encryption` + `sqlite`, and the SDK's crypto store lives in the per-account `sqlite_store`. The `SyncService` processes to-device room keys and the OlmMachine automatically; `Timeline::send` (the AD-13 single gate) auto-encrypts in encrypted Rooms, and `room.timeline()` auto-decrypts. So transparent decrypt/encrypt already works — do NOT add `EncryptionSettings`, cross-signing, or backup bootstrap (that steps on Stories 3.2/3.3). AC-1's deliverable is the manual encrypted-room round-trip verification plus the honest UTD state for the failure case.

**Key-arrival re-render is automatic.** matrix-sdk retries decryption when room keys arrive and emits a `VectorDiff::Set` for the affected item; `forward_timeline` already forwards `Set` verbatim and re-runs `item_to_vm`, so a UTD stub becomes a decrypted Message with no extra code.

**Encryption-status = verification_state.** `client.encryption().verification_state()` returns `Subscriber<VerificationState>` with `.get()` (current) + `.next().await` (updates) — a drop-in shape for the connection-status producer. It is per-account; the banner aggregates ("any signed-in account Unverified"), matching the offline-pill pattern. Map `Verified→Verified`, `Unverified→Unverified`, `Unknown→Unknown`; never show the banner on `Unknown`.

**Banner dismissal is session-scoped.** A pure UI nag-preference, not Matrix domain state — kept in the zustand store only (resets on restart, re-nudging a still-unverified device is acceptable security UX). No localStorage, no Rust persistence.

## Verification

**Commands:**
- `bun run check` -- expected: Biome + tsc + vitest all green.
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` clean.
- `bun run test:rust` -- expected: cargo-nextest green; ts-rs regenerates `EncryptionStatus`/`EncryptionStatusBatch`/`TimelineItemVm` bindings with no git drift.

**Manual checks (real encrypted room, test credentials in 1Password):**
- Send from keeper into an E2EE Room; confirm another Matrix client (e.g. Element) reads it, and that client's messages decrypt in keeper.
- On a fresh unverified login, confirm the banner appears; dismiss it and confirm the Settings badge persists; confirm history without keys shows the UTD stub (never a blank row).

## Auto Run Result

Status: done

**Summary:** Surfaced the honest E2EE states without touching the already-working crypto. Added a `TimelineItemVm::Utd` variant so undecryptable events render an explicit never-blank stub, and a per-account encryption-status stream (mapped from the SDK's `verification_state()`, mirroring the connection-status lifecycle) that drives a dismissible "verify this device" banner, an account-scoped persistent Settings badge, and a read-only Encryption section in Settings. Transparent decrypt/encrypt required no new wiring (the `e2e-encryption` feature and per-account crypto store are already in place); UTD→decrypted re-render is automatic via the SDK's `Set` diff. No `EncryptionSettings`, cross-signing, or key-backup code added (Stories 3.2/3.3).

**Files changed:**
- `src-tauri/crates/keeper-core/src/vm.rs` — `TimelineItemVm::Utd`, `EncryptionStatus`, `EncryptionStatusBatch` (+ serde round-trip unit tests).
- `src-tauri/crates/keeper-core/src/timeline.rs` — `item_to_vm` UTD branch (`MsgLikeKind::UnableToDecrypt`), carrying only key/sender/display-name/timestamp.
- `src-tauri/crates/keeper-core/src/account.rs` — `EncryptionSink`, `subscribe_encryption_status`/`unsubscribe_encryption_status`/`run_encryption_status_producer`/`map_encryption_status` (+ mapping test).
- `src-tauri/crates/keeper/src/ipc.rs`, `lib.rs` — two new commands registered in the handler.
- `src/lib/ipc/client.ts` (+ `gen/EncryptionStatus.ts`, `gen/EncryptionStatusBatch.ts`, `gen/TimelineItemVm.ts`) — typed IPC + regenerated bindings.
- `src/lib/stores/encryption-status.ts` — per-account mirror + banner/badge selectors (+ per-account badge selector, new-unverified re-nag).
- `src/lib/stores/settings-ui.ts` — shared Settings open-state.
- `src/hooks/use-encryption-statuses.ts` — all-account subscriber.
- `src/components/chat/utd-stub.tsx` — never-blank UTD stub (polite `role="status"`).
- `src/components/layout/verify-banner.tsx` — dismissible global banner.
- `src/components/layout/conversation-pane.tsx` — renders UTD rows.
- `src/components/layout/app-shell.tsx` — mounts banner + subscriber hook.
- `src/components/layout/account-footer.tsx` — shared settings store + account-scoped decorative verify badge.
- `src/components/settings/settings-dialog.tsx` — read-only Encryption section (Verified / Not verified / Checking…).
- Colocated tests for all of the above.

**Review findings:** 5 patches applied (3 medium: UTD a11y `role`, account-scoped verify badge, Settings "Checking…" for unknown; 2 low: new-unverified banner re-nag, UTD self-heal test). 1 deferred (Rust subscribe returns `Ok` for an aborted producer in the narrow spawn→register account-removal race — a faithful copy of the pre-existing connection-status pattern; fix both paths together later). 11 rejected (AC-mandated stub copy, specified UTD grouping, mirror-pattern stream-end/teardown, cosmetic styling/layout/perf, i18n-not-yet, standard-race coverage). No intent_gap, no bad_spec.

**Verification:** `bun run check` (biome + tsc + vitest: 271 passed), `bun run check:rust` (rustfmt + clippy `-D warnings`: clean), `bun run test:rust` (cargo-nextest: 179 passed; ts-rs bindings regenerate with only the expected diff). All re-run green after the review patches. The live encrypted-room round-trip (AC-1) and the fresh-unverified-login banner flow are manual real-credential checks, intentionally not run unattended.

**Residual risks:** AC-1's transparent decrypt/encrypt and the UTD-on-missing-keys path are proven at the unit/mapping level but not by a live encrypted-room round-trip; the manual checks above should be run before shipping. The all-account encryption subscriber inherits the same tear-all/rebuild-all subscription churn on account-set changes already deferred for connection-status (2.1/2.5).
