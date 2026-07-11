---
title: 'docs/ios.md — Free Signing Walkthrough'
type: 'feature'
created: '2026-07-11'
status: 'done'
baseline_revision: '091dff8292ae4a163bd33f2fff7e934c3f6ea75e'
final_revision: 'b892732ae60bb17ee18f853b78227dee1a9ff6bc'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings:
  - oversized
---

<intent-contract>

## Intent

**Problem:** `docs/ios.md` today only covers build setup (prerequisites, the XcodeGen regeneration loop, deployment target) from Story 12.1. It does not take a Mac + iPhone owner to a *running, and still-running* keeper: there is no free Personal-Team signing walkthrough, no on-device certificate-trust / Developer-Mode steps, no 7-day re-arm ritual, no AltServer auto-refresh option, and no Sideloadly/zsign re-sign path for shared IPAs. The in-app "What's different on iPhone" link (`about-section.tsx` → `https://github.com/tgorka/keeper/blob/main/docs/ios.md`) already points here, so the disclosure the app promises has no matching document.

**Approach:** Expand `docs/ios.md` into the single end-to-end free-signing walkthrough the epic requires, keeping the existing build-setup content intact and adding: Personal-Team setup, on-device trust + Developer Mode, the weekly 7-day re-arm ritual (with its per-week cost in minutes), the optional AltServer auto-refresh, and the Sideloadly/zsign re-sign path for installing shared IPAs without Xcode. Add a **Limitations** section that mirrors the in-app "On this iPhone" disclosure one-to-one, quoting the app's exact strings and naming their source file so app and docs can never diverge. Docs-only: no code, no secrets.

## Boundaries & Constraints

**Always:**
- Only `docs/ios.md` changes. No source, config, asset, or CI file is modified.
- The **Limitations** section reproduces the four `IOS_DISCLOSURE_LINES` from `src/components/settings/about-section.tsx` **verbatim** and enumerates exactly those four items — no extra platform-limitation claims that exceed the in-app disclosure, none omitted — and names that file as the single source of truth so future edits stay paired.
- The walkthrough documents the researched, project-true flow: bundle id `dev.tgorka.keeper` (shared with macOS), min iOS 16.0, free Personal Team (7-day profile expiry, ~3 devices, no TestFlight/App-Store), on-device Developer Mode + cert trust, `bun run tauri ios dev` weekly re-arm (data persists while bundle id is unchanged, ~30-second weekly chore), AltServer/AltStore Classic Wi-Fi auto-refresh, and `bun run tauri ios build --export-method debugging` → per-tester re-sign via Sideloadly or zsign (automatic-signing-then-re-sign, never manual signing configs — tauri#10668).
- Preserve the existing prerequisites, XcodeGen 2.45.4 regeneration-loop, and deployment-target content; integrate rather than delete.
- Honest voice: English, sentence case, no hype; state costs and limits plainly. Team id is set only via an env var exported in the shell, shown as a placeholder — never a real value.

**Block If:**
- The in-app "On this iPhone" strings (`IOS_DISCLOSURE_LINES` in `about-section.tsx`) cannot be located or read, so the required one-to-one match cannot be verified against the true source.

**Never:**
- No real Apple Team ID, provisioning profile, `.p12`, signing identity, or any secret in the doc.
- No on-device execution or sign-off in this story — the documented path is exercised end-to-end (including one Sideloadly re-sign flow) on the owner's iPhone in Story 15.6; this story delivers the document and self-verifies it against the codebase.
- No code, entitlement, bundle-id, deployment-target, or CI changes; no new tooling or endpoints. Do not add the actual shareable-IPA build recipe here (that is Story 15.3, which appends to this doc) — 15.2 documents the *re-sign* path only.

</intent-contract>

## Code Map

- `docs/ios.md` -- THE deliverable; currently build-setup only (prerequisites, signing-without-secrets, regeneration loop, deployment target, core compile check). Expand into the full walkthrough.
- `src/components/settings/about-section.tsx` -- `IOS_DISCLOSURE_LINES` (lines ~35–40, the four "On this iPhone" items) and `IOS_DOCS_URL` (line 28, the link into this doc). **Source of truth** for the Limitations section.
- `src/components/settings/no-background-sync-disclosure.tsx` -- `NO_BACKGROUND_SYNC_SENTENCE` + `BADGE_NOT_LIVE_SENTENCE`; its header comment already says these move to `docs/ios.md` via Story 15.2. Reference for the sync/badge honesty wording.
- `_bmad-output/planning-artifacts/research-ios-2026-07-09.md` -- §2.1/§2.2 free-signing facts (7-day/3-device limits, AltServer, Sideloadly/zsign, blocked entitlements, tauri#10668). Fact source, not linked from the doc.
- `src-tauri/crates/keeper/gen/apple/project.yml` -- bundle id `dev.tgorka.keeper`, iOS 16.0. Reference only.
- `.github/workflows/ci.yml` -- the compile-only `cargo check --workspace --target aarch64-apple-ios` gate; mention its scope, don't change it.

## Tasks & Acceptance

**Execution:**
- [x] `docs/ios.md` -- Expand into the free-signing walkthrough. Keep existing Prerequisites / regeneration-loop / deployment-target sections. Add, in reading order: (1) **Signing on a free Personal Team** — any Apple ID grants a Personal Team; 7-day profile expiry, ~3 devices, no TestFlight/App-Store; set the team via the Tauri env var (placeholder only) and confirm the current "Signing without secrets" section names the **Tauri-correct** variable (see Design Notes — reconcile `APPLE_DEVELOPMENT_TEAM` → `TAURI_APPLE_DEVELOPMENT_TEAM`, verified against Tauri v2). (2) **First device install** — enable Developer Mode (Settings → Privacy & Security → Developer Mode, reboot), trust the cert (Settings → General → VPN & Device Management), then `bun run tauri ios dev --open`. (3) **The 7-day re-arm ritual** — why the app stops launching after 7 days, that re-running `tauri ios dev` refreshes it, data persists while bundle id is unchanged, ~a couple of minutes per week. (4) **AltServer auto-refresh (optional)** — AltStore Classic auto-refreshes the 7-day signature over Wi-Fi, removing the weekly chore. (5) **Sharing a build without Xcode** — `bun run tauri ios build --export-method debugging` produces an IPA a tester re-signs with their *own* free Apple ID via Sideloadly (on-install) or zsign (CLI); automatic signing + re-sign afterwards, never manual signing configs. (6) **Limitations** — mirror the in-app disclosure (see below). Rationale: satisfy the epic's single-document requirement.
- [x] `docs/ios.md` (Limitations section) -- Reproduce the four `IOS_DISCLOSURE_LINES` from `about-section.tsx` verbatim as the limitations list, add a one-line pointer that this list must stay identical to that file (single source of truth), and cross-reference the re-arm ritual for item 4 (7-day signature renewal). Rationale: the epic's one-to-one "app and docs never diverge" constraint.

**Acceptance Criteria:**
- Given the rewritten `docs/ios.md`, when a reader with a Mac + iPhone follows it top to bottom, then it covers every required beat — Personal-Team setup, on-device trust/Developer-Mode, the 7-day re-arm ritual with its per-week minute cost, the AltServer auto-refresh option, and the Sideloadly/zsign re-sign path — with the project-true bundle id, min-iOS, and `bun run tauri ios …` commands, and the pre-existing build-setup content preserved.
- Given the **Limitations** section, when its items are diffed against `IOS_DISCLOSURE_LINES` in `src/components/settings/about-section.tsx`, then the four strings match verbatim, no additional platform-limitation claim exceeds the in-app disclosure, and the section names that file as the source of truth.
- Given the whole document and the diff, when scanned for secrets and scope, then it contains no real Team ID / provisioning profile / signing identity / token (only shell-placeholder env values), and no file other than `docs/ios.md` changed.
- Given the doc is the target of the in-app "What's different on iPhone" link (`IOS_DOCS_URL`), when opened at that URL's file, then a device owner lands on a document that explains the disclosure they tapped through from.

## Design Notes

**One-to-one Limitations source (quote verbatim, keep paired with `about-section.tsx`):**
```
- keeper syncs and notifies only while it's open; background notifications await a future decision.
- No self-hosted bridge runner — manage your own bridges from your Mac.
- No global summon hotkey.
- Updates arrive by reinstalling keeper; its signature renews every 7 days.
```
The canonical sync sentence and badge note (from `no-background-sync-disclosure.tsx`) may be quoted to enrich the sync/limitations prose, but the four lines above are the enumerated limitations list: `NO_BACKGROUND_SYNC_SENTENCE` = "On iPhone, keeper syncs and notifies only while open. Close it and messages wait on your homeserver until you return — nothing is lost, and nothing here pretends to be push." and `BADGE_NOT_LIVE_SENTENCE` = "The app-icon badge is not a live count while keeper is closed; it reflects what keeper knew when it was last open."

**Env-var reconciliation (load-bearing accuracy fix):** the current doc's "Signing without secrets" section exports `APPLE_DEVELOPMENT_TEAM`, but Tauri v2 iOS automatic signing reads `TAURI_APPLE_DEVELOPMENT_TEAM` (or `bundle.iOS.developmentTeam` in `tauri.conf.json`). Verify the correct variable against Tauri v2 before finalizing and make the doc consistent; do not present two conflicting variables.

**Commands to document (project-true, run from repo root):**
- Team id (placeholder): `export TAURI_APPLE_DEVELOPMENT_TEAM=XXXXXXXXXX`
- Dev loop / re-arm: `bun run tauri ios dev --open`
- Shareable IPA (for the re-sign path): `bun run tauri ios build --export-method debugging`
- Regenerate project after `project.yml` edits: `cd src-tauri/crates/keeper/gen/apple && xcodegen generate`

**Scope seam vs Story 15.3:** 15.2 documents how a tester *re-signs* an already-produced IPA. The repeatable, symbol-clean IPA *build* recipe (unsigned export, no desktop plugin symbols) is Story 15.3, which appends to this doc — keep a short forward pointer, don't pre-empt it.

**Residual dependency:** the on-device path (Story 12.6) is not yet device-validated; this doc is authored from the proven build setup (12.1–12.3) plus the research report and is exercised on real hardware — including one Sideloadly re-sign — in Story 15.6. State on-device steps as the documented procedure, not as a claim of "personally verified on this run".

## Verification

**Commands:**
- `git diff --name-only` -- expected: only `docs/ios.md`.
- `grep -nE '[A-Z0-9]{10}' docs/ios.md` -- expected: no real 10-char Team ID; any hit is the `XXXXXXXXXX` placeholder only (also eyeball for `.p12`, `syt_`, profile UUIDs → none).
- `grep -nF 'IOS_DISCLOSURE_LINES' src/components/settings/about-section.tsx` then diff those four literals against the doc's Limitations list -- expected: verbatim match, four items, no extras.
- `bun run check` -- expected: green (no code changed; confirms the docs edit broke nothing).

**Manual checks (if no CLI):**
- Read `docs/ios.md` end to end: every required beat present (Personal Team, on-device trust + Developer Mode, 7-day re-arm with per-week minutes, AltServer option, Sideloadly/zsign re-sign), existing build-setup content preserved, honest voice, no secrets, and the Limitations section mirrors `about-section.tsx` with a source-of-truth pointer and a forward pointer to the (later) hardened IPA build recipe.

## Spec Change Log

_No bad_spec loopbacks — the spec was implemented as written. Review produced only patch-level doc-quality hardening (see Review Triage Log). Note: the implementer verified the env-var reconciliation and found the Tauri-v2-correct variable is `APPLE_DEVELOPMENT_TEAM` (the rename `TAURI_APPLE_DEVELOPMENT_TEAM` → `APPLE_DEVELOPMENT_TEAM` landed pre-2.0; confirmed against the repo's `@tauri-apps/cli` schema/changelog) — the opposite direction from the spec's stated assumption, but exactly the "name the Tauri-correct variable" intent the spec required._

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 6: (high 0, medium 2, low 4)
- defer: 0
- reject: 15
- addressed_findings:
  - `[medium]` `[patch]` "First device install" numbered steps fought the real dependency order (Developer Mode only appears after a first deploy; cert can only be trusted after install) — reordered to connect/unlock → set team → first build → enable Developer Mode + reboot → trust cert → launch, so a literal reader no longer hits a chicken-and-egg wall.
  - `[medium]` `[patch]` The "Sharing a build" section never told testers they must do their own on-device setup — added a pointer to `First device install` (their own Developer Mode + cert trust) and restated the ~3-device / 7-day limits apply to the tester, so a re-signed install actually launches for them.
  - `[low]` `[patch]` The env-var note asserted `APPLE_DEVELOPMENT_TEAM` "sets `bundle.iOS.developmentTeam` in `tauri.conf.json`" — but the repo intentionally has no iOS block; reworded to Tauri's env-override semantics and noted that grepping the file finds nothing (expected), matching the `@tauri-apps/cli` schema wording.
  - `[low]` `[patch]` AltServer auto-refresh was over-promised as "automatic"; added its preconditions (AltServer running, Mac awake, same network) and the fallback to manual re-arm if the window is missed.
  - `[low]` `[patch]` The team-id `export` is per-shell; added a note to persist it in the shell profile so a fresh terminal does not fail signing.
  - `[low]` `[patch]` Replaced the user-facing internal ticket reference ("Story 15.3") in the shipped doc with plain "not yet documented here … added later" wording, keeping the forward pointer without leaking process language.
- rejected (noise/refuted/out-of-scope): `--export-method debugging` questioned (verified valid Tauri CLI value: app-store-connect|release-testing|debugging); tauri#10668 citation (appropriately cited, cannot disprove); undated 7-day/3-device Apple-policy facts (current and framed as free-team constraints); "30-second chore" estimate (already hedged with "a couple of minutes if a rebuild"); TOC anchor-link fragility (no drift today, no heading renamed); 10-App-ID/week quota and 3-device-cap recovery (not reachable with a single stable bundle id in this flow); re-arm-with-device-not-connected and `cd src-tauri` cwd (refuted — doc already states "with the device connected" and the `cd` is part of a repo-root command); Simulator-vs-device ambiguity, "Trust This Computer"/unlock, cert-needs-network-once, sub-iOS-16 (minor/low, some already covered — folded the most useful bits into the patches above); disclosure duplication vs "single source of truth" (by design — the epic requires the doc to mirror the in-app string; the "edit both together" note is the sanctioned mitigation); in-app "renews every 7 days" passive phrasing (verbatim-locked source string, out of scope for a docs diff).

## Design Notes (implementation continuity)

- **Code Map:** `docs/ios.md` is now the single free-signing walkthrough (10 sections: Prerequisites → Regeneration loop → Deployment target → Signing on a free Personal Team → First device install → 7-day re-arm ritual → AltServer auto-refresh → Sharing a build without Xcode → Verifying the core compiles → Limitations). The Limitations list is a hand-mirrored copy of `IOS_DISCLOSURE_LINES` in `src/components/settings/about-section.tsx` (kept in sync by the "edit both together" note; three copied-string invariants total, incl. the two sync/badge sentences from `no-background-sync-disclosure.tsx`).
- **Env var:** `APPLE_DEVELOPMENT_TEAM` is the Tauri-v2-correct variable (not `TAURI_APPLE_DEVELOPMENT_TEAM`).
- **Seam for Story 15.3:** the hardened, symbol-clean IPA *build* recipe is intentionally not written here — 15.3 appends it. The re-sign consumer steps live in "Sharing a build without Xcode".

## Auto Run Result

**Status:** done

**Summary:** Expanded `docs/ios.md` from a build-setup-only page (Story 12.1) into the single end-to-end **free-signing walkthrough** the epic requires: it takes a Mac + iPhone owner from a fresh checkout to a running keeper and keeps it running, using free Personal-Team signing only. Added sections for Personal-Team setup (7-day expiry, ~3 devices, no TestFlight/App Store), a correctly-ordered first-device install (Developer Mode + reboot, certificate trust), the weekly 7-day re-arm ritual (~30 s/week, data persists via the stable `dev.tgorka.keeper` bundle id), the optional AltServer Wi-Fi auto-refresh (with its preconditions), and the Sideloadly/zsign re-sign path for sharing IPAs without Xcode. The **Limitations** section reproduces the in-app "On this iPhone" disclosure (`IOS_DISCLOSURE_LINES`) verbatim, names that file as the source of truth, and links the in-app "What's different on iPhone" affordance (`IOS_DOCS_URL`) to a document that explains it. Docs-only: no code, config, CI, or secrets.

**Files changed:**
- `docs/ios.md` — rewritten/expanded into the full Mac+iPhone free-signing walkthrough; all pre-existing 12.1 build-setup content preserved and integrated.

**Review findings:** 0 intent_gap, 0 bad_spec, 6 patches applied (2 medium: first-install step reordering, tester on-device-setup pointer; 4 low: env-var phrasing precision, AltServer preconditions, per-shell env note, de-ticketed forward pointer), 0 deferred, 15 rejected (verified-valid command, appropriately-cited/undated-but-current facts, refuted or by-design items). Adversarial review independently verified the doc against the repo: all load-bearing claims (iOS 16.0, bundle id, CI gate, XcodeGen 2.45.4, no committed team id, the four disclosure strings + two sync/badge sentences) check out, no factual errors, no secret leaks.

**Verification:**
- `git diff --name-only` → only `docs/ios.md`.
- Four Limitations lines diffed byte-exact against `IOS_DISCLOSURE_LINES` in `about-section.tsx` → verbatim match (4/4), no extras.
- Secret scan → no real Team ID (only the `XXXXXXXXXX` placeholder), no `.p12`/`syt_`/profile UUIDs (the sole `.p12` mention is the "keep out of the repo" instruction).
- `--export-method debugging` confirmed a valid Tauri 2.x CLI value; `APPLE_DEVELOPMENT_TEAM` confirmed the correct env override via the `@tauri-apps/cli` config schema.
- `bun run check` → green (biome + tsc + 1245 vitest tests); unaffected, as no code changed.

**Follow-up review recommended:** false — the review changes are six localized, low/medium-consequence documentation edits (step reordering, phrasing precision, added caveats) with no behavior/API/security/data impact; the adversarial pass already verified factual accuracy and the verbatim disclosure match.

**Residual risks:**
- On-device validation of the documented path (a real free-signing install + one Sideloadly re-sign) is deferred to Story 15.6 (owner's physical iPhone), and Story 12.6's device gate is not yet run — the walkthrough is authored from the proven build setup (12.1–12.3) plus the iOS research report, not from a personally-executed device run.
- The Limitations list and the two quoted sync/badge sentences are hand-mirrored from the TypeScript source; only the "edit both together" note guards against future drift (no automated equality test). Verbatim-correct at this revision.
