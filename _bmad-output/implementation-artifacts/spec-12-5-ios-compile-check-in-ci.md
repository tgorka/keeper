---
title: 'iOS Compile Check in CI'
type: 'feature'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: []
baseline_revision: b81a45ad677ac760f6fde8353571627cff5e08ca
final_revision: a6d46d41fb215c407c68eecace3ed17d86467a18
---

<intent-contract>

## Intent

**Problem:** The iOS port (Stories 12.1–12.4) compiles today only because developers run `cargo check --target aarch64-apple-ios` by hand. Nothing in CI guards the compile seam, so a desktop-only change that breaks a `#[cfg(target_os = "ios")]` gate or a target-gated dependency would land on `main` silently and only surface when someone next builds for iOS.

**Approach:** Add a permanent `ios` job to `.github/workflows/ci.yml` that runs `cargo check --workspace --target aarch64-apple-ios` on the existing `macos-latest` runner for every push and PR — compile-only, no signing, no simulator build, no Apple credentials. The job blocks by failure; wiring it as a *required* branch-protection status is deferred to Story 15.4.

## Boundaries & Constraints

**Always:** The job runs on `macos-latest` (same runner class as the other jobs) and installs the `aarch64-apple-ios` Rust target via the toolchain action's `targets:` input. It runs `cargo check` (not build, not clippy, not test) from `src-tauri/` so the whole cargo workspace — including the `keeper` Tauri shell crate — is type-checked for iOS. Desktop CI jobs (`frontend`, `rust`, `licenses`, `build`) stay byte-identical. Follow the existing job style: `Swatinem/rust-cache@v2` with `workspaces: src-tauri`.

**Block If:** `cargo check --workspace --target aarch64-apple-ios` fails from a clean checkout of the current branch (that would mean the port is already broken on `main`, which is an intent contradiction, not a CI-wiring task).

**Never:** No signing, no `.p12`/provisioning profiles, no `TAURI_APPLE_DEVELOPMENT_TEAM` or any Apple credential/secret in the workflow. No simulator boot, no `tauri ios build`, no Xcode archive. Do not build the frontend (`bun install`/`bun run build`) in this job — `cargo check` needs no web assets. Do not add this job to the `build` job's `needs`. Do not flip the check to a required status here (that is Story 15.4). Do not add the Intel-only `x86_64-apple-ios` target (DW-106) — CI is Apple-Silicon `macos-latest`, which uses `aarch64-apple-ios`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Green on current main | Workspace compiles for iOS | `ios` job succeeds | No error expected |
| Broken cfg seam | A `#[cfg(target_os = "ios")]` path fails to compile | `ios` job fails, blocking the run | Job exits non-zero; spot-checked once (see Verification) |
| Desktop-only regression | A change compiles on macOS but breaks the iOS target | `ios` job fails while `rust`/`build` pass | Non-zero exit surfaces the port break |

</intent-contract>

## Code Map

- `.github/workflows/ci.yml` -- the whole change lives here. Add a new `ios` job alongside `frontend`/`rust`/`licenses`/`build` (`:12-83`). Model it on the `rust` job (`:26-45`): same `runs-on`, `checkout`, `dtolnay/rust-toolchain@stable` (add `targets: aarch64-apple-ios`), and `Swatinem/rust-cache@v2` with `workspaces: src-tauri`.
- `rust-toolchain.toml` -- reference only. Already pins `targets = ["aarch64-apple-ios", "aarch64-apple-ios-sim"]`; the job's explicit `targets:` input makes the CI install unambiguous and independent of file-driven auto-install. Do not modify.
- `src-tauri/Cargo.toml` -- reference only. Virtual workspace root; `cargo check` from `src-tauri/` checks all members (`keeper-core`, `keeper`). `keeper`'s `build.rs` (`tauri_build::build()`) resolves `tauri.conf.json` from the crate root without needing built frontend assets — already proven by the desktop `rust` job running clippy without `bun install`.

## Tasks & Acceptance

**Execution:**
- [x] `.github/workflows/ci.yml` -- add an `ios` job: `runs-on: macos-latest`; steps = `actions/checkout@v4`, `dtolnay/rust-toolchain@stable` with `targets: aarch64-apple-ios`, `Swatinem/rust-cache@v2` with `workspaces: src-tauri`, then `cargo check --workspace --target aarch64-apple-ios` with `working-directory: src-tauri`. No `needs:`, no bun, no secrets. Add a short comment noting it is compile-only and that required-status wiring is Story 15.4. -- gives every push/PR a permanent iOS compile gate that blocks by failure.

**Acceptance Criteria:**
- Given the current branch checked out clean, when the `ios` job runs `cargo check --workspace --target aarch64-apple-ios` from `src-tauri/`, then it finishes green (FR-55 CI leg; AD-32).
- Given the workflow triggers, when any push to `main` or any pull request occurs, then the `ios` job is scheduled (it inherits the workflow-level `on: [push to main, pull_request]` triggers) and runs independently of `frontend`/`rust`/`build`.
- Given the job definition, then it contains no signing step, no simulator/`tauri ios build` step, no Apple credential or secret reference, and no frontend build — compile-only (AD-32).
- Given a deliberately broken iOS-only compile seam, when the same `cargo check --target aarch64-apple-ios` runs, then it exits non-zero (spot-checked locally per Verification; scratch-branch/PR evidence per the epic AC folds into the PR).
- Given phase sequencing, then the job is not wired as a required branch-protection status here — that is left to Story 15.4; the job exists and blocks by failure from this story onward.
- Given the desktop gates, when `bun run check:all` and the existing CI jobs run, then they are unchanged and green (this story touches only `ci.yml`).

## Spec Change Log

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 0
- reject: 12
- addressed_findings:
  - none

Two adversarial reviewers (Blind Hunter + Edge Case Hunter, Opus, parallel, no prior context) produced 12 deduplicated findings; all rejected. Rationale: every finding is either (a) an explicit, documented design decision in this spec — plain `cargo check` (no `--all-targets`, so host-only `keeper-core/tests/` are not cross-compiled for iOS, matching AD-32's compile-only gate); explicit `targets: aarch64-apple-ios` input as belt-and-suspenders, with the device triple covering the identical `cfg(target_os = "ios")` seam the sim triple would (sim/`x86_64` out of scope per the Never boundary); compile-only `check` not linking (link/codegen errors are the on-device gate in 12.6/15.x); and required-status wiring deliberately deferred to Story 15.4 so the job blocks by failure as a check-run only — or (b) a pre-existing repo-wide CI convention that applies identically to all five jobs and is therefore not this change's problem: unpinned `@stable` channel, actions pinned by tag not SHA, no `timeout-minutes`, no `paths:` filter, and the per-job checkout/toolchain/cache block. The flagged `Swatinem/rust-cache` key collision between the `rust` and `ios` jobs is not real — the action keys on `github.job` (distinct `ios` vs `rust`) and the two jobs populate different target subtrees (`target/aarch64-apple-ios/` vs the host `target/debug/`), so neither eviction nor cross-target artifact serving occurs. No bad_spec loopback; `review_loop_iteration` stays 0.

## Design Notes

Why `cargo check` from `src-tauri/` and not `--manifest-path`: the existing `rust` job `cd`s into `src-tauri` via `working-directory`, and prior stories' verification (12.2–12.4) all used `cd src-tauri && cargo check --target aarch64-apple-ios`. Matching that exact invocation keeps the CI gate identical to the loop developers already run. `--workspace` is explicit (redundant with the virtual manifest but clearer about "the whole workspace" in the AC).

Why not `--all-targets` or clippy: the AC and AD-32 specify a compile-only `cargo check`. `--all-targets` would additionally compile test/bench targets for iOS (pulling dev-deps that need not cross-compile), and clippy is heavier than the gate requires. Keep it minimal; desktop clippy already covers lint.

Why explicit `targets: aarch64-apple-ios` on the action despite `rust-toolchain.toml` pinning it: belt-and-suspenders — the action installs the target deterministically regardless of whether rustup's file-driven auto-install fires during the cache-restore/first-invoke ordering.

Job skeleton (~10 lines):
```yaml
  ios:
    name: iOS (compile check)
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: aarch64-apple-ios
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
      # Compile-only iOS gate (AD-32): no signing, no simulator, no Apple creds.
      # Blocks by failure; required-status wiring is Story 15.4.
      - run: cargo check --workspace --target aarch64-apple-ios
        working-directory: src-tauri
```

## Verification

**Commands:**
- `cd src-tauri && cargo check --workspace --target aarch64-apple-ios` -- expected: `Finished` (mirrors exactly what the new CI job runs; proves the "green on current main" AC locally).
- Broken-seam spot-check (satisfies the epic's "deliberately broken cfg seam demonstrably fails it"): temporarily insert a compile error behind `#[cfg(target_os = "ios")]` in a `keeper` source file, run `cd src-tauri && cargo check --target aarch64-apple-ios` and confirm it exits non-zero, then revert so the tree is clean. Record the observed failure in the run result.
- `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml'))"` (or equivalent) -- expected: the workflow YAML parses and now contains a top-level `jobs.ios` key.
- `git diff --stat` -- expected: only `.github/workflows/ci.yml` changed (plus the spec/deferred-work artifacts).

**Manual checks:** Actual GitHub Actions execution (job scheduling on PR, green/red status) is observed when the branch is pushed and a PR is opened — inherent to CI config; not headless-automatable here. The enforceable local gate is the `cargo check --target aarch64-apple-ios` pass plus the broken-seam failure spot-check.

## Auto Run Result

Status: done

**Summary of implemented change.** Story 12.5 adds a permanent, compile-only iOS gate to CI. A new `ios` job in `.github/workflows/ci.yml` runs `cargo check --workspace --target aarch64-apple-ios` on the existing `macos-latest` runner for every push to `main` and every pull request — no signing, no simulator build, no Apple credentials, no frontend build. It installs the `aarch64-apple-ios` Rust target via `dtolnay/rust-toolchain@stable`'s `targets:` input and reuses `Swatinem/rust-cache@v2` with `workspaces: src-tauri`, mirroring the existing `rust` job. Desktop jobs (`frontend`, `rust`, `licenses`, `build`) are byte-identical. The job blocks by failure as a check-run; wiring it as a *required* branch-protection status is deferred to Story 15.4 (noted in an inline comment).

**Files changed.**
- `.github/workflows/ci.yml` — added the `ios` job (16 insertions) between `rust` and `licenses`; no other job touched, `build`'s `needs: [frontend, rust]` unchanged.

**Review findings breakdown.** Two adversarial reviewers (Blind Hunter + Edge Case Hunter, Opus, parallel, no prior context) → 12 deduplicated findings, **all rejected**, 0 patches, 0 defers, 0 bad_spec, 0 intent_gap. Rejections fall into: documented spec decisions (plain `cargo check` not `--all-targets`; explicit device target with the sim triple covering the identical `cfg(target_os="ios")` seam; compile-only check not linking; required-status deferred to 15.4) and pre-existing repo-wide CI conventions applying identically to all five jobs (unpinned `@stable`, tag-not-SHA action pins, no `timeout-minutes`, no `paths:` filter, per-job checkout/toolchain/cache block). The flagged `rust`/`ios` rust-cache key collision is not real: the action keys on `github.job` and the two jobs write different target subtrees. No loopback; `review_loop_iteration` stayed 0.

**Verification performed.**
- `cd src-tauri && cargo check --workspace --target aarch64-apple-ios` — green (`Finished` in 4.15s; last crate `keeper v0.1.0`), proving the "green on current main" AC.
- Broken-seam spot-check — inserting `#[cfg(target_os = "ios")] const _BROKEN: u32 = "not a number";` in `crates/keeper/src/lib.rs` made `cargo check --target aarch64-apple-ios` fail with `error[E0308]: mismatched types` (exit 101); edit fully reverted. Satisfies the epic AC that a deliberately broken cfg seam demonstrably fails the gate.
- Workflow YAML parses and now exposes `jobs.ios` (jobs: frontend, rust, ios, licenses, build).
- `git diff --stat` — only `.github/workflows/ci.yml` changed (plus orchestrator-managed spec artifact); working tree otherwise clean.

**Residual risks.** (1) Actual GitHub Actions scheduling and green/red status on a real PR are observed only once the branch is pushed — inherent to CI config, not headless-automatable here. (2) `cargo check` gates compilation, not link/codegen; iOS link-time errors (e.g. `security-framework` FFI) surface in the on-device build folded into Stories 12.6/15.x. (3) The gate is not yet a required status — a red `ios` does not block merge until Story 15.4 wires branch protection.
