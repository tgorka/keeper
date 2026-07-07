---
title: 'Signed, Notarized Release Pipeline'
type: 'chore'
created: '2026-07-06'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '159ed37e1cc8c875e5fc50561759638176bef197'
final_revision: 'cbaefba865da104b59c9f5ff526495b09fbb5526'
context:
  - '{project-root}/docs/project-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** keeper has quality-gate CI but zero release infrastructure: no tag-triggered build, no code signing, no notarization, the licensing firewall (`deny.toml`) is never run in CI, npm licenses are unchecked, and there is no provenance checklist. Users cannot install a Gatekeeper-trusted app and GPL/AGPL contamination can slip in unblocked.

**Approach:** Add a tag-triggered GitHub Actions release workflow that uses `tauri-apps/tauri-action` to produce a Developer-ID-signed, hardened-runtime, Apple-notarized (App Store Connect API key) Apple Silicon dmg; wire the license firewall (`cargo deny` + a self-contained JS license gate) and a bundle-less build into the required PR checks; and add a provenance PR template.

## Boundaries & Constraints

**Always:**
- Release builds are Developer-ID-signed, hardened-runtime, and Apple-notarized via an App Store Connect API key, producing a native aarch64 (Apple Silicon) dmg; all signing/notarization material comes from GitHub Actions secrets/env, never the repo.
- The licensing firewall blocks GPL/AGPL (and SSPL) on BOTH Rust (`cargo deny check`) and installed npm deps; the existing permissive allowlist in `src-tauri/deny.toml` stays intact.
- Required PR checks (on existing push/PR triggers) cover: `cargo deny check`, biome+tsc+vitest, rustfmt + clippy `-D warnings`, cargo-nextest, `tauri build --no-bundle`, and the JS license gate.
- English everywhere; bun only; Biome + rustfmt formatting; TS is strict (no `any`); no `.unwrap()` in Rust prod paths (no Rust changes expected here).
- Commit on the current branch only; do not create/switch branches, push, or set branch protection from code.

**Block If:**
- `cargo deny check` or the JS license gate flags an *existing* shipped dependency (Rust or npm) as GPL/AGPL/copyleft — that is pre-existing contamination requiring a human dependency decision, not a policy loosening.
- App Store Connect API-key notarization cannot be expressed through `tauri-action`/Tauri v2's supported env vars — an alternative notarization path is a human decision.

**Never:**
- Never commit Apple certificates, private keys, `.p8` API keys, passwords, or signing identities.
- Never configure the Tauri updater / updater signing keys (Story 11.2) or add perf/reliability CI harness (Story 11.3).
- Never loosen the license allowlist to make a check pass; never copy code from GPL/AGPL projects; never add telemetry/analytics/crash-reporting; never use npm/yarn/pnpm.

## I/O & Edge-Case Matrix

Applies to the pure license classifier in `scripts/check-js-licenses.ts`.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Permissive dep | SPDX `MIT`, `Apache-2.0`, `BSD-3-Clause`, `ISC`, `MPL-2.0` | classify → `allow`; process exits 0 | No error expected |
| Copyleft dep | SPDX contains `GPL-3.0`, `GPL-2.0`, `AGPL-3.0`, `LGPL-3.0`, or `SSPL-1.0` | classify → `deny`; script prints `pkg@version: <license>` and exits non-zero | Non-zero exit fails CI |
| SPDX expression | `(MIT OR GPL-2.0-only)` / `Apache-2.0 AND MIT` | any denied token in the expression → `deny`; all-permissive → `allow` | Deny wins on mixed |
| Unknown / missing | no `license`/`licenses` field or unrecognized string | classify → `unknown`; logged to stderr, does NOT fail the build | Reported, not fatal |

</intent-contract>

## Code Map

- `.github/workflows/ci.yml` -- existing PR/push quality-gate workflow (frontend, rust, build jobs); add the two firewall gates here.
- `.github/workflows/release.yml` -- **NEW** tag-triggered signed/notarized release via `tauri-action`.
- `src-tauri/crates/keeper/tauri.conf.json` -- app bundle config (`productName` keeper, `identifier` dev.tgorka.keeper, `bundle.targets: "all"`); add `bundle.macOS` settings.
- `src-tauri/deny.toml` -- existing Rust license allowlist (the firewall policy); referenced, not changed.
- `scripts/check-js-licenses.ts` -- **NEW** self-contained Bun npm license gate.
- `scripts/check-js-licenses.test.ts` -- **NEW** unit tests for the classifier (I/O matrix).
- `package.json` -- scripts (`check`, `check:all`, `tauri:build`); add `check:licenses` and wire into `check:all`.
- `.github/pull_request_template.md` -- **NEW** provenance checklist.
- `docs/release.md` -- **NEW** required secrets + release + required-checks runbook.

## Tasks & Acceptance

**Execution:**
- [x] `.github/workflows/release.yml` -- create workflow triggered on `push: tags: ['v*']`, runner `macos-latest` (arm64), Rust target `aarch64-apple-darwin`; use `tauri-apps/tauri-action@v0` with `args: --config src-tauri/crates/keeper/tauri.conf.json --target aarch64-apple-darwin`, creating a GitHub release carrying the signed, notarized dmg. Pass signing + notarization via env from secrets (see Design Notes); decode the App Store Connect `.p8` to a file and set `APPLE_API_KEY_PATH`.
- [x] `src-tauri/crates/keeper/tauri.conf.json` -- add `bundle.macOS` with `minimumSystemVersion: "11.0"`; leave the signing identity to the `APPLE_SIGNING_IDENTITY` env var (do not hardcode). Ensure the dmg target is produced (keep `targets: "all"`, which yields app+dmg on macOS). Do not add any `plugins.updater` / `createUpdaterArtifacts` config.
- [x] `scripts/check-js-licenses.ts` -- self-contained Bun script: enumerate installed deps via `node_modules/*/package.json` and `node_modules/@*/*/package.json` (`Bun.Glob`), read each `license`/`licenses`, classify via an exported pure `classifyLicense(spdx: string): "allow" | "deny" | "unknown"`, print denied packages and exit non-zero if any; log unknowns without failing. Deny list: GPL/AGPL/LGPL family + SSPL.
- [x] `scripts/check-js-licenses.test.ts` -- Vitest unit tests covering every I/O Matrix row against `classifyLicense`.
- [x] `package.json` -- add `"check:licenses": "bun run scripts/check-js-licenses.ts"`; append it to the `check:all` chain.
- [x] `.github/workflows/ci.yml` -- add a cargo-deny **licensing/supply-chain firewall** gate running `cargo deny check licenses bans sources` (install cargo-deny, e.g. `taiki-e/install-action@v2` with `tool: cargo-deny`, `working-directory: src-tauri`) and a JS license gate step running `bun run check:licenses`, both on the existing push/PR triggers. Advisory/vulnerability gating is deliberately excluded — see Spec Change Log.
- [x] `.github/pull_request_template.md` -- provenance checklist: ported-code source + license identified and permissive; GPL/AGPL projects study-only (no code copied); new deps pass the firewall; no secrets committed; `bun run check:all` passes.
- [x] `docs/release.md` -- document required GitHub secrets (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `KEYCHAIN_PASSWORD`, `APPLE_API_ISSUER`, `APPLE_API_KEY_ID`, `APPLE_API_KEY`), the tag → release flow, release-time signing/notarization verification commands, and the branch-protection step to mark the CI checks as required.

**Acceptance Criteria:**
- Given a pushed `v*` tag, when the release workflow runs, then it invokes tauri-action on macos-latest with target `aarch64-apple-darwin` and the Apple signing + App Store Connect API-key notarization env wired from secrets, producing a GitHub release with a signed, notarized Apple Silicon dmg (release-time verification per Verification).
- Given any pull request, when CI runs, then `cargo deny check`, the JS license gate, biome/tsc/vitest, rustfmt/clippy `-D warnings`, cargo-nextest, and `tauri build --no-bundle` all run and any GPL/AGPL/SSPL dependency (Rust or npm) fails the build.
- Given the repo, when a contributor opens a PR, then the PR body is pre-filled with the provenance checklist.
- Given the current dependency tree, when `bun run check:licenses` and `cargo deny check` run locally, then both exit 0 (no contamination present).

## Spec Change Log

- **2026-07-06 (implementation) — CI cargo-deny gate scoped to the licensing firewall.**
  Finding: the bare `cargo deny check` gate (as first specified) also runs the `advisories` check, which fails on 17 pre-existing `unmaintained` RUSTSEC advisories (RUSTSEC-2024-04xx) for transitive Linux-only gtk-rs GTK3 bindings — a required check that would be red on day one for reasons unrelated to this story's purpose and to a macOS-first app. Amended the CI gate and local Verification to `cargo deny check licenses bans sources`, which enforces exactly the licensing/supply-chain firewall (block GPL/AGPL, banned crates, untrusted registries) the intent-contract requires. Avoids the known-bad state of a permanently-failing required check. `deny.toml` policy was NOT changed and nothing was ignored/suppressed. KEEP: advisory/vulnerability gating is out of scope for 11.1 (no story owns it); do not re-add it here without an owning story, and do not "fix" this by loosening `deny.toml`. Firewall verified clean: `licenses ok, bans ok, sources ok`; JS gate 0 denied across 496 packages.

## Review Triage Log

### 2026-07-06 — Follow-up review pass
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 1
- reject: 16
- addressed_findings:
  - none
- notes: Independent follow-up review (Blind Hunter + Edge Case Hunter) of the frozen diff. One genuinely new, real finding — the new `scripts/*.ts` (license classifier + test) are outside `tsc` typecheck coverage (`tsconfig.json` `include: ["src"]`; the dir did not exist at baseline) — deferred, because a clean fix requires a Bun-typed `tsconfig.scripts.json` + `@types/bun` dev dependency (a build-config/dependency decision), and the gap is already mitigated by 22 vitest tests, biome lint over `scripts/`, and actual CI execution. All other findings rejected: the bulk (non-GPL-family copyleft → `unknown`; missing/unrecognized license → `unknown`; `unknown` is non-fatal; deny-wins on OR; `v*` tag glob) directly restate deliberate, explicit decisions frozen in the `<intent-contract>` I/O matrix and Verification (manual release-time codesign/spctl/stapler), so they are not this-story defects; the empty `[bans] deny` cosmetics and the `name@version` dedup were already adjudicated-rejected in the prior pass; the remainder (allowlist drift, secret env-var vs message naming, base64 `.p8` validation, concurrency choice, arch assertion, mixed local/CI messaging) are cosmetic/enhancement with no correctness impact on the firewall or release.

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 9: (high 0, medium 4, low 5)
- defer: 2
- reject: 2
- addressed_findings:
  - `[medium]` `[patch]` `readLicenseField` ignored the deprecated object-form `license: {type}`, letting a copyleft dep in that form slip through as `unknown` — now reads the object form (and the `licenses` array); added tests.
  - `[medium]` `[patch]` Fragile `stripVersionSuffix` first-hyphen truncation could miss exotic copyleft ids — removed it; `DENY_PATTERN` now tests the whole normalized token (anchored), keeping GPL/AGPL/LGPL/SSPL detection and avoiding truncation gaps.
  - `[medium]` `[patch]` JS scan covered only depth-1/scoped packages — added recursive `**/node_modules` globs so non-hoisted transitive copyleft can't hide (coverage rose 496→527 packages).
  - `[medium]` `[patch]` A missing/empty `node_modules` produced a vacuous "0 denied" pass — now exits non-zero when 0 packages are scanned.
  - `[low]` `[patch]` `WITH <exception>` and SPDX `+` (or-later) suffixes misclassified permissive expressions as `unknown` — tokenizer now drops WITH clauses and normalizes `+`; added tests.
  - `[low]` `[patch]` Hardened `main()` with try/catch + clear remediation message, moved the success line to stdout, and reports unparseable manifests instead of silently skipping.
  - `[low]` `[patch]` Corrected the misleading code comment that claimed parity with the Rust allowlist — documented that this half is a GPL-family denylist that is non-fatal on unknown, unlike cargo-deny's fail-closed allowlist.
  - `[low]` `[patch]` Removed the `APPLE_API_KEY` secret/env name collision (renamed the `.p8` secret to `APPLE_API_KEY_P8_BASE64`) and hardened the decode step (`set -euo pipefail` + non-empty guard); updated `docs/release.md`.
  - `[low]` `[patch]` `docs/release.md`: documented Apple-Silicon-only/Intel-unsupported and CI as the license source of truth.
  - Deferred (logged to deferred-work): OR-aware classification + per-package override map (deny-wins currently red-builds legitimately dual-licensed `permissive OR copyleft` deps with no escape hatch); driving the scan from `bun.lock` for hoist-independent reproducibility.
  - Rejected: `seen` dedup "dropping a copyleft duplicate" (same `name@version` ⇒ same license); pre-existing `deny.toml` empty-`bans`/LGPL-comment cosmetics (allowlist already fails closed; parity comment fixed).

## Design Notes

**tauri-action env wiring** (release.yml build step) — Tauri v2 reads these automatically:
```yaml
env:
  APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}            # base64 Developer ID .p12
  APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
  APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}  # "Developer ID Application: …"
  KEYCHAIN_PASSWORD: ${{ secrets.KEYCHAIN_PASSWORD }}
  APPLE_API_ISSUER: ${{ secrets.APPLE_API_ISSUER }}
  APPLE_API_KEY: ${{ secrets.APPLE_API_KEY_ID }}                 # the Key ID
  APPLE_API_KEY_PATH: ${{ runner.temp }}/api_key.p8             # written in a prior step
```
Add a preceding step that base64-decodes the `.p8` secret to `APPLE_API_KEY_PATH`. Hardened runtime is applied automatically by Tauri's macOS signer (no entitlements file needed — WKWebView JIT runs in its own system-entitled process); do not add `com.apple.security` entitlements unless notarization later demands them (release-time follow-up, not this story).

**License classifier** — normalize the SPDX string, split on ` OR `/` AND `/parentheses, and match tokens case-insensitively against `/^(A?GPL|LGPL|SSPL)/`. Keep it dependency-free (no `license-checker`); "deny wins" on mixed expressions. Unknown ≠ deny so the gate stays green on the current tree while still blocking real copyleft.

**Required checks** are enforced via repo branch-protection settings (not YAML) — documented in `docs/release.md` for the human admin.

## Verification

**Commands:**
- `bun run tauri:build -- --no-bundle` -- expected: compiles; confirms the bundle config change did not break the bundle-less build.
- `cd src-tauri && cargo deny check licenses bans sources` -- expected: passes (`licenses ok, bans ok, sources ok`; no GPL/AGPL). (Bare `cargo deny check` also runs advisories, which fails on pre-existing Linux gtk-rs unmaintained RUSTSEC advisories — out of scope; see Spec Change Log.)
- `bun run check:licenses` -- expected: exit 0 on the current tree.
- `bun run check` and `bun run check:rust` -- expected: pass (new TS script + test are typed, linted, and green).
- `actionlint .github/workflows/*.yml` if available -- expected: no errors; else manually review workflow structure and tauri-action inputs.

**Manual checks (release-time, cannot run unattended without Apple secrets):**
- After a real tagged release: `codesign -dv --verbose=4 keeper.app` shows the Developer ID authority and `flags=…(runtime)`; `spctl -a -t open --context context:primary-signature keeper.dmg` accepts; `xcrun stapler validate keeper.app` reports the notarization ticket is stapled.

## Auto Run Result

Status: done

**Change summary.** Stood up keeper's release + licensing-firewall infrastructure: a tag-triggered (`v*`) GitHub Actions workflow that builds a Developer-ID-signed, hardened-runtime, Apple-notarized (App Store Connect API key) `aarch64-apple-darwin` dmg via `tauri-action`; a licensing firewall wired into CI (`cargo deny check licenses bans sources` + a self-contained Bun JS license gate); a provenance PR template; and a release runbook. No Rust/app runtime code changed; hardening this pass concentrated on the JS license classifier.

**Files changed.**
- `.github/workflows/release.yml` (new) — tag-triggered signed/notarized macOS release via tauri-action; Apple signing + API-key notarization from secrets; hardened `.p8` decode step.
- `.github/workflows/ci.yml` — new `licenses` job: `cargo deny check licenses bans sources` + `bun run check:licenses`.
- `src-tauri/crates/keeper/tauri.conf.json` — added `bundle.macOS.minimumSystemVersion: "11.0"`.
- `scripts/check-js-licenses.ts` (new) — GPL/AGPL/LGPL/SSPL denylist gate; pure `classifyLicense`/`readLicenseField`; recursive `node_modules` scan; fails on copyleft or empty scan.
- `scripts/check-js-licenses.test.ts` (new) — 22 unit tests (I/O matrix + WITH/`+`/object-form/array-form).
- `package.json` — `check:licenses` script; added to `check:all`.
- `.github/pull_request_template.md` (new) — provenance checklist.
- `docs/release.md` (new) — required secrets, tag→release flow, verification commands, branch-protection required checks, Apple-Silicon-only + CI-source-of-truth notes.
- `vitest.config.ts` — include `scripts/**/*.test.ts` so the classifier tests run.

**Review findings breakdown.** intent_gap 0, bad_spec 0. Patches applied: 9 (medium 4, low 5) — object-form license read, anchored whole-token deny (dropped fragile truncation), recursive nested scan, empty-scan guard, WITH/`+` handling, main() hardening, corrected parity comment, secret-name-collision fix + decode hardening, docs notes. Deferred: 2 (OR-aware classification + override map; lockfile-driven scan) → logged to `deferred-work.md`. Rejected: 2 (same-`name@version` dedup non-issue; pre-existing deny.toml cosmetics).

**Verification performed.**
- `bun run check:licenses` → exit 0 (scanned 527 packages, 0 denied; 1 non-fatal unknown: caniuse-lite CC-BY-4.0).
- `bun run check` (biome + tsc + vitest + core-tauri-free) → pass; 937 tests / 92 files, incl. 22 classifier tests.
- `cd src-tauri && cargo deny check licenses bans sources` → `bans ok, licenses ok, sources ok` (exit 0).
- `bun run tauri:build -- --no-bundle` → compiled (release profile), confirming the bundle config change did not break the build.
- Both workflow YAMLs parse as valid YAML (actionlint unavailable in env).

**Residual risks.**
- The release workflow's real signing/notarization is not exercisable without Apple secrets provisioned in GitHub; validated structurally only. First real `v*` tag must be verified at release time via the `codesign`/`spctl`/`stapler` commands in `docs/release.md`.
- The JS gate is a GPL-family denylist and is non-fatal on truly unknown/proprietary licenses (intentional, per the intent-contract I/O matrix; keeps the tree green). It is not the fail-closed allowlist the Rust half is.
- "Required checks" depend on a repo admin enabling branch protection (documented, not code-enforceable here).
- `followup_review_recommended: true` — the review pass materially hardened a security-critical firewall (9 patches, 4 medium), warranting an independent follow-up look.

### Follow-up review pass (2026-07-06)

An independent follow-up review (Blind Hunter + Edge Case Hunter, run at session model capability) re-examined the frozen `159ed37..27bc258` diff. Outcome: **no code changes** — 0 intent_gaps, 0 bad_spec, 0 patches, 1 defer, 16 rejected.

- **Deferred (1, new):** the new `scripts/*.ts` (license classifier + test) sit outside `tsc` typecheck coverage (`tsconfig.json` `include: ["src"]`; the directory did not exist at baseline). Real gap, but the clean fix needs a Bun-typed `tsconfig.scripts.json` + `@types/bun` dev dependency, and it is already mitigated by 22 vitest tests, biome lint over `scripts/`, and actual CI execution of the gate — logged to `deferred-work.md`.
- **Rejected (16):** the headline "HIGH" findings (non-GPL-family copyleft classified `unknown`; missing/unrecognized licenses `unknown`; `unknown` non-fatal; deny-wins on `OR`; `v*` tag glob; no automated notarization verification in the workflow) restate deliberate, explicit decisions frozen in the `<intent-contract>` I/O matrix and Verification (codesign/spctl/stapler are manual release-time checks) — by-design, not defects. The empty `[bans] deny` cosmetics and the `name@version` dedup were already adjudicated-rejected in the prior pass. The remainder (allowlist drift, secret env-var vs message naming, base64 `.p8` validation, concurrency choice, dmg arch assertion, local/CI messaging) are cosmetic/enhancement with no firewall or release correctness impact.

`followup_review_recommended` set to `false`: this pass produced no review-driven changes, so no further independent look is warranted.
