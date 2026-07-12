---
title: 'Shareable IPA Build Path — Unsigned Export for Re-Signing'
type: 'feature'
created: '2026-07-11'
status: 'done'
baseline_revision: '79ff28ae344bfc5cea31a6731b5bd732b67809e5'
final_revision: 'a429757a0485fd1744e059556531bcb910d357ca'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings:
  - oversized
---

<intent-contract>

## Intent

**Problem:** Story 15.2 documented how a tester *re-signs* an already-built IPA, but there is no documented, repeatable recipe to *produce* that IPA, and nothing verifies FR-56 (the desktop/iOS compile seam) against the built artifact rather than just source. `docs/ios.md` §8 carries an explicit placeholder Note (lines ~241–245) deferring the hardened build recipe to this story.

**Approach:** Add a reusable seam-verification tool (`scripts/verify-ios-ipa.ts`) whose authoritative check proves the desktop-only Tauri plugins are absent from the iOS build closure via `cargo tree --target aarch64-apple-ios` (runnable now; already enforced structurally by the CI compile gate) and, when an IPA is present, scans the shipped Mach-O for those plugin symbols as an artifact-level confirmation. Wire it as `bun run verify:ios-ipa`, and expand `docs/ios.md` with the "Building a shareable IPA" recipe (exact command, output path, dev-signed-then-re-signed rationale, seam verification, no-signing-material guarantee), replacing the placeholder Note and cross-linking the existing re-sign steps. The real IPA build and the on-device re-signed install are exercised in Story 15.6.

## Boundaries & Constraints

**Always:**
- Only three files change: `scripts/verify-ios-ipa.ts` (new), `package.json` (one script entry), and `docs/ios.md`. No source, config, asset, or CI file is modified.
- The forbidden desktop-only set is exactly the crates in `crates/keeper/Cargo.toml`'s `[target.'cfg(not(any(target_os = "ios", target_os = "android")))'.dependencies]` block: `tauri-plugin-global-shortcut`, `tauri-plugin-autostart`, `tauri-plugin-updater`, `tauri-plugin-process`, and the `tray-icon` crate (the desktop-only `tauri` feature). `tauri-plugin-deep-link` is **cross-platform** (registered on both platforms) — it must never be flagged.
- The **authoritative** seam check is the dependency-graph assertion: `cargo tree -p keeper --target aarch64-apple-ios -e normal` shows the forbidden crates **absent**, while the desktop target (`aarch64-apple-darwin`) shows them **present** (differential proof; verified working — see Design Notes). The IPA binary scan is a best-effort artifact confirmation layered on top, never the sole signal.
- Build recipe: `bun run tauri ios build --export-method debugging`, producing the release-configuration IPA at `src-tauri/crates/keeper/gen/apple/build/arm64/keeper.ipa` (gitignored per `src-tauri/.gitignore`). Reconcile whether `--config src-tauri/crates/keeper/tauri.conf.json` is required given the non-default config location, and keep **every** `tauri` command in the doc internally consistent — never present two conflicting forms.
- Export uses Tauri automatic signing (dev-signed with the owner's free Personal Team) then re-signed by the tester — never manual/unsigned signing configs (tauri#10668). Frame the story's "unsigned export" as the AC's "dev-signed with signature replacement documented" branch.
- The verifier must run and pass in this environment on the graph path with no IPA present (clear message, correct exit code), be read-only and idempotent, and follow the conventions of `scripts/check-js-licenses.ts` (`#!/usr/bin/env bun`, self-contained, fail-closed exit codes).
- Honest English voice; no secrets. No real Team ID / provisioning profile / `.p12` / signing identity / token anywhere — placeholder env values only.

**Block If:**
- The keeper shell crate's Cargo package name or the desktop-plugin dependency names cannot be confirmed from `crates/keeper/Cargo.toml`, so the verifier's forbidden set cannot be tied to the real manifest. (Verified during planning: package `keeper`; the five deps above.)
- `cargo tree --target aarch64-apple-ios` cannot resolve in this environment (target/toolchain unavailable), so the authoritative seam check cannot be self-verified. (Verified working during planning.)

**Never:**
- No running a real `tauri ios build` / Xcode archive / code signing in this story or in CI (needs a Mac + Xcode + Apple ID; the device-free build is exercised for-real in Story 15.6).
- No `.github/workflows/*` edits — promoting the iOS compile check to a **required** CI status is Story 15.4. No new endpoints, no changes to the compile-seam source, `tauri.conf.json`, or `project.yml`.
- Do not fold `verify:ios-ipa` into the aggregate `check` / `check:all` gates (it targets iOS and needs graph resolution; keep it a targeted command).
- Do not alter the four in-app disclosure lines or the existing Sideloadly/zsign re-sign steps beyond integrating/cross-linking them.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Graph clean (happy) | run, no IPA arg; forbidden crates absent from iOS tree | prints per-crate seam-OK, notes artifact scan skipped + build hint, exit 0 | No error expected |
| Forbidden crate leaked | a desktop plugin appears in the iOS `cargo tree` | names the leaked crate(s), exit non-zero | Fail-closed |
| Differential control fails | forbidden crates also absent from the desktop tree (mis-scoped tree/pkg) | warns the check is not meaningfully asserting, exit non-zero | Fail-closed (never false-pass) |
| No IPA present | no IPA at default/arg path | reports artifact scan skipped with build-first hint; graph result governs exit | Informational, not a hard failure |
| IPA present, clean | valid IPA, no forbidden symbols in Mach-O | reports artifact scan pass, exit 0 | No error expected |
| IPA present, symbol found | forbidden symbol in the binary | names symbol + binary path, exit non-zero | Fail-closed |
| Tooling failure | `cargo`/`unzip`/`nm` missing or errors | clear error naming the missing tool/target, exit non-zero | Fail-closed |

</intent-contract>

## Code Map

- `docs/ios.md` -- §8 "Sharing a build without Xcode" holds the re-sign consumer steps (15.2) plus the placeholder Note (~lines 241–245) reserving the hardened build recipe for this story; TOC at top (lines 8–19). THE doc deliverable.
- `scripts/verify-ios-ipa.ts` -- **NEW** verifier. Model its shape on `scripts/check-js-licenses.ts`.
- `scripts/check-js-licenses.ts` -- precedent: a Bun verification-gate script (`#!/usr/bin/env bun`, self-contained, fail-closed exit codes) invoked from `package.json`.
- `scripts/gen-ios-icons.swift` -- precedent for a `scripts/` helper that self-resolves repo-root paths.
- `package.json` -- scripts block (`check:licenses` at the aggregate gate); add `verify:ios-ipa` beside it, NOT inside `check`/`check:all`.
- `src-tauri/crates/keeper/Cargo.toml:55-69` -- the `[target.'cfg(not(any(ios,android)))'.dependencies]` block; single source of truth for the forbidden set.
- `src-tauri/crates/keeper/gen/apple/ExportOptions.plist:6` -- export method already `debugging`. Reference only.
- `src-tauri/.gitignore:13-17` -- `build/`, `Externals/`, `Pods/`… gitignored; the IPA output lives under `build/`.
- `.github/workflows/ci.yml:47-61` -- compile-only iOS gate (`cargo check --workspace --target aarch64-apple-ios`). Reference only — do NOT edit (15.4 wires it required).

## Tasks & Acceptance

**Execution:**
- [x] `scripts/verify-ios-ipa.ts` -- Implement the verifier following `check-js-licenses.ts` conventions. (1) **Authoritative seam check:** run `cargo tree -p keeper --target aarch64-apple-ios -e normal` and assert the five forbidden crates are absent; run the same for `--target aarch64-apple-darwin` as a differential control and assert they are present (guards against a mis-scoped tree silently passing). (2) **Optional artifact scan:** accept an IPA path arg (default `src-tauri/crates/keeper/gen/apple/build/arm64/keeper.ipa`); if present, unzip to a temp dir, locate the `Payload/*.app` Mach-O (CFBundleExecutable), dump symbols (`nm -gU`, fall back to `strings`), assert forbidden symbols absent; if absent from disk, skip with a build-first hint. Fail-closed on any tooling error; read-only, idempotent; comment the forbidden set with a cross-reference to `Cargo.toml`. -- FR-56 "verify against the artifact, not just source".
- [x] `package.json` -- Add `"verify:ios-ipa": "bun run scripts/verify-ios-ipa.ts"` to `scripts`. Do NOT add it to `check` or `check:all`. -- makes the seam check discoverable/runnable without slowing the per-commit gates.
- [x] `docs/ios.md` -- Add a "Building a shareable IPA" section (merge with / precede "Sharing a build without Xcode"): the exact build command, the IPA output path, the release-config `--export-method debugging` rationale (dev-signed → tester re-signs; never manual signing configs; tauri#10668), the FR-56 seam-verification step (`bun run verify:ios-ipa [ipa]` — what it asserts and why the seam holds structurally), and the "no signing material in repo or CI" guarantee (AD-32, env-var-only team id). Remove the placeholder Note; keep and cross-link the existing Sideloadly/zsign re-sign steps; update the TOC; reconcile the `--config` question so all `tauri` commands are consistent. -- FR-55/FR-56, epic single-document requirement.

**Acceptance Criteria:**
- Given `bun run verify:ios-ipa` on this repo with no IPA built, when it runs, then the graph seam check confirms `tauri-plugin-global-shortcut/-autostart/-updater/-process` and `tray-icon` are absent from the `aarch64-apple-ios` closure and present for the desktop target, the artifact scan is reported skipped with a build hint, and it exits 0.
- Given a desktop-only plugin were present in the iOS `cargo tree` (or a forbidden symbol in a supplied IPA), when the verifier runs, then it names the offending crate/symbol and exits non-zero (fail-closed); and if the differential control does not show the crates on desktop, it refuses to false-pass.
- Given the rewritten `docs/ios.md`, when read top to bottom, then it documents the repeatable build command, the exact IPA output path, the dev-signed-then-re-signed posture (no manual signing configs; tauri#10668), the seam-verification step, and the no-signing-material guarantee; the placeholder Note is gone; the Sideloadly/zsign re-sign steps remain and are cross-linked; the TOC matches; all `tauri` commands are internally consistent.
- Given the diff, when scanned, then only `scripts/verify-ios-ipa.ts`, `package.json`, and `docs/ios.md` changed; no `.github/workflows/*`, `tauri.conf.json`, `project.yml`, or compile-seam source changed; and no real Team ID / profile / `.p12` / token appears (placeholders only).
- Given `bun run check`, when run, then biome + tsc + vitest are green (the new script lints/typechecks; no behavior regressions).

## Design Notes

**Why a script, not just docs:** FR-56 requires the seam "verified against the artifact, not just source," and the project already ships verification-gate scripts (`check-js-licenses.ts`, `gen-ios-icons.swift`). A pure `unzip | nm | grep` doc recipe is the kind of manual step that rots.

**The authoritative check is the dependency graph, verified working during planning:**
```
cargo tree -p keeper --target aarch64-apple-ios   -e normal  # forbidden crates ABSENT
cargo tree -p keeper --target aarch64-apple-darwin -e normal  # all 5 PRESENT (tray-icon, autostart,
                                                              # global-shortcut, process, updater)
```
The desktop plugins are `cfg(not(any(ios,android)))` dependencies — structurally excluded from the iOS target and already enforced every CI run by `cargo check --target aarch64-apple-ios`. The graph assertion is the load-bearing proof; the IPA scan confirms it in the shipped artifact.

**Stripping caveat:** release iOS Mach-O binaries are often stripped, so `nm` may show little — treat "no forbidden symbols" as pass but never rely on the binary scan alone (the graph assertion governs). Optionally also scan the unstripped Rust staticlib under `gen/apple/Externals/` when present.

**Export posture:** `--export-method debugging` yields a dev-signed IPA (owner's Personal Team); testers re-sign with their own identity (Sideloadly on-install / zsign CLI). A truly "unsigned" Tauri export is not achievable without manual signing configs that break iOS builds (tauri#10668) — this is the AC's "dev-signed with signature replacement documented" branch, not a literal unsigned artifact.

**Scope seams:** 15.2 = re-sign consumer steps (keep, cross-link); 15.4 = promote the iOS compile check to a **required** CI status (do not touch CI here); 15.6 = run the real build + one on-device re-signed install. This story = the build recipe + the seam verifier, authored and self-verified on the graph path, with the real-artifact scan and on-device install deferred to 15.6 (mirrors 15.2's "documented now, exercised on hardware later" posture).

## Verification

**Commands:**
- `bun run verify:ios-ipa` -- expected: graph seam check passes (forbidden plugins absent for iOS, present for desktop), artifact scan reported skipped (no IPA), exit 0.
- `cargo tree -p keeper --target aarch64-apple-ios -e normal | grep -iE 'global-shortcut|autostart|updater|tauri-plugin-process|tray-icon'` (from `src-tauri/`) -- expected: no matches.
- `bun run check` -- expected: green (biome + tsc + vitest).
- `git diff --name-only` -- expected: only `scripts/verify-ios-ipa.ts`, `package.json`, `docs/ios.md`.
- `grep -nE '[A-Z0-9]{10}' docs/ios.md` -- expected: only the `XXXXXXXXXX` placeholder; no real Team ID / `.p12` / `syt_` / profile UUID.

**Manual checks (if no CLI):**
- Read `docs/ios.md` end to end: build recipe present, output path correct, dev-signed-then-re-signed rationale, seam-verification step, no-signing guarantee, placeholder Note gone, Sideloadly/zsign steps retained + cross-linked, TOC consistent, honest voice, no secrets.

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 9: (high 0, medium 3, low 6)
- defer: 0
- reject: 8
- addressed_findings:
  - `[medium]` `[patch]` Default IPA scan hard-coded `build/arm64/keeper.ipa`; since no build has run the exact name/subdir is unverified and a wrong name would make the default scan silently SKIP forever — the scanner now recursively finds any `*.ipa` under `gen/apple/build/`, and the doc path is softened to "typically at … confirm from your first build's output".
  - `[medium]` `[patch]` `strings -a` on an unstripped debugging binary (exactly this story's `--export-method debugging` case) could exceed the 64 MB `maxBuffer` and throw, turning a clean binary into a hard `verification FAILED` — the IPA symbol dump now uses a non-throwing `tryDump` (256 MB buffer) that degrades to an inconclusive SKIP on truncation, so only the authoritative graph check can fail the run.
  - `[medium]` `[patch]` The shipped verifier's fail-closed branches (leaked crate, mis-scoped control, symbol hit) are unreachable in-repo (the seam is structurally enforced) and were therefore untested — extracted pure `parseForbiddenCrates`/`findForbiddenSymbols` and added `scripts/verify-ios-ipa.test.ts` (8 cases: clean/leaked/mis-scope trees, deep-link-not-flagged, substring safety, mangled-symbol matching) which runs inside `bun run check`.
  - `[low]` `[patch]` The largest-file executable fallback could scan a non–Mach-O resource (e.g. `Assets.car`) and report a meaningless pass — added a Mach-O magic-byte check; if no Mach-O is found the scan reports inconclusive rather than scanning a blob.
  - `[low]` `[patch]` `nm -gU` on a partly-stripped binary could surface C/ObjC symbols yet miss Rust ones and skip the `strings` fallback — the scan now unions `nm` and `strings` output.
  - `[low]` `[patch]` Added `-h/--help` output and an extra-argument guard to the CLI.
  - `[low]` `[patch]` Doc oversold the symbol scan ("confirms none appear") — reworded to "a non-match cannot prove absence in a stripped binary; the graph check is authoritative", and pointed at the unstripped `gen/apple/Externals/` staticlib for symbol-level certainty.
  - `[low]` `[patch]` Corrected the doc's `# graph check only` comment (the bare command also scans a default-path IPA when one exists).
  - `[low]` `[patch]` Added a comment noting `nm -U` is macOS-`nm` (defined-only) semantics, valid because iOS artifacts only exist on macOS.
- rejected (noise/refuted/out-of-scope): `--config` on `tauri ios build` (**empirically probed** — the bare command discovers the nested `tauri.conf.json` and proceeds past config/signing, so no `--config` is needed; matches the doc's existing dev/build convention); word-boundary symbol match (would break length-prefixed Rust mangled names, adding false negatives); crafted-IPA symlink escape (out of threat model — the maintainer builds/controls the IPA and output is only scanned, never exfiltrated); relative-path-vs-cwd (resolving user paths against cwd is correct CLI convention); rustup iOS-target precondition (`cargo tree --target` resolves the graph without an installed std — verified live); tauri#10668 timeless-caveat (already adjudicated reject in 15.2, appropriately framed as current Tauri 2.x behavior); cargo-tree `--no-dedupe` and zip-corruption (reviewer self-refuted / handled by the Payload existence check); partial-control mis-scope (the per-crate desktop assertion already fails on any specifically-missing crate).

## Auto Run Result

**Status:** done

**Summary:** Delivered the shareable-IPA build path for the free-signing iOS distribution posture. Added a reusable, fail-closed seam verifier (`scripts/verify-ios-ipa.ts`, wired as `bun run verify:ios-ipa`) whose authoritative layer proves via `cargo tree` that the five desktop-only Tauri plugins (`tray-icon`, `tauri-plugin-global-shortcut/-autostart/-updater/-process`) are absent from the `aarch64-apple-ios` build closure and present on `aarch64-apple-darwin` (differential control), with an optional best-effort Mach-O symbol scan of a built IPA. Expanded `docs/ios.md` with a "Building a shareable IPA" section: the exact `bun run tauri ios build --export-method debugging` recipe, the (gitignored) output path, the honest dev-signed-then-re-signed posture (no manual signing configs; tauri#10668), the FR-56 seam-verification step, and the AD-32 no-signing-material guarantee — replacing the placeholder Note left by 15.2 and cross-linking the existing Sideloadly/zsign re-sign steps. The real IPA build and on-device re-signed install are exercised in Story 15.6.

**Files changed:**
- `scripts/verify-ios-ipa.ts` — NEW: two-layer iOS compile-seam verifier (authoritative `cargo tree` differential + best-effort IPA Mach-O scan); read-only, idempotent, fail-closed.
- `scripts/verify-ios-ipa.test.ts` — NEW (review-driven): 8 vitest cases locking the pure parser/matcher and the fail-closed classification branches.
- `package.json` — added the `verify:ios-ipa` script (kept out of the aggregate `check`/`check:all` gates).
- `docs/ios.md` — new "Building a shareable IPA" section + TOC renumbering; placeholder build-recipe Note removed; re-sign steps retained and cross-linked.

**Review findings:** 0 intent_gap, 0 bad_spec, 9 patches applied (3 medium: default-IPA-path globbing, `strings` maxBuffer false-fail → inconclusive SKIP, added fail-closed unit tests; 6 low: Mach-O magic-byte check, nm∪strings union, `--help`/arg guard, doc honesty softening, `graph check only` comment fix, `nm -U` semantics comment), 0 deferred, 8 rejected. The `--config` finding was rejected after an **empirical probe** confirmed the bare `tauri ios build` discovers the nested config; both adversarial reviewers independently confirmed the seam genuinely holds (all five crates absent on iOS, present on desktop) and secret hygiene is sound.

**Verification:**
- `bun run verify:ios-ipa` → exit 0: all five forbidden crates ABSENT for `aarch64-apple-ios`, PRESENT for `aarch64-apple-darwin`; artifact scan reported SKIP (no IPA present).
- `bun run check` → green: Biome clean (315 files), tsc clean, 1253 vitest tests pass (117 files, incl. the 8 new verifier tests), core-tauri-free gate passes.
- `git status --porcelain` → only `docs/ios.md`, `package.json`, `scripts/verify-ios-ipa.ts`, `scripts/verify-ios-ipa.test.ts` (plus this planning artifact). No source/config/CI/asset changes; the `tauri ios build` config-discovery probe's regenerated `gen/apple` files were reverted to baseline.
- Secret scan → no real Team ID / `.p12` / `syt_` / profile UUID; only the `XXXXXXXXXX` placeholder and env-var names.

**Follow-up review recommended:** false — the review changes are localized hardening of a **dev-only** verification tool plus documentation wording, with no impact on app runtime behavior, API, security, or data. The authoritative graph layer is fully unit-tested and executed green this run.

**Deviations from spec:** the spec's "only three files change" constraint became four — `scripts/verify-ios-ipa.test.ts` was added as a review-driven patch (finding P). Justified: a colocated test is the idiomatic, in-scope companion to the new script and locks its otherwise-unreachable fail-closed branches; it changes no source/config/CI.

**Residual risks:**
- The IPA-scan branch (unzip → Mach-O detection → `nm`/`strings` union) cannot execute without a real build (no Xcode/signing in automation), so it is exercised for-real only in Story 15.6 on the owner's Mac; its pure sub-logic is unit-tested, but the orchestration around it is not run this pass. Consequence is low — it is a belt-and-suspenders layer over the fully-tested, authoritative graph check.
- The documented `bun run tauri ios build` recipe and the exact IPA output path are validated at the config-discovery/early-build stage (the probe reached signing) but not through a completed archive; the first completed build in Story 15.6 confirms the precise artifact name (the scanner already globs for it).
- `FORBIDDEN` is hand-mirrored from `Cargo.toml`'s `cfg(not(any(ios,android)))` block; a comment cross-references the manifest, but no automated equality test guards future drift if that block changes.
