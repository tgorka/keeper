---
title: 'First-Run Wizard'
type: 'feature'
created: '2026-07-05'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: [oversized]
baseline_revision: c69904b3e571c329732c655f42fdd0a5f230b9aa
final_revision: eaca5ec78a157b35f683e5008b6f29310ba2a7b5
---

<intent-contract>

## Intent

**Problem:** A first-run user lands on the bare `LoginScreen` and, even after signing in, must independently discover the Bridges surface, connect a network, and understand the "no homeserver" options — the individual flows (Epics 1–2 login, 6.2 discovery, 6.3/6.4 login stepper) exist but nothing ties them into a single guided path from zero to a bridged inbox.

**Approach:** Add a full-frame **First-Run Wizard** (Welcome → Add Account → Bridge discovery + per-Bridge login → Done) that **composes existing components** — `LoginScreen` in `addMode`, the `BridgeCard` (which owns its own ack gate + `BridgeLoginSheet`), and `useBridgeDiscovery`/`useBridgeCatalog` — driven by a session-scoped `wizardStore`. It auto-opens once on genuine first run (hydrated, zero accounts, posture chosen), is fully skippable (Skip on every step; Esc asks once), re-enterable from Settings, and lands the user in the Inbox — an empty inbox if everything is skipped.

## Boundaries & Constraints

**Always:** Reuse `LoginScreen`/`BridgeCard`/`BridgeLoginSheet`/discovery hooks — never reimplement login, discovery, QR, or risk-ack UI. The wizard's `active` state takes precedence in `App.tsx` over the `hasAccount` gate so adding an account mid-flow does not unmount the wizard. Every step is skippable and the wizard is a path, not a gate. The honest no-homeserver fork links to real docs only — no fabricated URLs and no fake sign-up. All UI strings sentence-case, English. No token/session material in the store.

**Block If:** A real managed-hosting/sign-up destination URL is required that does not already exist in the repo (there is none — use `COMPANION_STACK_DOCS_URL` and the in-step Beeper tab).

**Never:** No backend/Rust changes (pure frontend story). No persistence of the "seen/dismissed" flag (session-scoped only; re-entry from Settings covers repeat use). No new IPC. No auto-start of the wizard after a sign-out-of-last-account (first-run boot only). No embedding the at-rest-encryption choice inside the wizard (`App.tsx` already gates it before first sign-in).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| First run | hydrated, `accounts.length===0`, `postureChosen` resolved (not null/undefined) | Wizard auto-starts once; `App` renders `<FirstRunWizard/>` full-frame instead of `<LoginScreen/>` | n/a |
| Add-account success | LoginScreen `addAccount` grows account count | onDone detects growth → `setAccountId(newest)` + advance to discovery; wizard stays mounted despite `hasAccount` now true | n/a |
| Add-account cancel | onDone with unchanged count | Treated as cancel → return to Welcome (not advance) | n/a |
| Discovery for account | `accountId` set | Reuse `useBridgeCatalog`+`useBridgeDiscovery`; render `BridgeCard` per discovered network (loading / retriable-error / "No bridges found" states come from the reused pieces) | Retriable error surfaced by `useBridgeDiscovery` |
| Skip everything | user skips each step (or Esc-confirm at Welcome) | `finish()`: `active=false`; with zero accounts sets `dismissed=true` → `App` renders `<AppShell/>` empty inbox (footer "Add an account" card), NOT `<LoginScreen/>` | n/a |
| Esc pressed | any step | Opens a single confirm ("Skip setup? Run it again from Settings.") — does not exit immediately | n/a |
| Re-entry | Settings "Run setup again" clicked | `wizardStore.start()`, close Settings; wizard shows full-frame over the shell; `accountId` defaults to first account | n/a |
| Sign-out of last account | `hasAccount` goes false, wizard never started this session | `App` still renders `<LoginScreen/>` (unchanged) — wizard does NOT auto-start | n/a |

</intent-contract>

## Code Map

- `src/lib/stores/wizard.ts` -- **NEW**. Vanilla zustand store (mirror `add-account.ts`/`new-chat.ts`): `active`, `dismissed`, `step: "welcome"|"addAccount"|"discovery"|"done"`, `accountId: string|null`; actions `start()`, `goTo(step)`, `setAccountId(id)`, `finish()`. `start()` sets `active=true`, `dismissed=false`, and — with ≥1 existing account (Settings re-entry) — opens at `step="discovery"` with `accountId=` the first account; a true first run (zero accounts) opens at `step="welcome"`, `accountId=null`. `finish()` sets `active=false`; if `accountsStore.getState().accounts.length===0` also sets `dismissed=true`. Hook `useWizardStore(selector)` + exported `wizardStore` for imperative `getState()` calls.
- `src/components/wizard/first-run-wizard.tsx` (+ test) -- **NEW**. Full-frame surface (`fixed inset-0 z-50 bg-background`) rendering the current `step` with progress dots and a **persistent Skip control in the wizard chrome (a footer, not per-step)** so `LoginScreen`'s full-viewport layout can never push Skip below the fold. **Welcome:** intro + Get started. **Add Account:** a slim honest **no-homeserver fork** banner ABOVE (companion-stack docs via `COMPANION_STACK_DOCS_URL`, pointing at the in-step Beeper tab below — no fake sign-up), then `<LoginScreen addMode onDone={handleAddDone}/>` where `handleAddDone` reads `accountsStore` and, if count grew past the on-entry baseline, `setAccountId(newest)`+`goTo("discovery")`, else returns to Welcome. **Discovery:** resolve `accountId ?? firstAccount?.accountId`; if none show an "add an account first" note; else reuse `useBridgeCatalog()`+`useBridgeDiscovery(accountId)` and render a `BridgeCard` per discovered network (catalog-joined, same guard as `AccountBridges`) — `BridgeCard` self-drives the ack gate + `BridgeLoginSheet`. **Done:** success copy + Enter keeper → `finish()`. Esc handler opens a confirm `AlertDialog` (ask once) whose confirm calls `finish()`, and **stands down while a nested Radix overlay (`role="dialog"`/`"alertdialog"`) is open** so Escape closes the bridge-login Sheet / ack dialog first.
- `src/App.tsx` -- add wizard precedence + one-shot first-run auto-start. In `renderContent()`: after the `!hydrated` splash, `if (wizardActive) return <FirstRunWizard/>`. Add a boot `useEffect` (guarded by a `useRef` so it fires at most once) that starts the wizard when `hydrated && !hasAccount && postureChosen !== undefined && postureChosen !== null`. In the `!hasAccount` branch, when `postureChosen` is resolved and `wizardDismissed`, fall through to render `<AppShell/>` (empty inbox) instead of `<LoginScreen/>`; render the existing add-account overlay in the shell path so it is reachable from the empty-inbox footer too.
- `src/components/settings/settings-dialog.tsx` -- add a "Setup" row/section with a "Run setup again" `Button` that calls `wizardStore.getState().start()` then `onOpenChange(false)`.

## Tasks & Acceptance

**Execution:**
- [x] `src/lib/stores/wizard.ts` -- new session-scoped wizard store (active/dismissed/step/accountId + start/goTo/setAccountId/finish); no persistence, no secrets.
- [x] `src/components/wizard/first-run-wizard.tsx` -- full-frame stepper composing `LoginScreen addMode`, the no-homeserver fork, `useBridgeCatalog`+`useBridgeDiscovery`+`BridgeCard`, Done; progress dots, per-step Skip, Esc-asks-once confirm; add-account success detected via account-count growth.
- [x] `src/components/wizard/first-run-wizard.test.tsx` -- cover the I/O matrix: Welcome→addAccount advance, add-account success (count grows → discovery) vs cancel (→ Welcome), discovery renders `BridgeCard`s from mocked discovery+catalog, Skip on each step and Esc-confirm call `finish()`, Done calls `finish()`. Mock the IPC client + discovery/catalog hooks (do not hit Rust).
- [x] `src/App.tsx` -- wizard precedence branch + one-shot first-run auto-start effect + `dismissed`→`<AppShell/>` empty-inbox fall-through (with add-account overlay reachable).
- [x] `src/App.test.tsx` -- extend: wizard-active renders the wizard; first-run (zero accounts, posture resolved) auto-starts it; `dismissed` with zero accounts renders the shell not the login screen; sign-out-style zero-accounts without a wizard still renders the login screen.
- [x] `src/components/settings/settings-dialog.tsx` (+ its test) -- "Run setup again" entry calls `wizardStore.start()` and closes Settings.

**Acceptance Criteria:**
- Given a fresh install (zero accounts, posture chosen), when the app boots, then the First-Run Wizard replaces the whole frame (not the bare login screen) and walks Welcome → Add Account → Bridge discovery → per-Bridge login → Done.
- Given a prepared homeserver, when the user signs in and connects ≥1 discovered network inside the wizard, then they reach the Inbox with that bridge logged in without leaving the wizard or reading external docs.
- Given any step, when the user clicks Skip on every step (or confirms the Esc prompt), then the wizard closes and — with still zero accounts — lands in an empty Inbox showing the "Add an account" card, never trapping the user.
- Given a signed-in user, when they choose "Run setup again" in Settings, then the wizard re-opens over the shell and is fully re-runnable.
- Given `bun run check`, then Biome + tsc + vitest all pass; no Rust changes are introduced.

## Spec Change Log

_No bad_spec loopback occurred; this section is empty. Review-driven patches are recorded in the Review Triage Log below._

## Review Triage Log

### 2026-07-05 — Review pass (iteration 1)
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 1, medium 2, low 0)
- defer: 1
- reject: 16
- addressed_findings:
  - `[high]` `[patch]` `wizard.ts`/`first-run-wizard.tsx` — Settings "Run setup again" was effectively non-functional: `start()` reset to `welcome`/`accountId:null` and the discovery step was reachable only via an add-account *success*, so an existing user could never reach bridge setup for the account they already have (cancel looped back to Welcome). Fixed: `start()` opens directly at the `discovery` step for the first account when accounts exist (a true first run still starts at welcome) — consistent with the intent contract's "accountId defaults to first account" re-entry row. Locked with a re-entry test.
  - `[medium]` `[patch]` `first-run-wizard.tsx` — the wizard's `window` Escape listener popped the "Skip setup?" confirm even when a nested `BridgeLoginSheet` (`role="dialog"`) or risk-ack `AlertDialog` (`role="alertdialog"`) driven by `BridgeCard` was open, hijacking an Escape meant to close the child. Fixed: the handler stands down when any open Radix dialog/alertdialog is present in the DOM. Locked with a nested-overlay Escape test.
  - `[medium]` `[patch]` `first-run-wizard.tsx` — `LoginScreen` renders its own `flex h-screen` full-viewport layout, so mounting it above the no-homeserver fork + Skip pushed them ~100vh below the fold. Fixed: Skip hoisted into a persistent footer in the wizard chrome (shown on every non-terminal step); the no-homeserver fork rendered as a slim banner ABOVE the login card (copy no longer says "above"). Locked with a Skip-reachable + fork-visible test.
- deferred (1): `[medium]` `BridgeCard` shows a "Connect" action and a "Not set up" status word for a bridge whose discovered `status` is `loggedIn`/`configured` — pre-existing 6.1/6.2 behavior shared with the Bridges pane, surfaced (not caused) by this story's first-run framing. Logged to the deferred-work ledger.
- rejected (16, noise/by-design): count-growth-vs-concurrent-removal & re-login-upsert misread (spec's chosen heuristic, sound for single-user first run; re-entry no longer routes existing accounts through add-account); posture-read-failure auto-start (pre-existing honest FileVault-off default); `dismissed` session-stickiness after a later sign-out (by-design; the empty shell carries the add-account card); Continue-with-no-account & Done celebratory copy (discovery is only reachable with ≥1 account); unreachable `step`/`ProgressDots` default-branch guards; StrictMode double-listener (cleaned up by effect teardown); `finish()`/`start()` reading the `accountsStore` singleton (correct at call time); test-double contract notes; auto-start effect deps intentionally inert (ref-guarded, commented, tested).

## Design Notes

- **Why a dedicated `active` flag, not a `!hasAccount` derivation.** `LoginScreen` calls `addAccount` on success, flipping `hasAccount` true. If the wizard were gated on `!hasAccount` it would unmount the instant the Add-Account step succeeds — the discovery/login steps would never render. The session-scoped `active` flag makes the wizard a stable surface that outlives the account transition; `App` checks it *before* the `hasAccount` gate.
- **Add-account success vs cancel is disambiguated by account-count growth, not `onDone` alone.** In `addMode`, `LoginScreen` calls `onDone` on *both* success and cancel, and success calls `addAccount` synchronously *before* `onDone`. So `handleAddDone` compares `accountsStore.getState().accounts.length` against a baseline captured on step entry: grown ⇒ success (advance to discovery with the newest account, which `addAccount` appends last); unchanged ⇒ cancel (return to Welcome). This is load-bearing — a reviewer will otherwise read `onDone` as "cancel only".
- **Discovery + per-bridge login is pure composition.** `BridgeCard` already self-contains the risk-tier ack `AlertDialog` and the `BridgeLoginSheet` state machine (props are just `{network, accountId, status}`), so the wizard's discovery step is the same catalog-join + `BridgeCard` map that `AccountBridges` does — no bridge refactor, no ack/QR reimplementation.
- **`dismissed` only affects the zero-account landing, scoped to avoid a sign-out regression.** It is set solely by the wizard's own `finish()` with zero accounts, so a sign-out-of-last-account (Story 1.8) still returns to `<LoginScreen/>`. It is session-scoped — a relaunch with zero accounts legitimately re-offers the wizard.
- **Composing a full-viewport surface has two sharp edges (review iteration 1).** (1) `LoginScreen` owns a `flex h-screen` centered layout, so any wizard content placed *after* it falls below the fold — hence Skip lives in the persistent wizard footer (not per-step) and the no-homeserver fork is a slim banner *above* the login card. (2) The wizard's `window` Escape listener must not fight the Escape handling of the Radix `BridgeLoginSheet`/ack `AlertDialog` that `BridgeCard` portals to `document.body`; it stands down whenever an open `[role="dialog"]`/`[role="alertdialog"]` exists so the child closes first. (3) The whole point of Settings re-entry is bridges-for-an-existing-account, so `start()` skips straight to `discovery` when an account already exists — otherwise the strictly-linear welcome→addAccount→discovery path (discovery only reachable via an add *success*) traps re-entrants who don't want a second account.

## Verification

**Commands:**
- `bun run check` -- expected: Biome (no `any`, `import type`), tsc strict, and vitest all green, incl. the new `first-run-wizard` test and the extended `App`/`settings-dialog` tests.
- `bun run check:all` -- expected: still green; this story touches no Rust, so `check:rust`/`test:rust` are unaffected.

**Manual checks (if no CLI):**
- Fresh profile (no accounts): the wizard replaces the frame on launch; sign in → discovery lists bridges → connect one → Done → Inbox. Skip everything → empty Inbox with the footer "Add an account" card. Settings → "Run setup again" re-opens it.

## Auto Run Result

Status: done

**Summary:** Added a full-frame First-Run Wizard (Welcome → Add Account → Bridge discovery + per-Bridge login → Done) that composes the existing `LoginScreen` (addMode), `BridgeCard` (self-driving ack gate + `BridgeLoginSheet`), and the discovery/catalog hooks — driven by a new session-scoped `wizardStore`. It auto-opens once on genuine first run, is fully skippable (Skip in persistent chrome; Esc asks once and defers to nested overlays), re-enterable from Settings (landing on discovery for an existing account), and lands the user in the Inbox (empty inbox when everything is skipped). Pure frontend — no Rust changes.

**Files changed:**
- `src/lib/stores/wizard.ts` (new) — session-scoped wizard store; `start()` routes re-entry to discovery, first run to welcome; `finish()` dismisses to empty inbox only with zero accounts.
- `src/components/wizard/first-run-wizard.tsx` (new) — the stepper; composition of LoginScreen/BridgeCard/discovery hooks; persistent-footer Skip; Esc-asks-once with nested-overlay stand-down; honest no-homeserver fork banner.
- `src/components/wizard/first-run-wizard.test.tsx` (new) — I/O-matrix + patch coverage (re-entry→discovery, nested-Esc, Skip/fork visibility).
- `src/App.tsx` — wizard `active` precedence before the `hasAccount` gate; one-shot ref-guarded first-run auto-start; `dismissed`→empty-shell fall-through with the add-account overlay reachable.
- `src/App.test.tsx` — wizard-active render, first-run auto-start, dismissed→shell, sign-out→login.
- `src/components/settings/settings-dialog.tsx` (+ test) — "Run setup again" re-entry button.

**Review findings:** 3 patches applied (1 high: Settings re-entry could not reach bridge discovery; 2 medium: Escape hijacked nested overlays; LoginScreen `h-screen` hid Skip/fork below the fold) — all fixed and locked with tests. 1 deferred (pre-existing `BridgeCard` "Connect"/"Not set up" labeling for already-logged-in bridges → deferred-work ledger). 16 rejected as noise/by-design. No intent_gap, no bad_spec loopback.

**Verification:** `bun run check` green — Biome clean, tsc strict clean, vitest 683 passed (73 files); the tauri-free core check passed. `git status` confirms only TS files + this spec changed (no Rust).

**Residual risk:** The live add-account round-trip and a real bridge-login success inside the wizard depend on the underlying Epic 1–2 / 6.3 flows (already covered by their own stories); the wizard only composes them, and its seams are unit-tested against mocked login/discovery. Multi-account re-entry defaults discovery to the first account (per the intent contract) — a future account-picker is out of scope here.
