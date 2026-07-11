---
title: 'Capability-Gated Surfaces and "On this iPhone" Disclosure'
type: 'feature'
created: '2026-07-11'
status: 'done'
baseline_revision: 'ce17e2a750906071872eaca645d5fc1484d3c67d'
final_revision: 'fc3bff80bf5da1722acf76bf73c73f4091585c73'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** On the phone tier the desktop-only surfaces still render: the bbctl "Run your own bridge" panel, the Settings → Shortcuts (global-hotkey) section, the Settings → About software-update controls, the "Launch at login" + "Keep in menu bar" toggles, and the ⌘? cheat-sheet overlay. On iOS these are dead affordances — the backend already rejects them with a clean `Unsupported` IpcError (Story 12.2 stubs), but nothing hides the UI, so iOS limits read as broken buttons instead of facts. There is also no single honest statement of what the phone can't do.

**Approach:** Wire each of the six surfaces to the capability flag it already maps to in the `capabilitiesStore` mirror (Story 12.2) so it renders only when that capability is present, and add a capability-honest "On this iPhone" disclosure to Settings → About plus a backup-exclusion line to Settings → Archive & Storage — both gated on a hydrated all-desktop-surfaces-absent signal. Pure frontend, reuse-only: desktop (all capabilities present) stays byte-for-byte unchanged, and all hiding flows exclusively from the store (never platform sniffing — the existing convention test enforces it).

## Boundaries & Constraints

**Always:**
- Every hide flows from `useCapabilitiesStore` reading a `CapabilitiesVm` flag — never `navigator.*`, the Tauri OS plugin, `import.meta.env`, or any platform inference (the `src/test/no-user-agent-gating.test.ts` convention test must stay green).
- Surface → flag map (hide when the flag is `false`): bbctl panel → `bridgeSidecar`; Shortcuts section → `globalHotkey`; About software-update block → `inAppUpdater`; "Launch at login" row → `launchAtLogin`; "Keep in menu bar" row → `trayIcon`; cheat-sheet overlay → `nativeMenuBar`.
- The "On this iPhone" disclosure and the Archive & Storage backup line render only when the platform is capability-reduced: `hydrated === true` AND all seven `CapabilitiesVm` flags are `false`. The `hydrated` guard prevents the all-`false` safe default from flashing the iOS copy on desktop before the mirror resolves.
- Desktop (all flags `true`) renders identical to today: all six surfaces present, no "On this iPhone" list, no Archive backup line.
- Bridge management stays fully functional on iOS: discovery, native provisioning login, Bridge Bot fallback, health + re-login, risk tiers, start-new-Chat are untouched — only the bbctl runner panel hides.
- Copy follows project voice: sentence case, no exclamation marks, honest consequence-naming. The docs link opens `https://github.com/tgorka/keeper/blob/main/docs/ios.md` via `openUrl` from `@tauri-apps/plugin-opener` (best-effort, matching `login-screen.tsx`).

**Block If:**
- A surface the AC requires hidden has no corresponding flag in `CapabilitiesVm` (would force a backend VM change — out of this frontend story's scope). All six map today; halt only if that changed.
- Hiding a surface on iOS would also hide it on a desktop build (a capability flag conflated with a user preference — e.g. the `nativeMenuBar` capability must not be confused with the user's `menuBarPresence` toggle). Desktop must not regress.

**Never:**
- No backend changes. The `Unsupported` IpcError stubs (`hotkey_get/set`, `launch_at_login_get/set`, `menu_bar_presence_set`; `menu_bar_presence_get → Ok(false)`; `sidecar_path → Unsupported`) and the platform-blind palette registry already landed in Story 12.2 — do not re-implement, modify, or add capability threading to `palette_actions()`.
- No new capability flags; no edits to `CapabilitiesVm` (Rust or generated TS).
- Do not gate the egress list, the Dock-badge radio, the ⌘W/⌘Q background/quit copy, or any bridge-management surface — out of scope. iOS badge and lifecycle disclosures belong to Epic 14 (14-2/14-3).
- Do not implement the actual backup-exclusion file flagging (Epic 14-7) — 13.7 adds only the disclosure line.
- No platform sniffing; no forked/duplicated components.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Desktop build | all 7 flags `true`, hydrated | All six surfaces render; no "On this iPhone" list; no Archive backup line — byte-for-byte unchanged | No error expected |
| iOS build | all 7 flags `false`, hydrated | bbctl panel, Shortcuts section, About update block, Launch-at-login row, Keep-in-menu-bar row, cheat-sheet overlay all absent; "On this iPhone" list + Archive backup line render; egress list + bridge management intact | No error expected |
| Pre-hydration | DEFAULT_CAPABILITIES (all `false`), `hydrated === false` | Desktop-only surfaces hidden by safe default; "On this iPhone" list + Archive backup line NOT shown (hydrated gate) | No flash of iOS copy |
| Docs link tap | reduced platform, tap "docs/ios.md" link | `openUrl(<repo>/blob/main/docs/ios.md)` opens externally | Opener failure caught, best-effort no-op |
| Programmatic reach | iOS, code invokes a hidden surface's command | Clean `Unsupported` IpcError (`retriable:false`) — already implemented in Story 12.2 | Surfaced as-is; not re-implemented here |

</intent-contract>

## Code Map

- `src/lib/stores/capabilities.ts` (+ `capabilities.test.ts`) -- MODIFY: add a pure predicate `isReducedCapabilityPlatform(state)` (true when `hydrated` and all seven flags `false`) and a `useIsReducedCapabilityPlatform()` hook wrapping it; unit-test both. Drives the disclosure + Archive line.
- `src/components/layout/bridges-pane.tsx` (+ test) -- MODIFY: read `bridgeSidecar`; change the render at ~L136 to `{isBeeper && bridgeSidecar && <BbctlPanel .../>}`. Bridge discovery/cards untouched.
- `src/components/settings/settings-dialog.tsx` (+ test) -- MODIFY: (a) in `SettingsDialog`, wrap `<ShortcutsSection>` (L123) with `globalHotkey`; add the Archive & Storage backup-exclusion `<p>` (gated on reduced-platform) to the top block (L115-119). (b) in `BackgroundSection`, gate the "Launch at login" row on `launchAtLogin` and the "Keep in menu bar" row on `trayIcon`; leave the Dock-badge radio and quit copy as-is.
- `src/components/settings/about-section.tsx` (+ test) -- MODIFY: gate the "Software updates" sub-block (L202-246) on `inAppUpdater`; add the "On this iPhone" rendered list (gated on reduced-platform) with the four honesty lines and the `openUrl` docs link. Leave the egress list ungated.
- `src/components/layout/app-shell.tsx` (+ `cheat-sheet-overlay.test.tsx`) -- MODIFY: read `nativeMenuBar`; change the mount at L153 to `{nativeMenuBar && <CheatSheetOverlay />}`. The `useCheatSheetShortcut()` hook (L69) stays wired (rules-of-hooks); the overlay is unmounted so it cannot render.

## Tasks & Acceptance

**Execution:**
- [x] `src/lib/stores/capabilities.ts` (+ `.test.ts`) -- MODIFY: `isReducedCapabilityPlatform` predicate + `useIsReducedCapabilityPlatform` hook -- the single capability-honest "reduced platform" signal, tested for desktop / iOS / pre-hydration.
- [x] `src/components/layout/bridges-pane.tsx` (+ test) -- MODIFY: gate `BbctlPanel` on `bridgeSidecar` -- bbctl runner hidden on iOS; discovery/provisioning/bot/health/risk/new-chat stay.
- [x] `src/components/settings/settings-dialog.tsx` (+ test) -- MODIFY: gate `ShortcutsSection` on `globalHotkey`, the two `BackgroundSection` rows on `launchAtLogin`/`trayIcon`, and add the reduced-platform Archive & Storage backup-exclusion line -- Settings surface hiding + Archive disclosure.
- [x] `src/components/settings/about-section.tsx` (+ test) -- MODIFY: gate the software-update block on `inAppUpdater`; add the reduced-platform "On this iPhone" list with the docs/ios.md `openUrl` link -- About disclosure + updater hiding.
- [x] `src/components/layout/app-shell.tsx` (+ `cheat-sheet-overlay.test.tsx`) -- MODIFY: gate `<CheatSheetOverlay/>` on `nativeMenuBar` -- cheat sheet hidden on iOS.

**Acceptance Criteria:**
- Given an iOS build (all `CapabilitiesVm` flags `false`, mirror hydrated), when Settings and the bridges/app surfaces render, then the bbctl panel, Shortcuts section, About software-update block, Launch-at-login row, Keep-in-menu-bar row, and cheat-sheet overlay do not render at all — no dead buttons, no error-on-tap — while the egress list and every bridge-management surface remain fully functional.
- Given the same iOS build, when Settings → About renders, then an "On this iPhone" list states: syncs and notifies only while open (background notifications await a future decision), no self-hosted bridge runner (manage from your Mac), no global hotkey, and updates arrive by reinstall with a signature renewing every 7 days — plus a link that opens docs/ios.md; and Settings → Archive & Storage adds a line that the phone's Local Archive is excluded from device backup while the Mac remains the durable, exportable copy.
- Given a desktop build (all flags `true`), then all six surfaces render exactly as before and neither the "On this iPhone" list nor the Archive backup line appears — desktop is byte-for-byte unchanged.
- Given the pre-hydration window (`hydrated === false`, all flags at the all-`false` safe default), then the desktop-only surfaces are hidden by the safe default but the iOS-specific "On this iPhone" list and Archive backup line do NOT flash on desktop.
- Given all hiding logic, then it reads only `useCapabilitiesStore`/`useIsReducedCapabilityPlatform` — no `navigator.*`, Tauri OS plugin, or `import.meta.env` — so `no-user-agent-gating.test.ts` stays green; and the palette Actions scope shows no dead desktop-only entries (satisfied by construction — the registry has no desktop-only actions).
- Given `bun run check`, then Biome + `tsc --noEmit` + vitest pass, including the new `capabilities` predicate tests and the extended `bridges-pane` / `settings-dialog` / `about-section` / `cheat-sheet-overlay` suites; no Rust changes.

## Design Notes

**Frontend-only — the backend leg already shipped in Story 12.2.** The AC's "programmatic reach returns a clean `Unsupported` IpcError" and "desktop-only palette actions unregistered on iOS" are already true: `hotkey_*`, `launch_at_login_*`, `menu_bar_presence_set`, and `sidecar_path` return `CoreError::Unsupported` on iOS (`menu_bar_presence_get` honestly returns `Ok(false)`), and `palette_actions()` contains zero desktop-only entries (all actions — inbox/archive/approval/bridges/new-chat/search/export/add-account/incognito/sync/mute/etc. — are universal). Verify the registry still has no desktop-only action at implementation time; if one appeared, that's a `bad_spec` loopback, not silent scope growth. This story adds only the UI hiding + disclosure.

**Reduced-platform predicate.** `isReducedCapabilityPlatform = hydrated && !trayIcon && !globalHotkey && !launchAtLogin && !inAppUpdater && !nativeMenuBar && !bridgeSidecar && !revealInFileManager`. All seven flags move together (each is `cfg!(desktop)` in the `capabilities` command), so this equals "iOS" today while staying a pure capability read. The `hydrated` term is load-bearing: without it the all-`false` `DEFAULT_CAPABILITIES` would render the iOS disclosure on desktop for one frame before hydration.

**Cheat sheet rides `nativeMenuBar`.** The cheat-sheet overlay and the native menu bar are the two projections of the same Story 9.3 action registry, so `nativeMenuBar` is the honest flag. Note the flag is distinct from the user's "Keep in menu bar" preference (`menuBarPresence`): the `nativeMenuBar` *capability* is always `true` on desktop regardless of that toggle, so gating the cheat sheet on it never hides it on desktop.

**Scope boundary (Epic 14).** The Dock-badge radio, the ⌘W/⌘Q background/quit copy, and the actual backup-exclusion file flagging are Epic 14's territory (14-2 no-background-sync disclosure, 14-3 iOS badge, 14-7 backup exclusion). 13.7 gates only the six AC-enumerated capability-flagged surfaces and adds the two disclosure texts — it does not re-word or hide the desktop background/quit copy.

**Test approach.** Component tests drive the store via `capabilitiesStore.getState().applySnapshot(vm)` to simulate desktop (all `true`) vs iOS (all `false`) vs pre-hydration, then assert presence/absence via Testing Library queries. jsdom renders DOM only; on-device WKWebView confirmation folds into Epic 14/15 per the epic.

## Verification

**Commands:**
- `bun run check` -- expected: Biome clean, `tsc --noEmit` clean, vitest green including `capabilities` (new predicate/hook tests), `bridges-pane`, `settings-dialog`, `about-section`, `cheat-sheet-overlay`, and the unchanged `no-user-agent-gating` convention test.
- `bun run test -- capabilities bridges-pane settings-dialog about-section cheat-sheet-overlay no-user-agent-gating` -- expected: the touched suites pass in isolation.

**Manual checks (no device required for acceptance):**
- With the mirror set to all-`false` (iOS) in a sub-768px webview, open Settings and confirm: no Shortcuts section, no software-update block, no Launch-at-login / Keep-in-menu-bar rows, the "On this iPhone" list and Archive backup line present, egress list intact; the bbctl panel is absent while bridge discovery/cards remain; the cheat-sheet overlay does not open. With all-`true` (desktop), confirm every surface is present and the two iOS disclosures are absent.

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 0, low 3)
- defer: 1
- reject: 11
- addressed_findings:
  - `[low]` `[patch]` Cheat-sheet ⌘? was still wired on the phone tier (`use-cheat-sheet-shortcut.ts`): with the overlay gated off by `nativeMenuBar`, pressing ⌘? still called `cheatSheetStore.toggle()`, leaving a stale `isOpen: true` that nothing renders and any future store consumer would react to. Fixed: the handler now reads `capabilitiesStore.getState().capabilities.nativeMenuBar` at event time and returns early (no `preventDefault`, no toggle) on the reduced tier. Regression test added; existing shortcut tests now hydrate the desktop tier in setup.
  - `[low]` `[patch]` `BackgroundSection` fired `launchAtLoginGet()`/`menuBarPresenceGet()` on every iOS Settings open even though both rows are gated off — dead `Unsupported` IPC round-trips. Fixed: the two fetches are now guarded by `launchAtLogin`/`trayIcon` (with those flags added to the effect deps), matching the mount-level gating used by the other surfaces. Assertion added to the iOS settings-dialog test.
  - `[low]` `[patch]` `isReducedCapabilityPlatform` hand-listed all seven flags, a desync hazard if a capability is later added to `CapabilitiesVm`. Fixed: replaced the list with `Object.values(capabilities).every((present) => !present)` so a new (boolean) flag is folded in automatically. Existing predicate tests unchanged and green.

Rejected (11): iPhone-specific disclosure copy (the AC and story title specify "On this iPhone" / `docs/ios.md` verbatim — correct on the only platform it renders); pre-hydration updater flash / hydration-failure hides desktop surfaces (the all-`false` safe default is the deliberate 12.2 fail-safe — "never advertise a surface the platform lacks"; `capabilities()` returns `cfg!` bools and is effectively infallible; hydration completes at boot before Settings is opened); `DESKTOP_CAPABILITIES` test-fixture duplication / untyped / raw-`setState` reset (test hygiene; `applySnapshot(DESKTOP_CAPABILITIES)` is already shape-checked against `CapabilitiesVm` at the call site); `no-user-agent-gating` only bans four sniffing spellings (pre-existing 12.2 test; adequate for realistic patterns; broadening is out of scope); no test for open-while-unhydrated late-resolving-to-iOS (zustand subscription re-renders on `applySnapshot`; covered by the store's own tests); `openExternal` silent catch / untested rejection (matches the `login-screen.tsx` idiom the spec instructed to mirror; opener failure is extremely rare); `<a href>` middle/right-click lands in webview (the link renders only on the iOS touch tier where the sole activation is a tap → `onClick` → handled); cheat sheet borrowing `nativeMenuBar` rather than a dedicated flag (the spec's explicit, documented decision — both are Story 9.3 action-registry projections); mixed-flag partial-capability platform shows surfaces hidden with no disclosure (speculative — all flags are `cfg!(desktop)` and move as a set; no such platform exists, and Patch 3 removes the maintenance hazard anyway).

One defer: the export dialog's "Reveal in Finder" affordance (`src/components/export/export-dialog.tsx`) is not gated on the `revealInFileManager` capability, so on iOS it is a dead/error-on-tap affordance — a genuine capability-honesty gap, but pre-existing and outside Story 13.7's AC-enumerated six surfaces. Logged to the deferred-work ledger.

## Auto Run Result

Status: done

### Summary
Delivered Story 13.7 (FR-56 surface leg, FR-57; AD-27; UX-DR27) as a frontend-only change: six desktop-only UI surfaces are gated behind the capability flags they already map to in the `capabilitiesStore` mirror, and two capability-honest iOS disclosures are added. On iOS (all seven `CapabilitiesVm` flags `false`) the bbctl "Run your own bridge" panel (`bridgeSidecar`), the Settings → Shortcuts section (`globalHotkey`), the Settings → About software-update block (`inAppUpdater`), the "Launch at login" row (`launchAtLogin`), the "Keep in menu bar" row (`trayIcon`), and the ⌘? cheat-sheet overlay (`nativeMenuBar`) all render nothing — no dead buttons — while the egress list, Dock-badge radio, and every bridge-management surface (discovery, provisioning, bot fallback, health, risk tiers, start-new-Chat) stay intact. Settings → About gains an "On this iPhone" list (foreground-only sync, no self-hosted bridge runner, no global hotkey, reinstall/7-day-signature updates, plus an external `openUrl` link to `docs/ios.md`) and Settings → Archive & Storage gains a backup-exclusion line — both gated on a new `isReducedCapabilityPlatform` predicate (hydrated + every flag absent), whose `hydrated` term stops the all-`false` safe default from flashing iOS copy on desktop. All hiding flows exclusively from the capability store (the `no-user-agent-gating` convention test stays green); desktop (all flags `true`) is byte-for-byte unchanged. No Rust changes: the clean `Unsupported` IpcError stubs and the platform-blind palette registry already landed in Story 12.2, so those AC clauses hold by construction.

### Files changed
- `src/lib/stores/capabilities.ts` (+ test) — NEW `isReducedCapabilityPlatform` predicate (hydrated + `Object.values(...).every(absent)`, **review-simplified** from a hand-written flag list) and `useIsReducedCapabilityPlatform` hook.
- `src/components/layout/bridges-pane.tsx` (+ test) — bbctl panel gated on `bridgeSidecar`.
- `src/components/layout/app-shell.tsx` (+ `cheat-sheet-overlay.test.tsx`) — `<CheatSheetOverlay/>` mount gated on `nativeMenuBar`.
- `src/hooks/use-cheat-sheet-shortcut.ts` (+ test) — **review-patched**: ⌘? no-ops on the reduced tier so it never mutates a store nothing observes.
- `src/components/settings/settings-dialog.tsx` (+ test) — `ShortcutsSection` gated on `globalHotkey`; the two `BackgroundSection` rows gated on `launchAtLogin`/`trayIcon` (**review-patched** to also skip their now-dead backend fetches on the reduced tier); reduced-platform Archive & Storage backup-exclusion line.
- `src/components/settings/about-section.tsx` (+ test) — software-update block gated on `inAppUpdater` (egress list ungated); reduced-platform "On this iPhone" list with the `openUrl` docs link.

### Review findings breakdown
- Patches applied: 3 (all low) — cheat-sheet shortcut no-op guard, `BackgroundSection` dead-fetch guards, predicate `Object.values` simplification. Each covered by a test.
- Deferred: 1 — export dialog "Reveal in Finder" not gated on `revealInFileManager` (pre-existing, out of AC scope).
- Rejected: 11 — iPhone-copy-per-AC, fail-safe-by-design, test hygiene, speculative mixed-flag, idiom-matching. See the Review Triage Log.
- intent_gap: 0, bad_spec: 0.

### Follow-up review
`followup_review_recommended: false` — the three patches are localized, low-severity frontend guards (an event-time capability check, two effect-fetch guards, and a predicate refactor), each with a regression test, none touching the API surface, data model, security, or Rust; desktop stays byte-for-byte unchanged.

### Verification
- `bun run check` — green: Biome clean (298 files), `tsc --noEmit` clean, vitest **1161 passed / 1161** (109 files; +1 over the post-implementation 1160 for the new cheat-sheet-shortcut no-op test), core-tauri-free convention check passed. Run after implementation and again after the review patches.
- No `bun run check:rust` / `test:rust` needed — no Rust touched.

### Residual risks
- On-device WKWebView confirmation (the disclosures rendering under real safe-area/Dynamic-Type, the `openUrl` docs link opening in Safari, VoiceOver over the "On this iPhone" list) is only fully verifiable in the iOS Simulator / on a device; per the epic that folds into Epic 14/15 hardening + the SM-8 dogfooding gate, not this story's acceptance.
- The export dialog's "Reveal in Finder" affordance remains un-gated on iOS (deferred) — a residual capability-honesty gap tracked in the deferred-work ledger for a follow-up.
- The `isReducedCapabilityPlatform === iOS` equivalence holds because all flags are `cfg!(desktop)` today; a future non-iOS reduced-capability port would need its own disclosure copy (the current copy is iPhone-specific per the AC).
