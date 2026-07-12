---
title: '15-5 Paid-Program Decision Gate Recorded'
type: 'chore'
created: '2026-07-11'
baseline_revision: '023d647bf37ed9b6edca2299ff7b292b617cc34d'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
final_revision: 'b6728c70cb1f72e38b25f8a2d496fa8ad312c503'
context:
  - '{project-root}/docs/ios.md'
  - '{project-root}/docs/constraints-and-limitations.md'
warnings:
  - oversized
---

<intent-contract>

## Intent

**Problem:** The $99/yr Apple Developer Program is the sole unlock for iOS push (APNs), the Notification Service Extension, TestFlight, App Groups, and AltStore PAL notarization — yet keeper defers it. That deferral is real and load-bearing (ios.md and the in-app "On this iPhone" disclosure already say "background notifications await a future decision"), but it lives only in planning artifacts (PRD §13.5, architecture Deferred). Without a durable, discoverable decision record it reads as an omission a future contributor might "fix" by wiring project-run push infrastructure — which keeper's client-only + NFR-11 posture forbids.

**Approach:** Create a durable project decisions ledger at `docs/decisions.md` and record the paid-program deferral as its defining entry: what the program uniquely unlocks, the single trigger that opens the gate (push becomes a product goal), the PRD-level constraint it then forces (push must ride an operator's gateway / Beeper's / a user-run Sygnal — never project infrastructure), and the cheap-now mitigations this phase already paid for (single `data_dir()` root, shelved Plan B triggers). Add short discoverability pointers from the two existing docs that already touch the deferral. No code changes.

## Boundaries & Constraints

**Always:** English, honest voice. Every claim in the record cross-references its source (PRD §13.5, §13.8, architecture Deferred / AD-29, NFR-11, FR-65) so the doc and planning artifacts never diverge. Markdown relative links must resolve. The record must state the constraint that push may never ride project/keeper infrastructure (NFR-11) — it is the load-bearing reason the gate exists.

**Block If:** the required unlocks/trigger/constraint/mitigations cannot be sourced from the PRD/architecture as written (they can — quoted in Design Notes). Do not invent new decisions, dates, or owners beyond what the planning artifacts record.

**Never:** No code changes — no files under `src/`, `src-tauri/`, no config, no CI. No secrets, team ids, or credentials. Do NOT edit the four mirrored bullets in `docs/ios.md`'s Limitations list or `IOS_DISCLOSURE_LINES` in `about-section.tsx` (those are code-sourced and mirror-locked); pointers go in surrounding prose only. Do not reopen the NFR-15 cold-start question (that is Story 15.6). Do not decide to *adopt* the paid program — this records a deferral, not a purchase.

</intent-contract>

## Code Map

- `docs/decisions.md` -- **new**; the project decisions ledger. Holds the D-1 paid-program deferral record.
- `docs/constraints-and-limitations.md` -- existing honest-limitations doc; add a one-line pointer to the ledger under a deferred/deliberate-policy spot.
- `docs/ios.md` -- existing iOS doc; Limitations section prose (after the mirrored bullets, ~line 344–353) gets a one-line pointer to the ledger for "the future decision" it already names.
- `_bmad-output/planning-artifacts/prds/.../prd.md` §13.5, §13.8 -- read-only source of the decision text (see Design Notes).
- `_bmad-output/planning-artifacts/architecture/.../ARCHITECTURE-SPINE.md` Deferred, AD-29 -- read-only source for the mitigations/cross-refs.

## Tasks & Acceptance

**Execution:**
- [x] `docs/decisions.md` -- create the decisions ledger with a short preamble (what this file is: durable, project-level decisions with planning cross-refs) and entry **D-1 — Paid Apple Developer Program: deferred by decision**. D-1 must contain, each cross-referenced: (a) **Unlocks** — APNs push, the NSE with its 24 MB memory ceiling and App-Group store-layout implications, TestFlight, App Groups, AltStore PAL notarization (PRD §13.5, spine Deferred); (b) **Opening trigger** — push becomes a product goal (PRD §13.5, §13.8 open item 2); (c) **Constraint it forces** — push must ride a homeserver operator's gateway, Beeper's, or a user-run Sygnal, never project/keeper infrastructure (NFR-11, PRD §13.5); (d) **Cheap-now mitigations already paid for** — the single `Platform::data_dir()` root makes the future App Group move a path change, not a migration (AD-29, FR-65), and Plan B's revisit triggers stay recorded (PRD §13.8); (e) **Status/owner** — deferred; revisit owned by PM/owner when push demand is real (PRD §13.8). -- fulfills all three ACs.
- [x] `docs/constraints-and-limitations.md` -- add a one-line pointer to `docs/decisions.md` (D-1) so the deferral is discoverable from the canonical limitations doc. -- discoverability.
- [x] `docs/ios.md` -- in the Limitations prose (not the mirrored bullet list), add a one-line pointer: the "future decision" the disclosure names is recorded in `docs/decisions.md`. -- discoverability without touching mirror-locked lines.

**Acceptance Criteria:**
- Given the new decisions ledger, when D-1 is read, then it names every unique unlock (APNs push, NSE + 24 MB ceiling + App-Group store-layout implications, TestFlight, App Groups, AltStore PAL notarization), the opening trigger (push becomes a product goal), and the forced constraint (push rides an operator/Beeper/user-Sygnal gateway, never project infrastructure), each with a PRD/architecture cross-reference.
- Given the cheap-now mitigations, when D-1 is read, then it records the single `data_dir()` root making the App Group move a path change (AD-29) and that Plan B's revisit triggers stay recorded (PRD §13.8).
- Given scope discipline, when the change is reviewed, then `git diff --name-only` shows only files under `docs/` (no `src/`, `src-tauri/`, CI, or config), and no secrets/team-ids/credentials appear anywhere in the diff.
- Given the discoverability pointers, when `docs/constraints-and-limitations.md` and `docs/ios.md` are read, then each links to `docs/decisions.md` with a resolving relative path, and the four mirror-locked `docs/ios.md` limitation bullets are byte-for-byte unchanged.

## Spec Change Log

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 0, low 3)
- defer: 0
- reject: 11: (high 0, medium 0, low 11)
- addressed_findings:
  - `[low]` `[patch]` Preamble claimed cross-refs make the ledger and artifacts "never diverge" — an unenforceable guarantee in a doc whose pitch is honesty. Softened to "so a reader can always trace a decision back to where it was made."
  - `[low]` `[patch]` D-1 Constraint bullet coined "project/keeper infrastructure", a compound term absent from the sources. Aligned to the source wording "project infrastructure" (PRD §13.5 / NFR-11).
  - `[low]` `[patch]` The paid-program open question was cited by fragile ordinal ("§13.8 open item 2", twice). Re-cited by content ("§13.8, the paid-program-timing open question") so a reorder in the PRD can't rot the reference.
  - Rejected (11, all low): FR-65 "wrong citation" (FR-65's text explicitly states the single-`data_dir`-root/App-Group-path-change claim — the reviewer saw only its title); 24 MB "unanchored" (bullet cites PRD §13.5, which states it); Glossary capitalization (the `docs/` convention, e.g. `egress.md`, is lowercase); missing ledger→consumer back-links, "Superseded/Revisited" mechanic, explicit title convention (all premature format design for a one-entry ledger); AltStore "for EU distribution" date hedge (faithful copy of PRD §13.5); NFR-11 paraphrase-not-quoted (accurate + cited); "not a purchase" wording (defensible); ios.md pointer antecedent coupling and trigger "auto-open" reading (the Status/owner bullet already names the human decider).

## Design Notes

Verbatim source anchors the record must stay faithful to (quote-derived, do not restate more strongly):

- **PRD §13.5:** "The $99/yr program is the sole unlock for APNs push, the NSE (background notification decryption, with its 24 MB memory ceiling and App-Group store-layout implications — kept cheap now by the single data-dir root, FR-65), TestFlight, App Groups, and AltStore PAL notarization for EU distribution. The gate opens only when push becomes a product goal — and it then forces a PRD-level question that keeper's client-only constraint makes hard: push must ride a homeserver operator's gateway, Beeper's, or a user-run Sygnal — never project infrastructure (NFR-11)."
- **Architecture Deferred (spine):** "APNs push + NSE architecture — behind the paid-program decision gate (PRD §13.5); AD-29's single data-dir root keeps the App Group move cheap; the Sygnal/gateway question is PRD-level when it opens."
- **AD-29:** "All account state remains under the one `Platform::data_dir()` root so a future App Group container move (NSE era) is a path change, not a migration."
- **PRD §13.8 (Plan B):** "Plan B (UniFFI + native SwiftUI shell) stays shelved. Revisit triggers … (a) the blank-webview bug class proves unfixable across Tauri releases; (b) NSE work begins — noting the NSE is a Rust+Swift target under Plan A regardless." **§13.8 Open item 2:** "Paid-program timing — the §13.5 gate itself. Owner: PM/owner, when push demand is real."
- **NFR-11:** "keeper contacts only user-configured Homeservers/Bridges, Beeper's API when a Beeper Account is added, and the signed-update endpoint. … Egress surface is documented and diffable per release."

Keep D-1 tight (a decision record, not an essay). The ledger preamble should make room for future entries (D-2…) without implying any exist. Pointers are one line each — they link, they do not restate D-1.

## Verification

**Commands:**
- `git diff --name-only` -- expected: only paths under `docs/` (`docs/decisions.md`, `docs/constraints-and-limitations.md`, `docs/ios.md`); nothing under `src/`, `src-tauri/`, `.github/`, or config.
- `git diff docs/ios.md` -- expected: additions are prose only; the four mirrored Limitations bullets (lines beginning "keeper syncs and notifies", "No self-hosted bridge runner", "No global summon hotkey", "Updates arrive by reinstalling") are unchanged.
- `grep -n "syt_\|BEGIN .*PRIVATE KEY\|developmentTeam\|TEAM_ID" docs/decisions.md docs/ios.md docs/constraints-and-limitations.md` -- expected: no secret/team-id matches.

**Manual checks:**
- Open `docs/decisions.md` D-1 and confirm all five elements (unlocks, trigger, constraint, mitigations, status/owner) are present, each with a cross-reference, and the constraint explicitly forbids project/keeper-run push infrastructure.
- Confirm the relative links in `docs/constraints-and-limitations.md` and `docs/ios.md` resolve to `docs/decisions.md`.

## Auto Run Result

Status: **done**

### Summary

Recorded the paid Apple Developer Program deferral as an on-the-record decision, closing Epic 15's single deliberate deferral (PRD §13.5). Created a durable project decisions ledger (`docs/decisions.md`) whose defining entry, **D-1**, captures what the $99/yr program uniquely unlocks (APNs push; the NSE with its 24 MB ceiling and App-Group store-layout implications; TestFlight; App Groups; AltStore PAL notarization), the single trigger that opens the gate (push becomes a product goal), the PRD-level constraint it then forces (push must ride an operator's / Beeper's / a user-run Sygnal gateway — never keeper's own infrastructure, per NFR-11), and the cheap-now mitigations this phase already paid for (single `Platform::data_dir()` root → App Group move is a path change, AD-29/FR-65; Plan B shelved with revisit triggers, PRD §13.8). Two one-line discoverability pointers link the existing limitations and iOS docs into the ledger. No code changed.

### Files changed

- `docs/decisions.md` (new) — project decisions ledger + entry D-1 (paid-program deferral, five cross-referenced elements).
- `docs/constraints-and-limitations.md` — one-line pointer to D-1 under "Should not do (deliberate policy)".
- `docs/ios.md` — one-line prose pointer to D-1 (the four mirror-locked "On this iPhone" limitation bullets left byte-for-byte unchanged).
- `_bmad-output/implementation-artifacts/spec-15-5-…md` — this spec (planning/review record).
- Removed the stale `bmad-dev-auto-result-15-5-…md` (its premises — dirty tree, blocked 15-4 — were invalidated once 15-4 landed in `023d647`).

### Review findings breakdown

- Two adversarial reviewers (general + edge-case). Verdict: factually accurate, well-sourced, no blockers.
- **Patches applied (3, all low):** softened an unenforceable "never diverge" guarantee in the preamble; aligned a coined "project/keeper infrastructure" term to the source's "project infrastructure"; re-cited a fragile "§13.8 open item 2" ordinal by content.
- **Deferred:** 0. **Rejected:** 11 (all low) — incl. a "wrong FR-65 citation" claim that was itself wrong (FR-65's text states the single-`data_dir`-root property), plus several premature-format-design suggestions for a one-entry ledger.
- Follow-up review recommended: **false** (three localized low-severity wording/citation fixes).

### Verification performed

- `git status --porcelain` — content changes confined to `docs/` + BMAD artifacts; nothing under `src/`, `src-tauri/`, `.github/`, or config.
- `git diff docs/ios.md` — additions are prose-only; the four mirror-locked Limitations bullets unchanged.
- Secret scan of the three docs — no `syt_`/private-key/team-id values.
- Manual: D-1 contains all five required elements each cross-referenced; both pointer links resolve to sibling `docs/decisions.md`.

### Residual risks

- The ledger↔artifact and pointer↔D-1 consistency is prose-enforced, not test-enforced (accepted for a docs artifact; the softened preamble no longer over-claims). No regulatory/date hedge on the "AltStore PAL for EU distribution" unlock — a faithful copy of PRD §13.5, revisited if that source changes.
