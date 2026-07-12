---
title: 'Required iOS CI Gate and Release Hygiene'
type: 'chore'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '7a53cddb5acfa667bf8637f3fdf0b336c80d080b'
final_revision: 'c83b30616ef3700935b56a5486e76b0f1386b38c'
context:
  - '{project-root}/docs/project-context.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** The `iOS (compile check)` CI job exists and blocks by failure, but it is not recorded as a **required** PR status, and the release checklist plus contributor docs never mention iOS — so the port's integrity is remembered, not enforced.

**Approach:** Record the iOS job in the existing required-status-checks convention in `docs/release.md`, add an iOS release-hygiene checklist there, document the gate's scope and local reproduction in `README.md`, and keep the CI job's comment self-consistent with those docs — without changing the job's compile-only scope.

## Boundaries & Constraints

**Always:** English, honest voice; no Team IDs, credentials, provisioning profiles, or secrets in docs/repo/CI (placeholders only). The iOS job stays **compile-only** (`cargo check --workspace --target aarch64-apple-ios`) — no signing, no simulator, no Apple creds (AD-32). The recorded required-check name MUST be the CI job's exact `name:` string, `iOS (compile check)`, so branch protection binds. `docs/ios.md` `## Limitations` stays the single source of truth mirrored from `IOS_DISCLOSURE_LINES` — reference it, never duplicate or diverge it. iOS adds **no new network egress endpoints** (NFR-11 unchanged); state this, don't invent egress claims.

**Block If:** (a) The CI iOS job's `name:` in `.github/workflows/ci.yml` is not exactly `iOS (compile check)` (a mismatched name would silently fail to bind as a required check) — HALT with the discrepancy. (b) The promoted gate is **not green** on this tree (`cargo check --workspace --target aarch64-apple-ios` fails) — HALT, since the docs must not call a red check "required".

**Never:** Do not attempt to flip GitHub branch-protection/merge-queue settings via API/`gh`/UI — that is a repo-admin action; the deliverable is the *recorded configuration* in docs. Do not change the iOS job's steps or add signing/simulator/creds. Do not edit `docs/ios.md`, `docs/egress.md`, `tauri.conf.json`, `project.yml`, or any compile-seam / badge-FFI source. **No code changes** — the badge fix that keeps the iOS gate green already landed in commit `481d426`; this story is docs-only.

</intent-contract>

## Code Map

- `docs/release.md:180-197` -- `## Required status checks (branch protection)`: bulleted required checks mapped to CI jobs; the `iOS (compile check)` bullet sits at ~186-190 and the "These correspond to the … jobs" sentence (line 192) names `ios`. THE required-status deliverable.
- `docs/release.md:162-178` -- `## iOS release checklist`: intro (164-166) noting the CI job only compiles, then four `[ ]` items (15.3 IPA path, 15.2 docs + `## Limitations` mirror, NFR-15 cold-start with owner-confirmation status, NFR-11 no-new-egress).
- `README.md:35,56-61` -- `## Development`: the iOS compile-gate paragraph — compile-only scope (no signing/simulator/creds; blocks PRs) and the `cargo check --workspace --target aarch64-apple-ios` local repro from `src-tauri/`.
- `.github/workflows/ci.yml:47-61` -- the `ios:` job: `name: iOS (compile check)` (line 48), `runs-on: macos-latest`, comment (58-59) recording the required-status in `docs/release.md`, compile step (60) `cargo check --workspace --target aarch64-apple-ios`.
- `docs/ios.md:328` -- `## Limitations` (mirrors `IOS_DISCLOSURE_LINES`). Reference for the checklist's "docs/ios.md current" item; do not edit.
- `_bmad-output/planning-artifacts/epics.md:2316-2336` -- Story 15.4 ACs (FR-55 CI-required leg; AD-32 compile-only, recorded-not-YAML; AD-23 egress honesty). Citation source; do not edit.

## Tasks & Acceptance

**Execution:**
- [x] `docs/release.md` -- In `## Required status checks`, record a bullet for `iOS (compile check)` — `cargo check --workspace --target aarch64-apple-ios` (Rust, device-free compile gate; no signing/simulator) mapped to the `ios` job, and ensure the "correspond to … jobs" sentence names `ios`. -- FR-55: promote the compile check to a required PR status (AD-32, recorded not YAML-enforced).
- [x] `docs/release.md` -- Ensure the `## iOS release checklist` section (after the SM-3 sign-off block) holds four `[ ]` items: IPA build path exercised (Story 15.3 — `bun run verify:ios-ipa` + the `docs/ios.md` build recipe); `docs/ios.md` current with its `## Limitations` still one-to-one with the in-app "On this iPhone" disclosure (Story 15.2); NFR-15 cold-start (launch → interactive Unified Inbox) measured and recorded **with its owner-confirmation status** (authored 3 s bar, not yet gating — PRD §13.8 / Story 15.6); egress note that iOS adds no new endpoints (NFR-11 unchanged, `docs/egress.md`). -- release checklist gains iOS items.
- [x] `README.md` -- In `## Development`, document the iOS compile gate: its scope (compile-only, no signing/simulator/creds; blocks PRs) and the exact local reproduction `cargo check --workspace --target aarch64-apple-ios` run from `src-tauri/` (matching the CI command verbatim). -- FR-55: contributor docs state the gate's scope and how to reproduce locally.
- [x] `.github/workflows/ci.yml` -- Ensure the `ios:` job comment records the check as a required PR status in `docs/release.md` (no stale "wiring is Story 15.4" text); job `runs-on`/steps stay unchanged (compile-only). -- keep the workflow self-consistent with the docs.

**Acceptance Criteria:**
- Given `docs/release.md`, when read, then `## Required status checks` lists `iOS (compile check)` as a required check mapped to the `ios` job, described as compile-only (no signing/simulator), and the "correspond to … jobs" sentence names `ios`.
- Given `docs/release.md`, when read, then an iOS release-checklist section holds the four items with correct cross-references (15.3 IPA path, 15.2 docs + limitations mirror, NFR-15 recorded with owner-confirmation status, NFR-11 no-new-endpoints), all as unchecked `[ ]` boxes.
- Given `README.md`, when read, then it states the iOS gate's compile-only scope and the exact local reproduction command run from `src-tauri/`.
- Given `.github/workflows/ci.yml`, then the `ios:` job `name:` is exactly `iOS (compile check)`, its comment records the required-status in `docs/release.md`, and its step remains `cargo check --workspace --target aarch64-apple-ios` (compile-only, unchanged).
- Given the promoted gate, when `cargo check --workspace --target aarch64-apple-ios` runs from `src-tauri/`, then it succeeds (green) — the tree honors the "required check is green before docs call it required" premise.
- Given the full tree, when scanned, then only the `docs/release.md` / `README.md` / `.github/workflows/ci.yml` hygiene content carries this story, and no real Team ID / `.p12` / `syt_` / profile UUID appears (placeholders only).

## Spec Change Log

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 1: (high 0, medium 1, low 0)
- reject: 12: (high 0, medium 0, low 12)
- addressed_findings:
  - `[low]` `[patch]` README local-repro command dropped `--workspace`, diverging from the CI gate command `cargo check --workspace --target aarch64-apple-ios`. Harmless today (`src-tauri/` is a virtual workspace, so both forms compile the same members) but a latent green-locally / red-in-CI hazard once a non-default member is added. Fixed: added `--workspace` to `README.md` so the documented reproduction matches CI verbatim; aligned the spec's task and Code Map command strings.
- notes:
  - deferred (1, medium): the four pre-existing required-check bullets in `docs/release.md` ("License firewall", "Frontend", "Rust", "Tauri build") do not verbatim match their CI job `name:` values, so a repo admin copy-pasting the labels into branch protection could fail to bind them. Not caused by this story (the added `iOS (compile check)` row matches its job `name:` exactly, per the Block-If guard). Logged to `deferred-work.md`.
  - rejected findings were verified against sources and dropped: NFR-15's `PRD §13.8` / `Story 15.6` refs and the `Story 15.2` / `Story 15.3` pointers are correct (`epics.md`, `sprint-status.yaml`); the egress checklist item is honest to-verify framing (`docs/egress.md` has no iOS-specific egress, consistent with "no new endpoints"); the recorded-vs-enforced distinction is already stated by the section intro ("A repo admin must, under Settings → Branches …"); the rest were low-value wording nitpicks.

## Design Notes

**Why docs, not automation:** GitHub enforces required checks via **branch-protection settings, not YAML** — `docs/release.md` already records them as prose (Epic 11 pattern), so this story extends that convention; the toggle itself is a repo-admin action outside automation scope. Enforcement already exists (the `ios` job blocks by failure); "required" only prevents merging a red or missing result. GitHub binds the requirement to the job's `name:` value, so the doc must say `iOS (compile check)` verbatim — hence the Block-If guard. And a red required check blocks every merge, so verification compiles the promoted gate on this tree before the doc calls it required.

**Gate-green precondition already satisfied:** an earlier attempt surfaced an iOS compile break (Story 14.3's badge port calling the `#[cfg(desktop)]` `WebviewWindow::set_badge_count`). The coordinator-authorized fix — badge via `UNUserNotificationCenter::setBadgeCount` (`objc2-user-notifications`, second audited function-level FFI exception) — **already landed in commit `481d426`** on this baseline. This story is docs-only and must not re-touch the badge source; it only *verifies* the gate is green.

**NFR-15 posture (do not over-claim):** the 3 s cold-start is an *authored* bar; owner confirmation is required before it becomes release-gating (Story 15.6). The checklist records the measurement **and its confirmation status**, never a hard fail — mirroring the existing NFR-3 "measure & flag" item.

## Verification

**Commands:**
- `cargo check --workspace --target aarch64-apple-ios` (from `src-tauri/`) -- expected: success — the gate being called required is green on this tree.
- `git grep -n 'iOS (compile check)' docs/release.md README.md .github/workflows/ci.yml` -- expected: recorded in all three.
- `grep -nE '[A-Z0-9]{10}' docs/release.md README.md` -- expected: only benign env-var names / placeholders, no real Team ID / secret / profile UUID.

**Manual checks (if no CLI):**
- Read `docs/release.md` and `README.md` end to end: `iOS (compile check)` recorded as required and mapped to the `ios` job; four iOS checklist items present with correct 15.2/15.3/NFR-15/NFR-11 cross-refs and owner-confirmation wording; README states gate scope + `src-tauri/` local repro; honest voice; no secrets.

## Auto Run Result

**Status:** done (dev-auto re-drive, 2026-07-11)

**Context — why this was a re-drive:** The attempt-1 spec was frozen `blocked` (authored against a red iOS gate) and kept halting re-drives even after commit `481d426` turned the gate green. The coordinator's HEAD commit `7a53cdd` ("Drop stale 15-4 spec/result…") intended to delete the stale spec + result so the re-drive could regenerate on the current baseline, but its diff only *appended* text and deleted nothing. This run completed that authorized drop (`git rm` of the stale spec + result note, both verified stale), then regenerated this spec fresh from `epics.md` / `epic-15-context.md` on baseline `7a53cdd`.

**Summary of change:** Story 15.4 is a docs-only release-hygiene chore. Its deliverables were already implemented and committed on baseline `7a53cdd` (docs in `481d426`; the badge FFI fix that keeps the iOS gate green also in `481d426`). Implementation verified every task's end-state holds on disk (no code edits needed). Review applied one patch.

**Files changed this run (since baseline `7a53cdd`):**
- `README.md` — review patch: local iOS-gate reproduction command now `cargo check --workspace --target aarch64-apple-ios`, matching the CI job verbatim (was missing `--workspace`).
- `_bmad-output/implementation-artifacts/spec-15-4-required-ios-ci-gate-and-release-hygiene.md` — regenerated fresh spec (replaced the stale `blocked` attempt-1 spec at the canonical slug).
- `_bmad-output/implementation-artifacts/bmad-dev-auto-result-15-4-required-ios-ci-gate-and-release-hygiene.md` — deleted (stale attempt-1 result note; part of the coordinator's authorized drop).
- `_bmad-output/implementation-artifacts/deferred-work.md` — one defer entry: pre-existing required-check label drift in `docs/release.md` (four bullets don't match their CI job `name:`; a branch-protection binding hazard not caused by this story).

The story's own deliverable content (`docs/release.md` iOS required-check bullet + `## iOS release checklist`, `README.md` gate scope, `.github/workflows/ci.yml` comment) was already committed in `481d426` and is unchanged this run except the README `--workspace` patch.

**Review findings breakdown:** 2 reviewers (adversarial general + edge-case hunter, Opus). intent_gap 0, bad_spec 0. Patches applied: 1 (README `--workspace`, low). Deferred: 1 (required-check label drift, medium, pre-existing). Rejected: 12 (verified-correct cross-refs and low-value framing nitpicks). `review_loop_iteration` stayed 0 (no repair loopback).

**Follow-up review recommended:** false — the only review-driven change was a single localized, low-consequence doc patch.

**Verification performed (on the patched tree):**
- `cargo check --workspace --target aarch64-apple-ios` (from `src-tauri/`) → `Finished` (green). Gate honors the "required check is green before docs call it required" premise.
- `git grep 'iOS (compile check)'` → recorded in `ci.yml:48`, `README.md:56`, `docs/release.md:164` & `:190`.
- `grep -nE '[A-Z0-9]{10}' docs/release.md README.md` → only benign env-var names (`APPLE_CERTIFICATE`…) and the word `ASSUMPTIONS`; no real Team ID / `.p12` payload / `syt_` token / profile UUID.
- Block-If guards both clear: CI `ios:` job `name:` is exactly `iOS (compile check)`; the promoted gate is green.

**Residual risks / handback to coordinator:**
- `sprint-status.yaml` still shows `15-4-…: backlog`. This automated run does not own sprint-status transitions (orchestrator/coordinator domain) — flip `15-4-required-ios-ci-gate-and-release-hygiene` to `done`.
- Making the required check actually enforced is a repo-admin branch-protection toggle (GitHub Settings → Branches), outside automation scope; docs record the intended configuration only.
- Deferred: audit the four pre-existing required-check labels in `docs/release.md` against their CI job `name:` values so branch protection binds them (see `deferred-work.md`).
