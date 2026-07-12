---
title: 'Required iOS CI Gate and Release Hygiene'
type: 'chore'
created: '2026-07-11'
status: blocked
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '4f2bd5b4132ce5f97192a06a23baa70029e5b7a1'
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
- [ ] `docs/release.md` -- In `## Required status checks`, add a bullet `- **iOS (compile check)** — `cargo check --workspace --target aarch64-apple-ios` (Rust, device-free compile gate; no signing/simulator).` and extend the "These correspond to the ... jobs" sentence to include `ios`. -- FR-55: promote the compile check to a required PR status (AD-32, recorded not YAML-enforced).
- [ ] `docs/release.md` -- Add a new `## iOS release checklist` section (after the SM-3 sign-off block) with four `[ ]` items: IPA build path exercised (Story 15.3 — `bun run verify:ios-ipa` + the `docs/ios.md` build recipe); `docs/ios.md` current with its `## Limitations` still one-to-one with the in-app "On this iPhone" disclosure (Story 15.2); NFR-15 cold-start (launch → interactive Unified Inbox) measured and recorded **with its owner-confirmation status** (authored 3 s bar, not yet gating — PRD §13.8 / Story 15.6); egress note that iOS adds no new endpoints (NFR-11 unchanged, `docs/egress.md`). -- release checklist gains iOS items.
- [ ] `README.md` -- In `## Development`, document the iOS compile gate: its scope (compile-only, no signing/simulator/creds; blocks PRs) and the exact local reproduction `cargo check --target aarch64-apple-ios` run from `src-tauri/`. -- FR-55: contributor docs state the gate's scope and how to reproduce locally.
- [ ] `.github/workflows/ci.yml` -- Replace the stale `# ... required-status wiring is Story 15.4.` comment on the `ios:` job with one noting the check is a required PR status recorded in `docs/release.md`. Comment-only; do not alter the job's `runs-on`/steps. -- keep the workflow self-consistent with the docs.

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

**Status:** blocked (dev-auto run 2026-07-11)

**Blocking condition:** The story's own Design Notes + Verification require the promoted
`iOS (compile check)` gate to be **green on this tree** before the docs call it a *required*
status. It is **red**. The prior "block resolution" commit `4f2bd5b`
("Resolve story 15-4 block: iOS badge via UNUserNotificationCenter FFI") is **defective**:
`git show --stat 4f2bd5b` changes only two markdown files (this spec + the dev-auto result
note) — its diff touches **zero source files**. The coordinator-authorized badge-FFI code
fix was never applied, so the compile-seam call at `crates/keeper/src/ipc.rs:676`
(`window.set_badge_count(...)`, desktop-only) still stands and the iOS target does not
compile.

**Reproduced (from `src-tauri/`):**
```
cargo check --workspace --target aarch64-apple-ios
error[E0599]: no method named `set_badge_count` found for struct `tauri::WebviewWindow<R>`
   --> crates/keeper/src/ipc.rs:676
error: could not compile `keeper` (lib) due to 1 previous error
```
(`aarch64-apple-ios` rustup target is installed — not a toolchain issue.)

**Why this halts the run:** Fixing the gate requires the coordinator-authorized code change
recorded in `bmad-dev-auto-result-15-4-required-ios-ci-gate-and-release-hygiene.md`
(replace the desktop-only `WebviewWindow::set_badge_count` call with
`UNUserNotificationCenter.setBadgeCount` via `objc2-user-notifications`, a second audited
function-level `#[allow(unsafe_code)]` FFI exception, audit inventory updated in
`docs/constraints-and-limitations.md`). That is a substantial unsafe-FFI change with a new
dependency (cargo-deny license firewall) and a policy/audit update — and this story's
**read-only intent-contract explicitly forbids code changes** and limits scope to
`docs/release.md`, `README.md`, `.github/workflows/ci.yml`. An unattended dev step must not
silently override the frozen contract to land that change. This is a CRITICAL contradiction
between the frozen spec and reality — it needs coordinator/human resolution.

**Resolution options (for the coordinator / `bmad-loop-resolve`):**
- (a) Re-apply the intended code fix from commit `4f2bd5b` to
  `crates/keeper/src/ipc.rs:659-678` (badge via `UNUserNotificationCenter`), update the
  unsafe-FFI audit inventory, confirm `cargo check --target aarch64-apple-ios` +
  `cargo deny check` pass, then re-drive 15.4's docs; **or**
- (b) split the badge FFI fix into its own story and relax this story's "the promoted gate
  is green on this tree" premise (record the gate as required with a known-red follow-up),
  so the docs can honestly ship.

**Work completed and left in the working tree (uncommitted, correct — do not redo):**
The docs deliverables are done and all five acceptance criteria (docs content) pass.
Modified files: `docs/release.md` (required-checks bullet for `iOS (compile check)` mapped
to the `ios` job + "correspond to … jobs" now naming `ios`; new `## iOS release checklist`
after the SM-3 block with the four cross-referenced items), `README.md` (`## Development`
now documents the gate's compile-only scope + the `cargo check --target aarch64-apple-ios`
local repro from `src-tauri/`), `.github/workflows/ci.yml` (stale
"required-status wiring is Story 15.4" comment on the `ios:` job replaced with a
"recorded as a required PR status check in docs/release.md" note; job steps unchanged).
Block-If guard passed: the CI iOS job `name:` is exactly `iOS (compile check)`. No secrets
introduced. These edits are held pending the block resolution.

---

### Update — dev-auto run 2026-07-11 (re-invoked)

**Status: still `blocked` (spec frontmatter is stale), but the block is RESOLVED in the
tree — only a coordinator status-flip remains. No dev work to do.**

The coordinator resolved the block via **option (a)** and committed it as
`481d426` ("Fix iOS badge port: UNUserNotificationCenter::setBadgeCount (FR-62)"),
which landed *after* the defective `4f2bd5b`. That single commit:

- **Fixed the root cause:** `crates/keeper/src/ipc.rs` now calls
  `UNUserNotificationCenter::setBadgeCount` (safe `objc2-user-notifications` binding, iOS
  16+) instead of the `#[cfg(desktop)]` `WebviewWindow::set_badge_count`; the unused
  `IOS_MAIN_WINDOW_LABEL` was dropped. New dep added under the cargo-deny firewall.
- **Committed all of story 15.4's docs deliverables** in the same commit:
  `docs/release.md` (required-check bullet + `## iOS release checklist`), `README.md`
  (gate scope + `src-tauri/` local repro), `.github/workflows/ci.yml` (stale comment
  replaced with "recorded as a required PR status check in docs/release.md").

**Verified on HEAD (`481d426`) this run:**
- The promoted gate is **GREEN**: `cargo check --workspace --target aarch64-apple-ios`
  (from `src-tauri/`) → `Finished` (aarch64-apple-ios target installed). The story's
  "gate must be green before docs call it required" premise now holds.
- All four deliverable files present and correct on HEAD (release.md required-checks +
  iOS checklist, README iOS gate + local repro, ci.yml comment, ipc.rs badge fix).
- Block-If guard still passes: CI job `name:` is exactly `iOS (compile check)`.
- Working tree clean; nothing left to implement — every acceptance criterion is met by
  committed content.

**Why this run HALTs instead of proceeding:** the spec's frozen `status` is `blocked`.
An unattended dev-auto step must not self-promote a frozen CRITICAL block to `done`/review
— resolving/closing a block is a coordinator / `bmad-loop-resolve` action. The coordinator
committed the code+docs fix but did not flip the spec status, so automation hands back.

**Action needed (coordinator / `bmad-loop-resolve`):** flip this spec's status to `done`
(work is complete and committed on `481d426`), or to `in-review` if a formal dev-auto
review pass is wanted. Consider bumping `baseline_revision` from `4f2bd5b` to `481d426`.
No further code or docs changes are required.

