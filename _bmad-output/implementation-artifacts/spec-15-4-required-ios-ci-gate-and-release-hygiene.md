---
title: 'Required iOS CI Gate and Release Hygiene'
type: 'chore'
created: '2026-07-11'
status: 'blocked'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: 'acfe0e13042ae5831949091efb0b5309cd3de0cb'
context:
  - '{project-root}/docs/project-context.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** The `iOS (compile check)` CI job exists and blocks by failure, but it is not recorded as a **required** PR status, and the release checklist plus contributor docs never mention iOS — so the port's integrity is remembered, not enforced.

**Approach:** Record the iOS job in the existing required-status-checks convention in `docs/release.md`, add an iOS release-hygiene checklist there, document the gate's scope and local reproduction in `README.md`, and refresh the now-stale "wiring is Story 15.4" comment on the CI job — without changing the job's compile-only scope.

## Boundaries & Constraints

**Always:** English, honest voice; no Team IDs, credentials, provisioning profiles, or secrets in docs/repo/CI (placeholders only). The iOS job stays **compile-only** (`cargo check --workspace --target aarch64-apple-ios`) — no signing, no simulator, no Apple creds (AD-32). The recorded required-check name MUST be the CI job's exact `name:` string, `iOS (compile check)`, so branch protection binds. `docs/ios.md` `## Limitations` stays the single source of truth mirrored from `IOS_DISCLOSURE_LINES` — reference it, never duplicate or diverge it. iOS adds **no new network egress endpoints** (NFR-11 unchanged); state this, don't invent egress claims.

**Block If:** The CI iOS job's `name:` in `.github/workflows/ci.yml` is not exactly `iOS (compile check)` (a mismatched name would silently fail to bind as a required check) — HALT with the discrepancy.

**Never:** Do not attempt to flip GitHub branch-protection/merge-queue settings via API/`gh`/UI — that is a repo-admin action; the deliverable is the *recorded configuration* in docs. Do not change the iOS job's steps or add signing/simulator/creds. Do not edit `docs/ios.md`, `docs/egress.md`, `tauri.conf.json`, `project.yml`, or any compile-seam source. No code changes.

</intent-contract>

## Code Map

- `docs/release.md:162-178` -- `## Required status checks (branch protection)`: bulleted required checks mapped to CI jobs (`licenses`/`frontend`/`rust`/`build`); the `iOS (compile check)` job is currently absent. THE required-status deliverable.
- `docs/release.md:139-160` -- `## Perf and reliability sign-off (SM-3)`: existing release-time checklist (NFR-1/3/6); the new iOS checklist section slots after it.
- `README.md:35-54` -- `## Development` / Quality gates: dev prerequisites and `bun run` gates; no iOS gate documented. Add scope + local repro here.
- `.github/workflows/ci.yml:47-61` -- the `ios:` job; comment at 58-59 says "required-status wiring is Story 15.4" (now stale). Update the comment only; leave steps unchanged.
- `docs/ios.md:328` -- `## Limitations` (mirrors `IOS_DISCLOSURE_LINES`). Reference for the checklist's "docs/ios.md current" item; do not edit.
- `_bmad-output/planning-artifacts/epics.md:113,120` -- NFR-11 (egress honesty) and NFR-15 (cold start <3 s, owner-confirmation required before gating). Citation source; do not edit.

## Tasks & Acceptance

**Execution:**
- [x] `docs/release.md` -- In `## Required status checks`, add a bullet `- **iOS (compile check)** — `cargo check --workspace --target aarch64-apple-ios` (Rust, device-free compile gate; no signing/simulator).` and extend the "These correspond to the ... jobs" sentence to include `ios`. -- FR-55: promote the compile check to a required PR status (AD-32, recorded not YAML-enforced).
- [x] `docs/release.md` -- Add a new `## iOS release checklist` section (after the SM-3 sign-off block) with four `[ ]` items: IPA build path exercised (Story 15.3 — `bun run verify:ios-ipa` + the `docs/ios.md` build recipe); `docs/ios.md` current with its `## Limitations` still one-to-one with the in-app "On this iPhone" disclosure (Story 15.2); NFR-15 cold-start (launch → interactive Unified Inbox) measured and recorded **with its owner-confirmation status** (authored 3 s bar, not yet gating — PRD §13.8 / Story 15.6); egress note that iOS adds no new endpoints (NFR-11 unchanged, `docs/egress.md`). -- release checklist gains iOS items.
- [x] `README.md` -- In `## Development`, document the iOS compile gate: its scope (compile-only, no signing/simulator/creds; blocks PRs) and the exact local reproduction `cargo check --target aarch64-apple-ios` run from `src-tauri/`. -- FR-55: contributor docs state the gate's scope and how to reproduce locally.
- [x] `.github/workflows/ci.yml` -- Replace the stale `# ... required-status wiring is Story 15.4.` comment on the `ios:` job with one noting the check is a required PR status recorded in `docs/release.md`. Comment-only; do not alter the job's `runs-on`/steps. -- keep the workflow self-consistent with the docs.

**Acceptance Criteria:**
- Given `docs/release.md`, when read, then `## Required status checks` lists `iOS (compile check)` as a required check mapped to the `ios` job, described as compile-only (no signing/simulator), and the "correspond to ... jobs" sentence names `ios`.
- Given `docs/release.md`, when read, then an iOS release-checklist section holds the four items with correct cross-references (15.3 IPA path, 15.2 docs + limitations mirror, NFR-15 recorded with owner-confirmation status, NFR-11 no-new-endpoints), all as unchecked `[ ]` boxes.
- Given `README.md`, when read, then it states the iOS gate's scope and the exact local reproduction command run from `src-tauri/`.
- Given `.github/workflows/ci.yml`, when diffed, then only the `ios:` job comment changed; its steps remain `cargo check --workspace --target aarch64-apple-ios` (compile-only, unchanged).
- Given the full diff, when scanned, then only `docs/release.md`, `README.md`, and `.github/workflows/ci.yml` changed; and no real Team ID / `.p12` / `syt_` / profile UUID appears (placeholders only).

## Design Notes

**Why docs, not automation:** GitHub enforces required checks via **branch-protection settings, not YAML** — `docs/release.md` already records them as prose (Epic 11 pattern), so this story extends that convention; the toggle itself is a repo-admin action outside automation scope. Enforcement already exists (the `ios` job blocks by failure); "required" only prevents merging a red or missing result. GitHub binds the requirement to the job's `name:` value, so the doc must say `iOS (compile check)` verbatim — hence the Block-If guard. And a red required check blocks every merge, so verification compiles the promoted gate on this tree before the doc calls it required.

**NFR-15 posture (do not over-claim):** the 3 s cold-start is an *authored* bar; owner confirmation is required before it becomes release-gating (Story 15.6). The checklist records the measurement **and its confirmation status**, never a hard fail — mirroring the existing NFR-3 "measure & flag" item.

## Verification

**Commands:**
- `cargo check --workspace --target aarch64-apple-ios` (from `src-tauri/`) -- expected: success — the gate being promoted to required is green on this tree.
- `git diff --name-only` -- expected: only `docs/release.md`, `README.md`, `.github/workflows/ci.yml`.
- `grep -nE '[A-Z0-9]{10}' docs/release.md README.md` -- expected: no real Team ID / secret / profile UUID (placeholders only).

**Manual checks (if no CLI):**
- Read `docs/release.md` and `README.md` end to end: `iOS (compile check)` recorded as required and mapped to the `ios` job; four iOS checklist items present with correct 15.2/15.3/NFR-15/NFR-11 cross-refs and owner-confirmation wording; README states gate scope + `src-tauri/` local repro; honest voice; no secrets.

## Auto Run Result

Status: **blocked**

Blocking condition: **iOS compile gate is red at baseline — cannot promote a failing check to a required PR status.**

### What was completed (correct, in the working tree — uncommitted)

All three documentation/CI edits were made exactly per spec and independently verified; all five acceptance criteria are met:

- `docs/release.md` — added `iOS (compile check)` to `## Required status checks` (mapped to the `ios` job, compile-only) and a new `## iOS release checklist` section with the four iOS items (15.3 IPA path, 15.2 docs/limitations mirror, NFR-15 cold-start-with-owner-confirmation, NFR-11 no-new-endpoints).
- `README.md` — documented the iOS gate's scope + local repro (`cargo check --target aarch64-apple-ios` from `src-tauri/`).
- `.github/workflows/ci.yml` — refreshed only the stale `ios:`-job comment (job scope unchanged).
- `git diff --name-only` → exactly those three files; no secrets/Team IDs introduced.

### Why blocked (pre-existing regression, out of this story's scope)

The story's Verification requires `cargo check --workspace --target aarch64-apple-ios` to succeed, and the story's whole purpose is to make that check a **required** merge gate. It does **not** compile on this tree (baseline `acfe0e1`, clean tree, Tauri 2.11.5, iOS target installed):

```
error[E0599]: no method named `set_badge_count` found for struct `tauri::WebviewWindow<R>`
   --> crates/keeper/src/ipc.rs:676:14
error: could not compile `keeper` (lib) due to 1 previous error
```

**Root cause (confirmed in the Tauri source):** `WebviewWindow::set_badge_count` / `Window::set_badge_count` live in `#[cfg(desktop)]` impl blocks in Tauri 2.11.5 (`webview_window.rs:1912` gate over the method at `:2157`; `window/mod.rs` mirror). They are **desktop-only** and do not exist on `aarch64-apple-ios`. Story **14.3**'s badge port (`crates/keeper/src/ipc.rs:659-677`, inside `#[cfg(target_os = "ios")] impl Platform for IosPlatform`) calls this desktop-only API, so the iOS target has been **uncompilable since Story 14.3** was committed. The `iOS (compile check)` CI job has therefore been red on `main` for ~7 stories (14.4→15.3); the automation loop did not catch it because it commits directly to `main` without gating on CI results.

**Why this blocks 15.4 specifically:** promoting a currently-red compile check to a *required* PR status would, the moment branch protection is flipped, block **every** future merge until the iOS build is fixed. Recording it as required while it is red is dishonest and actively harmful — it violates the story's own green-before-required premise.

### Prerequisite work needed before 15.4 can complete (coordinator decision)

Make the iOS target compile again — a separate bug/story, not a docs change (this spec is explicitly "no code changes / never edit source"). The iOS badge path in `crates/keeper/src/ipc.rs::IosPlatform::set_badge_count` must stop calling the desktop-only Tauri API. Design options for the coordinator/dev to choose:

1. **cfg the iOS badge to a documented honest no-op** (badge deferred on iOS) — smallest, but drops the all-accounts badge on the OS icon that 14.3 aimed for; must reconcile with 14.3's AC and the "On this iPhone" disclosure.
2. **Reach `UIApplication.applicationIconBadgeNumber` via objc2** behind the `Platform` port — restores the badge, but is unsafe FFI requiring a `// SAFETY:` comment and an entry in the `docs/constraints-and-limitations.md` audit inventory (per the 2026-07-11 policy amendment).
3. **A Tauri mobile badge API / plugin** if one is available at the pinned version — investigate.

Recommended path: open a bug story to fix the iOS badge compile (option 2 is closest to 14.3's intent), land it so `cargo check --target aarch64-apple-ios` is green, then re-drive story 15.4 (its docs edits above are already done and correct and can be kept or re-applied).

### Working-tree state at HALT

`docs/release.md`, `README.md`, `.github/workflows/ci.yml` carry the (correct) 15.4 edits; `spec-15-4-…md` is new. No commits, branches, or pushes were made. The next run / coordinator should decide whether to keep these edits in place or stash them while the prerequisite badge fix lands.
