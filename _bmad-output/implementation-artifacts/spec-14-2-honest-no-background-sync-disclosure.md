---
title: 'Honest No-Background-Sync Disclosure'
type: 'feature'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: ['oversized']
baseline_revision: '085efe45d5799147f20b9d509a4840531a807c2e'
final_revision: '49f6cd0c1152524d4756ac65c1197badfb1d5715'
---

<intent-contract>

## Intent

**Problem:** On iOS keeper only syncs and notifies while it is open (Story 14.1 pauses sync on background), but nothing tells the user that plainly, and one desktop surface actively lies on the phone tier: Settings' "Background & dock" copy (`BACKGROUND_QUIT_SENTENCE`) promises "⌘W keeps keeper running in the background… keeps syncing, and still shows notifications" — false on iOS and a background-delivery claim the honesty rule (FR-53) forbids. There is also no place, in-app, that establishes the canonical "only while open" copy that Story 15.2's `docs/ios.md` must later mirror one-to-one.

**Approach:** Add the FR-61 lifecycle-honesty disclosure on the reduced-capability (iOS) tier: a one-time acknowledgeable card shown on first entry to the app shell (i.e. just after the Wizard's Done step for a fresh install, or the first Inbox render for a restored Account), persisted device-globally in Rust so it shows once ever; the exact same canonical sentence lives permanently in Settings → Notifications, alongside a note that the app-icon badge is not a live count while keeper is closed. Audit the iOS surface and close the one background-delivery leak by hiding the desktop "Background & dock" section on the reduced tier. Desktop is untouched.

## Boundaries & Constraints

**Always:**
- The one-time card and the Settings → Notifications copy render the EXACT canonical string, verbatim, as the single exported source of truth (so `docs/ios.md` in Story 15.2 can match it one-to-one): `"On iPhone, keeper syncs and notifies only while open. Close it and messages wait on your homeserver until you return — nothing is lost, and nothing here pretends to be push."`
- All iOS-only surfaces gate on `useIsReducedCapabilityPlatform()` — the same capability read Story 13.7 uses; never a user-agent/build-flag/OS check, never a new `CapabilitiesVm` field.
- "Shown once" persists through the Rust `settings` k/v table (`registry::get/set_setting`), mirroring `get/set_dock_badge_mode` — device-global, one-way latch; the source of truth stays in Rust.
- Voice rules (UX-DR10/UX-DR17): sentence case, no exclamation marks, honest consequence-naming; new copy constants carry a JSDoc citing Story 14.2 + the FR, mirroring `BACKGROUND_QUIT_SENTENCE`.
- Rust: no `.unwrap()`/bare `.expect()` in production paths; `cargo fmt` + clippy `-D warnings` clean. TS: no `any`, `import type` for types, Biome clean.
- The disclosure is best-effort/non-trapping: a failed "shown" read is treated as already-shown (never nag/loop); a failed persist still hides the card for the session; no toast.

**Block If:**
- Satisfying an AC would require the Story 14.3 iOS app-icon badge *mechanism* (posting the badge, an iOS badge-mode control) — that is 14.3; 14.2 only adds the badge *disclosure copy*.
- The canonical copy string in the epic/PRD were to conflict with what a surface already ships — stop rather than silently reword the mandated sentence.

**Never:**
- Never show the card, or hide the "Background & dock" section, on desktop (non-reduced tier) — Story 10.3 copy and behavior stay exactly as-is.
- Never re-show the card after it has been acknowledged (persisted latch).
- Never build the iOS badge mechanism, an iOS badge-mode control, or edit `docs/ios.md` (Story 14.3 / 15.2 respectively).
- Never introduce a new shared copy/i18n module — follow the project's inline `const …_SENTENCE` convention.
- Never add `localStorage` (the frontend uses none) — persist via Rust.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh iOS install, wizard finished | reduced tier, `hasAccount`, `!wizardActive`, flag unset | one-time card renders the canonical sentence; acknowledge persists the flag and hides it | persist error swallowed; card still hides for the session |
| Existing Account restored at boot (iOS) | reduced tier, `hasAccount` at first shell render, flag unset | same one-time card on first Inbox render | same |
| Card already acknowledged | reduced tier, flag = shown | no card | n/a |
| Mid-wizard / no account yet | reduced tier, `wizardActive` or `!hasAccount` | no card (waits for first shell entry) | n/a |
| Desktop | non-reduced tier | no card ever; "Background & dock" section + `BACKGROUND_QUIT_SENTENCE` render unchanged | n/a |
| "Shown" flag read fails at boot | reduced tier, IPC rejects | treat as already-shown → no card (never trap) | error swallowed |
| Settings → Notifications (iOS) | reduced tier, dialog open | canonical sentence + badge-not-live note render; "Background & dock" section hidden | n/a |
| Settings → Notifications (desktop) | non-reduced tier | no iOS disclosure; "Background & dock" unchanged | n/a |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/registry.rs` -- home of `get/set_setting` (148/167) and the sibling `get/set_dock_badge_mode` (490/499) to mirror; add the disclosure-shown k/v helpers + round-trip test.
- `src-tauri/crates/keeper/src/ipc.rs` -- thin `#[tauri::command]` pattern (see `encryption_posture` 1409 / `set_encryption_posture` 1400); add the get/set commands resolving `state.platform.data_dir()`.
- `src-tauri/crates/keeper/src/lib.rs` -- `generate_handler!` list (159); register both new commands (near `dock_badge_mode_*` at 287).
- `src/lib/ipc/client.ts` -- typed wrappers (mirror `encryptionPosture` 460 / `setEncryptionPosture` 451); add the two disclosure wrappers.
- `src/components/settings/no-background-sync-disclosure.tsx` -- NEW: the one-time card + exported copy constants; mirrors `at-rest-encryption-choice.tsx` (a boot-time honesty gate) and uses `@/components/ui/dialog`.
- `src/App.tsx` -- mounts boot gates/hooks (`AtRestEncryptionChoice`, `useAppLifecycle`); mount the self-gating card alongside `<Toaster/>` (105) so it can overlay the shell.
- `src/components/settings/settings-dialog.tsx` -- `NotificationsSection` (248) add the iOS disclosure block; gate `<BackgroundSection>` (call site ~141) behind `!reducedPlatform`. `BACKGROUND_QUIT_SENTENCE` (309) is the offending phone-tier surface.
- `src/lib/stores/capabilities.ts` -- `useIsReducedCapabilityPlatform()` (the gate), and `useAccountsStore`/`useWizardStore` for the card's show conditions.
- `src/components/settings/about-section.tsx` -- Story 13.7 "On this iPhone" list (already forward-references Epic 14); do NOT duplicate/contradict it.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- add `const UI_IOS_SYNC_DISCLOSURE_SHOWN_KEY: &str = "ui.ios_sync_disclosure_shown"`, `get_ios_sync_disclosure_shown(data_dir) -> Result<bool>` (present `"1"` ⇒ true, absent ⇒ false) and `set_ios_sync_disclosure_shown(data_dir) -> Result<()>` (writes `"1"`), mirroring the dock-badge helpers. -- device-global one-way latch.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- add `#[tauri::command] ios_sync_disclosure_shown_get(state) -> Result<bool, IpcError>` and `ios_sync_disclosure_shown_set(state) -> Result<(), IpcError>`, resolving `state.platform.data_dir()` (map_err `to_ipc_error`) then calling the registry helpers. -- typed IPC seam.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register both commands in `generate_handler!`. -- wires them.
- [x] `src/lib/ipc/client.ts` -- add `iosSyncDisclosureShownGet(): Promise<boolean>` and `iosSyncDisclosureShownSet(): Promise<void>` wrapping the commands. -- frontend seam.
- [x] `src/components/settings/no-background-sync-disclosure.tsx` -- NEW: export `NO_BACKGROUND_SYNC_SENTENCE` (the exact canonical string) and `BADGE_NOT_LIVE_SENTENCE`; a `NoBackgroundSyncDisclosure` component that returns `null` unless `useIsReducedCapabilityPlatform() && hasAccount && !wizardActive` and the tri-state "shown" read (`undefined` loading / `false` unshown / `true` shown, read failure ⇒ shown) is `false`; when shown, render a modal (`@/components/ui/dialog`) with the canonical sentence and a "Got it" acknowledge button; acknowledging (and any close) calls `iosSyncDisclosureShownSet()`, swallows failure, and hides for the session. -- the one-time FR-61 card.
- [x] `src/App.tsx` -- mount `<NoBackgroundSyncDisclosure />` alongside `<Toaster />` (above the content gate). -- self-gating overlay.
- [x] `src/components/settings/settings-dialog.tsx` -- in `NotificationsSection`, when `useIsReducedCapabilityPlatform()`, render `NO_BACKGROUND_SYNC_SENTENCE` and `BADGE_NOT_LIVE_SENTENCE` as muted paragraphs; gate the `<BackgroundSection>` call site behind `!reducedPlatform`. -- permanent Settings copy + closes the background-delivery leak on iOS.
- [x] `src/components/settings/no-background-sync-disclosure.test.tsx` -- NEW: cover the I/O matrix — shows on reduced tier when `hasAccount && !wizardActive && unshown`; acknowledge calls the setter and hides; no card on desktop, mid-wizard, no-account, already-shown, or read-failure. -- guards gating + latch.
- [x] `src/components/settings/settings-dialog.test.tsx` -- extend: on the reduced tier the Notifications section shows the canonical sentence + badge note and the "Background & dock" section (incl. `BACKGROUND_QUIT_SENTENCE`) is absent; on desktop the reverse. -- the copy-sweep audit guard.

**Acceptance Criteria:**
- Given the reduced-capability (iOS) tier with at least one Account and the wizard not active, when the app shell first renders and the disclosure has not been acknowledged, then a one-time card shows exactly the canonical sentence, and acknowledging it persists the shown flag so it never appears again (including across relaunch).
- Given the iOS tier, when Settings → Notifications is open, then the same canonical sentence appears permanently plus a note that the app-icon badge is not a live count while keeper is closed, and no surface implies background delivery — the desktop "Background & dock" / ⌘W-keeps-syncing copy is not shown on this tier.
- Given the desktop tier, when the app runs and Settings opens, then no iOS card ever shows and Story 10.3's "Background & dock" section and copy are unchanged.
- Given the full change, when the quality gates run, then `bun run check`, `bun run check:rust`, and `bun run test:rust` all pass.

## Spec Change Log

<!-- Append-only. Empty until the first bad_spec loopback. -->

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 1
- reject: 10
- addressed_findings:
  - none
- notes:
  - Deferred (1, logged to `deferred-work.md`): gating the whole "Background & dock" section off the reduced tier removed the only badge-mode control (all/mentions/off) reachable on iOS, while the new `BADGE_NOT_LIVE_SENTENCE` advertises the badge — Story 14.3 should re-surface an iOS-appropriate badge-mode control when it builds the app-icon badge.
  - Rejected (10): **card ambushing the pre-login `AtRestEncryptionChoice`/`LoginScreen`** (not reachable — the card gates on `hasAccount`, and the at-rest posture always resolves *before* any account exists, so `renderContent` only reaches the shell when `hasAccount && !wizardActive`; the Edge Case Hunter independently confirmed the gating); device-global latch vs. multi-account restore (by-design — a platform disclosure shown once per device is correct); acknowledge-before-persist / quit-in-flight reappearance (spec-sanctioned, non-trapping; re-showing an honest disclosure once more is harmless); canonical sentence in `DialogDescription` styling (idiomatic slot, announced via `aria-describedby`); exported `TITLE`/`ACK_LABEL` drift risk (UI labels, not the canonical contract); fresh-install hand-off test proves a weaker property (test-quality nit, gates still covered); stale inner `BackgroundSection` probe comment (still correct on desktop, section never mounts on iOS); `shown` not reset on a `reduced` true→false→true flip (unreachable — capabilities hydrate once and the mount-once card never unmounts).

## Design Notes

- **One trigger covers both AC moments.** Rather than distinguish "restored at boot" from "just created via wizard" (no such signal exists — both funnel through `accountsStore`), the card self-gates on `reduced && hasAccount && !wizardActive && unshown`. For a fresh install that fires the instant the wizard's Done step hands off to the shell; for a restored Account it fires on the first Inbox render — exactly the two moments the AC names, with one mechanism.
- **Persistence idiom.** Reuse the `settings` k/v table exactly as `sdk_encryption` / `notify.dock_badge_mode` do; a one-way `"1"` latch is enough (no "off" state). Device-global is correct — the disclosure is about the platform, not an Account.
- **Single source of truth for the copy.** `NO_BACKGROUND_SYNC_SENTENCE` is exported once and imported by both the card and `settings-dialog.tsx`, so the card and Settings can never drift; Story 15.2 transcribes this same constant into `docs/ios.md`.
- **Audit scope.** The one live background-delivery claim on the iOS surface is `BACKGROUND_QUIT_SENTENCE` (⌘W/⌘Q, desktop-only mechanics); hiding the whole "Background & dock" section on the reduced tier removes it cleanly (its launch-at-login/menu-bar rows are already capability-hidden, and the Dock-badge control is a desktop noun the iOS badge story 14.3 will re-surface appropriately). `NOTIFY_PREVIEWS_SENTENCE`'s "this Mac" wording is a device-*noun* inaccuracy, not a background-delivery claim — out of scope here (platform-noun sweep belongs with 13.7/15.2), noted not fixed.

## Verification

**Commands:**
- `bun run check:rust` -- expected: `cargo fmt --check` + clippy `-D warnings` clean over the new registry helpers and IPC commands.
- `bun run test:rust` -- expected: cargo-nextest green, including the disclosure-shown round-trip test.
- `bun run check` -- expected: Biome + tsc + Vitest green, including `no-background-sync-disclosure.test.tsx` and the extended `settings-dialog.test.tsx` reduced-tier assertions.

## Auto Run Result

Status: done

**Summary:** Added the FR-61 iOS lifecycle-honesty disclosure. A one-time acknowledgeable card (Radix `Dialog`) shows on the reduced-capability (iOS) tier the first time the app shell renders with an Account and the wizard is closed — one self-gating mechanism covering both the fresh-install wizard hand-off and the first Inbox render for a restored Account. "Shown once" persists device-globally through a new Rust `settings` k/v latch (`ui.ios_sync_disclosure_shown`). The exact canonical sentence lives permanently in Settings → Notifications alongside a badge-not-live note, both sourced from a single exported constant so the card, Settings, and (later) `docs/ios.md` cannot drift. The honesty audit closed the one live background-delivery leak: the desktop "Background & dock" section (⌘W/⌘Q "keeps syncing in the background" copy + Dock-badge radio) is now hidden on the reduced tier. Desktop (Story 10.3) untouched.

**Files changed:**
- `src-tauri/crates/keeper-core/src/registry.rs` — `ui.ios_sync_disclosure_shown` one-way latch (`get/set_ios_sync_disclosure_shown`) + round-trip test.
- `src-tauri/crates/keeper/src/ipc.rs` — `ios_sync_disclosure_shown_get/set` commands.
- `src-tauri/crates/keeper/src/lib.rs` — command registration.
- `src/lib/ipc/client.ts` — `iosSyncDisclosureShownGet/Set` wrappers.
- `src/components/settings/no-background-sync-disclosure.tsx` (new) — the one-time card + exported canonical copy constants (`NO_BACKGROUND_SYNC_SENTENCE`, `BADGE_NOT_LIVE_SENTENCE`).
- `src/components/settings/no-background-sync-disclosure.test.tsx` (new) — 10 tests over the full I/O matrix.
- `src/App.tsx` — mounts the self-gating card above the content gate.
- `src/components/settings/settings-dialog.tsx` — permanent iOS disclosure copy in `NotificationsSection`; `BackgroundSection` gated behind `!reducedPlatform`.
- `src/components/settings/settings-dialog.test.tsx` — reduced-tier (iOS) vs. desktop copy/section assertions.

**Review findings breakdown:** 0 intent gaps, 0 spec repairs, 0 patches. 1 deferred (`deferred-work.md`): the iOS badge-mode control (all/mentions/off) is no longer reachable after gating "Background & dock" off the reduced tier — Story 14.3 should re-surface an iOS-appropriate control when it builds the app-icon badge. 10 rejected — the headline "card ambushes the pre-login encryption/login screens" was a render-order misread (the card gates on `hasAccount`, and the at-rest posture always resolves before any account exists, so `renderContent` only reaches the shell when `hasAccount && !wizardActive`; independently confirmed by the Edge Case Hunter); the remainder were by-design (device-global latch, non-trapping acknowledge, idiomatic `DialogDescription`) or noise.

**Follow-up review recommended:** false — the review pass made no code changes (no patches, no loopback); its single outcome was a forward-looking defer entry.

**Verification** (all re-run independently after implementation):
- `bun run check:rust` — PASS (`cargo fmt --check` + clippy `-D warnings`).
- `bun run test:rust` — PASS (770 tests, incl. the disclosure-shown round-trip).
- `bun run check` — PASS (Biome + tsc + Vitest 1181 tests incl. the new/extended settings tests; core-tauri-free convention holds).

**Residual risks:** the iOS badge-mode control gap (deferred to 14.3); a quit between tapping "Got it" and the fire-and-forget persist can re-show the honest card once more on next launch (spec-sanctioned, non-trapping, harmless); `NOTIFY_PREVIEWS_SENTENCE`'s "this Mac" device-noun still shows on iOS — a separate platform-noun sweep out of this story's scope (noted in Design Notes).
