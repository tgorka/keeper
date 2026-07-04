---
title: 'Beeper Coverage Disclosure'
type: 'feature'
created: '2026-07-04'
status: 'done'
baseline_revision: '0f1d6a0f532f7d87a6d640288329933a292c7730'
final_revision: '294feb0b1b1d723e6f432d9ba6161a18e2871348'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** keeper can now sign in a Beeper account (Story 2.3), but Beeper's flagship "On-Device Connection" chats (e.g. WhatsApp bridged inside the official Beeper app) never reach `matrix.beeper.com`, so they are invisible to keeper — and to any Matrix client. FR-7/UX-DR17 require keeper to disclose this honestly before login completes and keep the disclosure permanently reachable, so a missing chat reads as a named limitation, not breakage.

**Approach:** Add a reusable coverage-disclosure card (exact copy, voice-rules compliant) rendered as an acknowledgment gate in the Beeper login flow after `loginBeeper` succeeds but before the account enters the inbox, and reachable afterwards from each Beeper account's row in the sidebar footer. Pure frontend: a Beeper account is identified by its `matrix.beeper.com` homeserver (the only signal — Beeper logins persist as `StoredSession::Password`). No Rust/IPC changes.

## Boundaries & Constraints

**Always:**
- The disclosure shows for Beeper accounts only, identified by `homeserverUrl` host `matrix.beeper.com`.
- In the login flow, the disclosure appears **after** `loginBeeper` resolves and **before** `addAccount`+`onDone` run; an explicit acknowledgment action is the only forward path into the inbox.
- The card names the specific broken expectation with the literal sentence "WhatsApp connected in the official Beeper app will not appear here." and names self-hosted Bridges as the parity path.
- Copy follows the voice rules (UX-DR10): sentence case, no exclamation marks, no "please"/softening, consequence named plainly, glossary nouns capitalized (Chat, Bridge, Account).
- The same disclosure content (one shared component) is used both in the login gate and in the permanent settings surface.
- Reuse existing shadcn primitives (`Dialog`, `Alert`/`Card`, `Button`) and existing patterns; no new deps.

**Block If:**
- The required exact disclosure copy or acknowledgment requirement cannot be satisfied with the available `AccountVm` fields — none anticipated.

**Never:**
- Do not add a Matrix/network/`thirdparty/protocols` coverage query or hungryserv probe (that is OQ-3 / later-epic work — this story is static, honest disclosure only).
- Do not add a `provider`/`kind` field to `AccountVm`/`StoredSession` or change any Rust/IPC/persistence — homeserver-host match is the correct available signal.
- Do not make the login-flow disclosure dismissible without acknowledgment (it is not a hint); do not build the Story 2.5 account switcher/settings panel — attach permanent access to the existing footer row.
- No secrets, no softening copy, no exclamation marks.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Beeper auth succeeds | `loginBeeper` resolves with `AccountVm` | Disclosure card renders; `addAccount`/`onDone` NOT yet called | No error expected |
| Acknowledge in login flow | user activates "I understand" | `addAccount(account)` then `onDone?.()` called once | No error expected |
| Beeper account row (footer) | account `homeserverUrl` = `https://matrix.beeper.com` | A "Beeper coverage" info control is shown; opening it renders the same disclosure in a Dialog | No error expected |
| Non-Beeper account row | account with any other homeserver | No coverage control rendered | No error expected |
| `isBeeperAccount` | `https://matrix.beeper.com/` → true; `https://example.org` → false | Correct boolean by homeserver host | Malformed/empty URL → false, never throws |

</intent-contract>

## Code Map

- `src/lib/beeper.ts` -- NEW. `BEEPER_HOMESERVER_HOST = "matrix.beeper.com"` and `isBeeperAccount(account: AccountVm): boolean` (parse `homeserverUrl` with `URL`, compare host case-insensitively; return `false` on parse failure — never throws).
- `src/components/auth/beeper-coverage-disclosure.tsx` -- NEW. Presentational `BeeperCoverageDisclosure` rendering the fixed copy (title + the literal WhatsApp sentence + the On-Device explanation + the self-hosted-Bridge parity path). Export the copy as named constants (`DISCLOSURE_TITLE`, `DISCLOSURE_BODY`/sentences) for reuse and test assertions. No acknowledgment control lives here — callers supply the surrounding chrome (login gate vs. Dialog).
- `src/components/auth/login-screen.tsx` -- `BeeperTab`: add `pendingAccount` state (`AccountVm | null`). In `handleVerify`, on `loginBeeper` success set `pendingAccount` (keep `flowStartedRef.current = false`) INSTEAD of calling `addAccount`/`onDone`. Add a render branch (after the `failed` branch, before the form): when `pendingAccount` is set, render `<BeeperCoverageDisclosure/>` plus a primary "I understand" button whose handler calls `addAccount(pendingAccount)` then `onDone?.()`. The existing unmount cleanup is unaffected (request id already consumed).
- `src/components/layout/account-footer.tsx` -- `AccountRow`: when `isBeeperAccount(account)`, render an extra icon Button (lucide `Info`, `aria-label` "Beeper coverage for {userId}") that opens a `Dialog` containing `BeeperCoverageDisclosure` and a Close. Wire it in both collapsed (Tooltip) and expanded layouts, mirroring the sign-out control's structure. Non-Beeper rows render nothing extra.
- `src/lib/beeper.test.ts`, `src/components/auth/beeper-coverage-disclosure.test.tsx`, `src/components/auth/login-screen.test.tsx` (extend), `src/components/layout/account-footer.test.tsx` (extend) -- tests per Acceptance.

## Tasks & Acceptance

**Execution:**
- [x] `src/lib/beeper.ts` -- Add `BEEPER_HOMESERVER_HOST` + `isBeeperAccount` (URL-host match, safe on malformed input).
- [x] `src/components/auth/beeper-coverage-disclosure.tsx` -- Shared presentational disclosure with exported copy constants; voice-rules compliant; includes the literal WhatsApp sentence and self-hosted-Bridge parity path.
- [x] `src/components/auth/login-screen.tsx` -- Gate `BeeperTab` completion behind the disclosure: hold the account after `loginBeeper`, render the disclosure + "I understand", and only then `addAccount`+`onDone`.
- [x] `src/components/layout/account-footer.tsx` -- Add the per-Beeper-account coverage info control opening the same disclosure in a Dialog (collapsed + expanded).
- [x] Tests -- `isBeeperAccount` (beeper/non-beeper/malformed); disclosure renders the exact required copy and contains no exclamation mark; login flow: after `loginBeeper` success the disclosure shows and `addAccount`/`onDone` are NOT called, then "I understand" calls both once; footer: a Beeper row exposes the coverage control which opens the disclosure, a password row does not.

**Acceptance Criteria:**
- Given the Beeper login flow, when authentication succeeds but before completion, then a disclosure card states plainly that On-Device Connection chats are invisible to keeper — naming the broken expectation ("WhatsApp connected in the official Beeper app will not appear here.") — points to self-hosted Bridges as the parity path, and requires an explicit acknowledgment before the account enters the inbox (FR-7).
- Given a connected Beeper Account, when the user opens that Account's controls in the sidebar footer, then the same disclosure component is permanently accessible there (FR-7), and it is not shown for non-Beeper accounts.
- Given the disclosure card, when it is rendered in either surface, then its copy follows the voice rules — sentence case, consequence-naming, no softening, no exclamation marks (UX-DR10).
- Given `bun run check`, then biome + tsc strict + vitest pass, including the new/updated tests; no Rust or IPC surface changed.

## Design Notes

**Gate placement.** `loginBeeper` already runs the full backend `add_account` (syncing Client + Keychain), so "authentication succeeds but before completion" maps to the window between `loginBeeper` resolving and the frontend `addAccount`+`onDone`. Holding the returned `AccountVm` in `pendingAccount` and rendering the disclosure there is the minimal, architecture-preserving gate. If the overlay is force-closed at the disclosure step, the account is already authenticated and persisted (not partial residue) and simply surfaces on the next session restore — acceptable; this story does not add an auth-rollback path.

**Beeper identity by homeserver.** After login a Beeper session is indistinguishable from a password session in `StoredSession` (both `Password`); the homeserver `matrix.beeper.com` is the only durable signal and is present on `AccountVm.homeserverUrl` for both fresh adds and restored accounts. Match on parsed host (not substring) so a lookalike like `matrix.beeper.com.evil.example` does not match.

**Copy (fixed, voice-rules compliant).** Title names the specific gap (not a vague "some chats may be unavailable"). Body leads with the literal required sentence, explains On-Device Connections run inside the official Beeper app and never reach the Beeper servers keeper syncs from, and ends with the parity path. Example shape (final wording lives in the component constants):
```
On-Device chats won't appear in keeper
WhatsApp connected in the official Beeper app will not appear here.
Beeper's On-Device Connections run inside the official Beeper app and never
reach the Beeper servers keeper syncs from, so keeper cannot see those chats.
Running your own Bridge is the path to parity.
```

## Verification

**Commands:**
- `bun run check` -- expected: biome lint + tsc strict + vitest all green, including `beeper.test.ts`, `beeper-coverage-disclosure.test.tsx`, and the extended login-screen / account-footer tests.

**Manual checks (if no CLI):**
- Add a Beeper account: after entering the emailed code, confirm the disclosure card appears with the exact WhatsApp sentence and the self-hosted-Bridge parity path, and that the account only appears in the inbox after acknowledging.
- In the sidebar footer, confirm the Beeper account row exposes the coverage control that reopens the same disclosure, and a password/OIDC account row does not.

## Spec Change Log

_No `bad_spec` loopback occurred; the spec was implemented as written._

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 0, low 4)
- defer: 1
- reject: 7
- addressed_findings:
  - `[low]` `[patch]` The "I understand" acknowledgment button had no idempotency guard while every other action button in the file (Send code / Verify / Sign out) does — a rapid double-click could fire `addAccount`/`onDone` twice before the overlay tore down (`addAccount` is idempotent, but `onDone` closing the overlay twice is not). Fixed: an `acknowledgedRef` gate makes the handler run once.
  - `[low]` `[patch]` `isBeeperAccount` matched `new URL(...).host`, which includes an explicit port, so a resolved homeserver like `matrix.beeper.com:443` would not be recognized as Beeper. Fixed: match `.hostname` instead; added a port test case.
  - `[low]` `[patch]` The dependence of Beeper identity on Beeper's `.well-known` continuing to resolve to `matrix.beeper.com` was undocumented in `beeper.ts`. Fixed: added a doc note on `BEEPER_HOMESERVER_HOST` calling out the coupling and pointing at the deferred durable-tag fix.
  - `[low]` `[patch]` Only the load-bearing WhatsApp/parity sentences were pinned by exact-string tests; `DISCLOSURE_TITLE`/`DISCLOSURE_EXPLANATION` (which carry the UX-DR10 "consequence named plainly" burden) could be silently reworded. Fixed: added a test asserting the rendered title + explanation.
- notes: Blind Hunter + Edge Case Hunter reviewed the full baseline→working-tree diff. Deferred 1: Beeper identity by homeserver host couples to Beeper's well-known (durable account-kind tag is a Rust/IPC change this frontend-only story excluded). Rejected 7: the gate-bypass-across-restart/tab-switch/unmount cluster (account is fully persisted by `loginBeeper` before the disclosure, so acknowledgment gates only the store insert) is real but **by-design and spec-documented** — FR-7's permanent-settings-access leg (the footer control, present on the restored account) is the durable backstop, and an auth-rollback path is explicitly out of scope; no-Cancel-at-disclosure is the same intentional "acknowledgment is the only forward path" decision; the two nested Radix `Dialog` roots work correctly (independent controlled `open` state, no `DialogTrigger` consuming the outer context) in a footer Story 2.5 will replace; the "Add account" lowercasing is pre-existing and the new copy is glossary-compliant; the X-plus-explicit-Close pairing is acceptable a11y; the spec's Design-Notes title example was explicitly non-canonical ("final wording lives in the component constants"); the MXID-in-aria-label matches the sibling sign-out control.

## Auto Run Result

Status: **done**

**Summary.** Implemented the Beeper coverage disclosure (FR-7, UX-DR17) as a pure frontend change. A single shared, voice-rules-compliant `BeeperCoverageDisclosure` component (exported copy constants; leads with the literal "WhatsApp connected in the official Beeper app will not appear here.", explains that Beeper's On-Device Connections run inside the official Beeper app and never reach the Beeper servers keeper syncs from, and names self-hosted Bridges as the parity path) is used in two surfaces: (1) an acknowledgment gate in `BeeperTab` — after `loginBeeper` resolves, the returned `AccountVm` is held in `pendingAccount` and the disclosure + "I understand" render instead of the form; `addAccount`/`onDone` run only on acknowledgment; (2) a permanent per-Beeper-account control in the sidebar footer (`BeeperCoverageControl`, an Info button opening the same disclosure in a Dialog), shown only for Beeper accounts. A Beeper account is identified by `isBeeperAccount` (`homeserverUrl` hostname === `matrix.beeper.com`), the only durable signal since Beeper logins persist as `StoredSession::Password`. No Rust/IPC/persistence changed.

**Files changed (one-line each).**
- `src/lib/beeper.ts` — NEW: `BEEPER_HOMESERVER_HOST` + `isBeeperAccount` (exact `hostname` match, safe on malformed/empty input, documents the well-known coupling).
- `src/components/auth/beeper-coverage-disclosure.tsx` — NEW: shared presentational disclosure (Alert-based) with exported copy constants.
- `src/components/auth/login-screen.tsx` — `BeeperTab` gates completion behind the disclosure (`pendingAccount` + "I understand" with an idempotency guard); `addAccount`/`onDone` deferred to acknowledgment.
- `src/components/layout/account-footer.tsx` — `BeeperCoverageControl` opening the disclosure Dialog, rendered per Beeper account (collapsed + expanded); sign-out behavior untouched.
- `src/lib/beeper.test.ts`, `src/components/auth/beeper-coverage-disclosure.test.tsx` — NEW tests.
- `src/components/auth/login-screen.test.tsx`, `src/components/layout/account-footer.test.tsx` — extended tests (disclosure gate; per-account footer control).
- `_bmad-output/implementation-artifacts/deferred-work.md` — one deferred entry (durable account-kind tag).

**Review findings breakdown.** intent_gap 0, bad_spec 0, patch 4 (all low), defer 1, reject 7. Patches: (low) idempotency guard on "I understand"; (low) match `.hostname` not `.host` so a port can't defeat identity (+ port test); (low) document the well-known dependency in `beeper.ts`; (low) pin `DISCLOSURE_TITLE`/`DISCLOSURE_EXPLANATION` in tests. Deferred 1: durable account-kind tag to decouple Beeper identity from well-known (needs Rust/IPC, out of this frontend-only story). Rejected 7 (by-design gate-bypass-across-restart with FR-7 permanent-access backstop; intentional no-Cancel-at-disclosure; benign nested Dialog roots in the throwaway footer; pre-existing "Add account" casing; acceptable X+Close a11y; non-canonical spec title example; sibling-consistent MXID aria-label).

**Verification.** `bun run check` PASS — biome clean (94 files), `tsc --noEmit` strict clean, vitest 205/205 (23 files), keeper-core tauri-free guard PASS. No Rust changes, so `check:rust`/`test:rust` untouched.

**Residual risks.** Beeper identity relies on Beeper's `.well-known` continuing to resolve to `matrix.beeper.com` (deferred: a durable account-kind tag). If a user completes Beeper auth but abandons before acknowledging (closes overlay / switches tab), the account — already persisted by `loginBeeper` — surfaces on the next session restore without having shown the pre-completion card; this is accepted by design, and FR-7's permanent settings-access control still exposes the disclosure on the restored account. The real-Beeper-account manual check and OQ-3 hungryserv exit check remain non-automatable here.
