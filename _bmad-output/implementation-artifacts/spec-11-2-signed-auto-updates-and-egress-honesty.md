---
title: 'Signed Auto-Updates and Egress Honesty'
type: 'feature'
created: '2026-07-06'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '2203a6f456353db2618167603df54ef256cc329d'
final_revision: '64608eb15af6b8c3f847cb81f6695df01961ccba'
context:
  - '{project-root}/docs/project-context.md'
warnings: [multiple-goals, oversized]
---

<intent-contract>

## Intent

**Problem:** keeper ships no auto-update path (no `tauri-plugin-updater`, no signed-update endpoint, no in-app update check) and makes no verifiable claim about where it sends network traffic — NFR-11's "egress honesty" and NFR-12's signed auto-updates are asserted in docs but not realized. Story 11.1 built the signing/notarization pipeline; this story makes trust *verifiable* rather than asserted.

**Approach:** Wire `tauri-plugin-updater` (+ `tauri-plugin-process` for relaunch) with a GitHub-releases signed-update endpoint and an in-app "check for updates" control; add a Rust `egress_list` command that computes the live set of network destinations from actual app state (each account's homeserver, `api.beeper.com` when a Beeper account exists, the update endpoint) and render it as UI under Settings → About; and emit a per-release egress diff note from the release job with `docs/egress.md` as the canonical, diffable egress surface. Updater private key + signing stay CI-secret/release-provisioned exactly like the Apple secrets in 11.1.

## Boundaries & Constraints

**Always:**
- The egress list is computed from **live app state** — accounts registry (`homeserver_url` + `provider`) + Beeper presence + the update endpoint — never hardcoded, faked, or omitting a real destination; it renders as UI in Settings → About (not a doc link). Duplicate homeservers collapse to one entry.
- Beeper presence follows the existing single source of truth: `provider == Beeper` OR homeserver host is Beeper's (`is_beeper_homeserver`); when true, and only then, `api.beeper.com` appears (once).
- The updater is wired end-to-end: Rust plugin registered, `plugins.updater` config (endpoints + pubkey), `bundle.createUpdaterArtifacts: true`, capabilities permissions, JS plugin, and an in-app flow that detects → downloads → verifies (via the committed pubkey) → installs via the plugin. Updater/verification errors surface as a rendered state — no `.unwrap()`/panic, no silent failure.
- All signing material (updater private key, Apple secrets) comes only from GitHub Actions secrets/env; the committed `pubkey` is a build-valid scaffold documented in `docs/release.md` for maintainer replacement.
- New Rust VM types are `#[serde(rename_all = "camelCase")]` + `#[ts(export)]`; regenerate `src/lib/ipc/gen/` via `bun run test:rust` and commit it (`bindings:check` must stay green).
- New dependencies pass the license firewall (`cargo deny check licenses bans sources` + `bun run check:licenses`). English everywhere; bun only; Biome + rustfmt; TS strict (no `any`, `import type`); no `.unwrap()` in Rust prod paths. Commit on the current branch only — no branch/push/history changes.

**Block If:**
- `cargo deny check` or the JS license gate flags `tauri-plugin-updater`, `tauri-plugin-process`, `@tauri-apps/plugin-updater`, `@tauri-apps/plugin-process`, or a transitive dep as GPL/AGPL/SSPL/copyleft — that is a dependency-contamination decision for a human, not a policy loosening.
- Tauri v2's updater cannot express a GitHub-releases static-`latest.json` endpoint with pubkey verification through `tauri.conf.json` (i.e. it would require a bespoke update server keeper does not run) — an alternative update-distribution path is a human decision.

**Never:**
- Never add telemetry, analytics, or crash reporting (no opt-in scaffolding either — there is nothing to add); never fabricate, omit, or stale-cache egress entries.
- Never commit the updater private key, Apple certificates, `.p8`, passwords, or signing identities; never hardcode secrets.
- Never implement Story 11.3 (perf/reliability CI gates); never put Matrix/network logic in TypeScript; never use npm/yarn/pnpm; never loosen the license allowlist to make a check pass.

## I/O & Edge-Case Matrix

Applies to the pure egress computation (`compute_egress(accounts, update_endpoint)` → `Vec<EgressEndpointVm>`).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| No accounts | `[]` | Exactly one entry: the update endpoint (`kind = update`) | No error |
| One non-Beeper account | password acct on `https://matrix.example.org` | `[homeserver matrix.example.org, update]` — no `api.beeper.com` | No error |
| One Beeper account | provider `Beeper` (or host `matrix.beeper.com`) | `[homeserver matrix.beeper.com, beeper api.beeper.com, update]` | No error |
| Multiple accounts, same homeserver | two accts on `matrix.example.org` | homeserver listed **once** (dedup); update endpoint present | No error |
| Multiple Beeper accounts | two Beeper accts | `api.beeper.com` appears **once** | No error |
| Malformed homeserver URL | `homeserver_url` unparseable | entry still shown using the raw stored string; not treated as Beeper | No panic; no `.unwrap()` |
| Updater check fails offline | plugin `check()` rejects | in-app state = "couldn't check for updates" (rendered), retriable | Error surfaced, not thrown to console-only |

</intent-contract>

## Code Map

- `src-tauri/Cargo.toml` -- workspace deps; add `tauri-plugin-updater = "2"` and `tauri-plugin-process = "2"` (follow the `tauri-plugin-autostart` line).
- `src-tauri/crates/keeper/Cargo.toml` -- add both plugins as `{ workspace = true }`.
- `src-tauri/crates/keeper/src/lib.rs` -- register `.plugin(tauri_plugin_updater::Builder::new().build())` and `.plugin(tauri_plugin_process::init())` after the autostart plugin (line ~41); add `ipc::egress_list` to the `generate_handler!` list (line ~130).
- `src-tauri/crates/keeper/tauri.conf.json` -- add `plugins.updater` (`endpoints`, `pubkey`) and `bundle.createUpdaterArtifacts: true`.
- `src-tauri/crates/keeper/capabilities/default.json` -- add updater + process permissions (`updater:default`, `process:default` / `process:allow-restart`).
- `src-tauri/crates/keeper-core/src/egress.rs` -- **NEW** module: `EGRESS_UPDATE_ENDPOINT` const + pure `compute_egress(...)`; unit tests for the I/O matrix.
- `src-tauri/crates/keeper-core/src/lib.rs` -- declare `pub mod egress;`.
- `src-tauri/crates/keeper-core/src/vm.rs` -- **NEW** `EgressEndpointVm { url, kind, label }` + `EgressKind` enum (`Homeserver | Beeper | Update`), camelCase + `#[ts(export)]`.
- `src-tauri/crates/keeper/src/ipc.rs` -- **NEW** `egress_list` command: read accounts (homeserver_url + provider) from the same registry path `session_restore` uses, feed `compute_egress`, return `Vec<EgressEndpointVm>`.
- `src/lib/ipc/client.ts` -- add `egressList()` wrapper + re-export `EgressEndpointVm`/`EgressKind` from `./gen`.
- `src/lib/ipc/gen/EgressEndpointVm.ts`, `EgressKind.ts` -- **GENERATED** by ts-rs (`bun run test:rust`); commit.
- `src/components/settings/about-section.tsx` -- **NEW** About section: renders the egress list + a "Check for updates" control using `@tauri-apps/plugin-updater` (+ `plugin-process` relaunch).
- `src/components/settings/about-section.test.tsx` -- **NEW** tests (mock `egressList` + the updater/process plugins).
- `src/components/settings/settings-dialog.tsx` -- render `<AboutSection open={open} />` after `SetupSection`.
- `package.json` -- add `@tauri-apps/plugin-updater` and `@tauri-apps/plugin-process` deps.
- `.github/workflows/release.yml` -- add `TAURI_SIGNING_PRIVATE_KEY` + `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` env (from secrets) to the tauri-action step; set `fetch-depth: 0` on checkout; add an "Egress diff note" step.
- `docs/egress.md` -- **NEW** canonical egress surface + the no-telemetry invariant.
- `docs/release.md` -- document updater keypair provisioning, the two updater secrets, and the egress diff note.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/Cargo.toml` + `src-tauri/crates/keeper/Cargo.toml` -- add `tauri-plugin-updater` and `tauri-plugin-process` (workspace pattern).
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register both plugins after autostart; add `ipc::egress_list` to the handler list.
- [x] `src-tauri/crates/keeper/tauri.conf.json` -- add `plugins.updater.endpoints = ["https://github.com/tgorka/keeper/releases/latest/download/latest.json"]`, `plugins.updater.pubkey = <committed build-valid public key>`, and `bundle.createUpdaterArtifacts = true`.
- [x] `src-tauri/crates/keeper/capabilities/default.json` -- add updater + process permissions.
- [x] `src-tauri/crates/keeper-core/src/egress.rs` (+ `lib.rs` mod) -- `EGRESS_UPDATE_ENDPOINT` const and pure `compute_egress`; unit-test **every** I/O Matrix row.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- `EgressEndpointVm` + `EgressKind` (camelCase, `#[ts(export)]`).
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- `egress_list` command wiring registry accounts → `compute_egress`.
- [x] `src/lib/ipc/client.ts` -- `egressList()` + type re-exports; run `bun run test:rust` to regenerate & commit `gen/`.
- [x] `src/components/settings/about-section.tsx` + `.test.tsx` -- egress list rendering + update-check flow (load-on-open + unmount-guard pattern; errors → rendered state).
- [x] `src/components/settings/settings-dialog.tsx` -- mount `<AboutSection open={open} />`.
- [x] `package.json` -- add `@tauri-apps/plugin-updater`, `@tauri-apps/plugin-process`.
- [x] `.github/workflows/release.yml` -- updater signing env + `fetch-depth: 0` + "Egress diff note" step.
- [x] `docs/egress.md` + `docs/release.md` -- canonical egress surface + updater/egress runbook.

**Acceptance Criteria:**
- Given a Beeper account plus a non-Beeper account on two distinct homeservers, when Settings → About renders, then the egress list shows both homeservers, `api.beeper.com` once, and the update endpoint — as UI, each derived from live state (NFR-11, UX-DR17).
- Given no Beeper account, when Settings → About renders, then `api.beeper.com` does **not** appear.
- Given the built app configured with `plugins.updater` + `createUpdaterArtifacts`, when the user triggers "check for updates", then the app detects/downloads/verifies/installs via the updater plugin, and any failure (offline, bad signature) renders as an honest state rather than crashing (NFR-12).
- Given a pushed `v*` tag, when the release job runs, then it signs updater artifacts with `TAURI_SIGNING_PRIVATE_KEY` and emits an egress diff note (diff of `docs/egress.md` vs the previous tag) to the job summary (NFR-11, AD-23).
- Given the repo, when `cargo deny check licenses bans sources` and `bun run check:licenses` run, then both exit 0 with the new updater/process dependencies present.

## Design Notes

**Updater keypair (the one human release-provisioning step).** The committed `pubkey` is a build-valid scaffold — generate a real keypair with `bun tauri signer generate` (or `npx @tauri-apps/cli signer generate`), commit only the public key, and do **not** persist the private key anywhere in the repo. `docs/release.md` must state: to actually ship updates the maintainer generates their own keypair, replaces `plugins.updater.pubkey`, and stores `TAURI_SIGNING_PRIVATE_KEY` + `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` as GitHub secrets. This mirrors 11.1's Apple-secret model: full code/config wiring now, real signing verified at release time. `--no-bundle` builds skip updater artifact creation, so CI's bundle-less build and `bun run check` stay green without the private key.

**Egress endpoint set.** Bridges are Matrix appservices reached **through** the homeserver (server-side), so they do not add distinct client egress — the homeserver entry is their egress. Do not fabricate per-bridge hosts. The update endpoint is a shared Rust const (`EGRESS_UPDATE_ENDPOINT`) that must stay in sync with `tauri.conf.json`'s updater endpoint (call this out in a code comment). Keep `compute_egress` pure over `[(homeserver_url, Provider)]` so the whole matrix is unit-testable without a Tauri runtime.

**About section** (follow `NotificationsSection`/`EncryptionSection` in `settings-dialog.tsx`): load-on-open, `let cancelled = false` unmount guard, render `<ul>`; group under the existing `.mt-2 .flex .flex-col .gap-2 .border-border .border-t .pt-3 .text-sm` section style. Update-check states: idle / checking / up-to-date / available(version) / downloading / error. Tests mock `@/lib/ipc/client` and the two `@tauri-apps/plugin-*` modules.

**Egress diff note** (release.yml): `prev=$(git describe --tags --abbrev=0 "${{ github.ref_name }}^" || true)`; if set, append `git diff "$prev" "${{ github.ref_name }}" -- docs/egress.md` to `$GITHUB_STEP_SUMMARY`, else append `docs/egress.md` as the initial baseline. Requires `fetch-depth: 0` on the checkout.

## Verification

**Commands:**
- `bun run tauri:build -- --no-bundle` -- expected: compiles with the updater + process plugins and `plugins.updater`/`createUpdaterArtifacts` config; confirms the placeholder pubkey does not break the build.
- `bun run test:rust` -- expected: `egress::compute_egress` matrix tests pass; regenerates `src/lib/ipc/gen/EgressEndpointVm.ts` + `EgressKind.ts` (commit them).
- `bun run bindings:check` -- expected: clean (`gen/` committed).
- `bun run check` -- expected: biome + tsc + vitest pass, incl. `about-section.test.tsx`.
- `bun run check:rust` -- expected: rustfmt + clippy `-D warnings` pass.
- `cd src-tauri && cargo deny check licenses bans sources` -- expected: `licenses ok, bans ok, sources ok` (new plugin deps are MIT/Apache).
- `bun run check:licenses` -- expected: exit 0.
- `actionlint .github/workflows/release.yml` if available -- expected: no errors; else review the added env + egress step structure manually.

**Manual checks (release-time, cannot run unattended without the updater private key):**
- After a real tagged release: `latest.json` and signed updater artifact are attached to the GitHub release; a prior-version app checks, downloads, verifies against the committed pubkey, and applies the update; the release job summary shows the egress diff note.

## Review Triage Log

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 9: (high 0, medium 5, low 4)
- defer: 0
- reject: 8
- addressed_findings:
  - `[medium]` `[patch]` A mere "Check for updates" click auto-downloaded, installed, AND relaunched the app (losing any in-flight compose/draft in the webview), and the `"available"` state was set-then-overwritten in the same tick (dead UI). Split into a **two-step consent flow**: check surfaces "Update X available" with an explicit "Download and install" button; download/verify/install/relaunch happen only on that second click.
  - `[medium]` `[patch]` The egress honesty copy claimed "every server keeper talks to" but omitted the GitHub asset CDN (`*.githubusercontent.com`) that release-asset downloads 302-redirect to. Disclosed the CDN in the About copy and added a row to `docs/egress.md`, so the "exhaustive egress" claim holds.
  - `[medium]` `[patch]` `EGRESS_UPDATE_ENDPOINT` (Rust) and `tauri.conf.json` `plugins.updater.endpoints` could silently drift, making the egress list dishonest. Added `egress_update_endpoint_matches_tauri_conf` (keeper crate) that `include_str!`s the config and fails the build on divergence.
  - `[medium]` `[patch]` The release egress-diff step interpolated `${{ github.ref_name }}` directly into the shell (script-injection boundary) and blindly `|| true`-swallowed git failures (a false "no change" defeats the review gate). Moved the ref to a `REF` env var, guarded the `cat` under `set -e`, and now distinguish "no change" / real diff / diff-failed / missing-file explicitly.
  - `[medium]` `[patch]` A drafted release breaks the `releases/latest/download/latest.json` endpoint until published (GitHub's `/releases/latest/` never resolves to a draft) — updates silently undelivered. Added a prominent note to `docs/release.md` step 5 that the release must be Published for auto-updates to go live.
  - `[low]` `[patch]` On a successful install followed by a failed `relaunch()`, the UI overwrote the state with a misleading "check failed" message. Added a distinct `installedNeedsRestart` state ("Update installed. Restart keeper to finish.").
  - `[low]` `[patch]` The update-flow promise chain lacked the unmount guard the docstring claimed (only the egress load had one). Added a `mounted` ref + `setUpdateSafe` wrapper so no async resolution sets state after unmount.
  - `[low]` `[patch]` `errorMessage()` could render "[object Object]", "undefined", or a dangling colon for a non-string/empty/object-valued `message`. Hardened it to fall back to a generic line.
  - `[low]` `[patch]` `api.beeper.com` was a private hardcoded literal in `egress.rs` while the doc claimed derived-from-live-state. Made `keeper-core::auth::BEEPER_API_BASE` public and reused it (single source of truth, mirroring how Beeper detection reuses `BEEPER_HOMESERVER`); tightened the doc wording.
- notes: Rejected 8 as noise/by-design: raw-string homeserver dedup (resolved URLs are canonical; over-listing is the safe direction), empty `update_endpoint` (unreachable — non-empty const), empty `homeserver_url` (schema is `NOT NULL`; login flows never emit empty), `check()` returning `available:false` non-null (v1-API assumption; v2 `check()` returns `Update | null`), Update resource-lifecycle test nit, updater/process perms on the default capability (single-window app, standard), settings-dialog integration-test coverage (covered in `about-section.test.tsx`), and the single macOS-only updater endpoint (macOS-first by design). No intent_gap and no bad_spec: every finding was fixable within the diff without amending the frozen `<intent-contract>`.

### 2026-07-06 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 1
- reject: 15: (high 0, medium 0, low 15)
- addressed_findings:
  - `[low]` `[patch]` The "Download and install" flow could (a) launch a second concurrent `downloadAndInstall()` on the same `Update` handle via a rapid double-click (the install button, unlike "Check for updates", had no busy guard) and (b) hang forever on "Downloading and verifying…" if `relaunch()` resolved without actually restarting (only the throw path set a terminal state). Now consume `pendingUpdate.current` at install start (a second click finds `null` and bails; retry is a re-check) and set the terminal `installedNeedsRestart` state after `relaunch()` resolves as well as on throw; strengthened the success-path test to assert the flow never stays stuck on "downloading".
- notes: Deferred 1 — no build/release-time check binds `TAURI_SIGNING_PRIVATE_KEY` to the committed `plugins.updater.pubkey`, so a key/pubkey mismatch would ship a "signed" `latest.json` that every client silently rejects; inherent to the frozen scaffold + human-release-provisioning model (not a spec defect, not auto-patchable — a release-process decision), logged to the deferred-work ledger. Rejected 15 as noise/by-design/already-mitigated: raw-string homeserver dedup produces no duplicates because the stored `homeserver_url` is the SDK-canonical `probe.homeserver()` string (two accounts on one homeserver store byte-identical values); `process:allow-restart` is required (the JS `relaunch()` invokes the plugin `restart` command); aarch64-only `latest.json` is macOS/arch-first by design (prior-rejected); static app version is covered by `docs/release.md` step 2 ("bump the version"); the GitHub asset CDN is disclosed in the About copy and `docs/egress.md`; the release egress-note `if: always()` is informational and its `cat docs/egress.md` is guarded by a prior `-f` existence check; the `git describe` baseline is bounded to `v*` tag pushes and only affects an informational job-summary note; the synchronous `setUpdate` setters cannot fire after unmount (they run inside click handlers on a mounted component); non-HTTPS/malformed homeservers are shown verbatim per the frozen I/O matrix; the Beeper-tag-on-non-Beeper-host path is already covered by the `::::garbage` provider-tag test; the endpoint-sync test matches its documented claim (URL drift, not pubkey); a stale `pendingUpdate` after a re-check is unreachable (the install button renders only in the `available` state, which always re-arms it) and is now moot after the consume-on-install patch; empty `homeserver_url`/`update_endpoint` remain unreachable (`NOT NULL` schema; non-empty const with a drift guard); and a registry-read failure surfaces an honest "could not load" error rather than a partial/false-empty list (the safe direction). No intent_gap and no bad_spec: the one real code issue was a low-severity in-diff patch and the frozen `<intent-contract>` stands.

## Auto Run Result

Status: done

**Change summary.** Realized NFR-11 (egress honesty) and NFR-12 (signed auto-updates) as verifiable behavior. Wired `tauri-plugin-updater` + `tauri-plugin-process` with a GitHub-releases signed-update endpoint and an in-app two-step update control (check → consent → download/verify/install/relaunch) in Settings → About; added a pure, unit-tested `keeper-core::egress::compute_egress` + `egress_list` command that renders the live network-egress list from the accounts registry (homeservers, `api.beeper.com` iff a Beeper account exists, the update endpoint); and made the release job emit a per-release egress-diff note with `docs/egress.md` as the canonical, diffable surface. The updater private key stays a CI secret / release-provisioned (like the Apple secrets in 11.1); only the public pubkey is committed.

**Files changed.**
- `src-tauri/Cargo.toml`, `src-tauri/crates/keeper/Cargo.toml` — `tauri-plugin-updater` + `tauri-plugin-process` deps.
- `src-tauri/crates/keeper/src/lib.rs` — register both plugins; add `ipc::egress_list` to the handler list.
- `src-tauri/crates/keeper/tauri.conf.json` — `plugins.updater` (GitHub `latest.json` endpoint + committed build-valid pubkey) + `bundle.createUpdaterArtifacts`.
- `src-tauri/crates/keeper/capabilities/default.json` — `updater:default`, `process:default`, `process:allow-restart`.
- `src-tauri/crates/keeper-core/src/egress.rs` (new) — pure `compute_egress` + `EGRESS_UPDATE_ENDPOINT`; 9 matrix unit tests; reuses `BEEPER_API_BASE`.
- `src-tauri/crates/keeper-core/src/lib.rs` — `pub mod egress;`.
- `src-tauri/crates/keeper-core/src/vm.rs` — `EgressEndpointVm` + `EgressKind` (camelCase, `#[ts(export)]`).
- `src-tauri/crates/keeper-core/src/auth/beeper.rs`, `auth.rs` — made `BEEPER_API_BASE` public + re-exported (single source of truth for the egress list).
- `src-tauri/crates/keeper/src/ipc.rs` — `egress_list` command (reads `registry::list_accounts`) + `egress_update_endpoint_matches_tauri_conf` build-guard test.
- `src/lib/ipc/gen/EgressEndpointVm.ts`, `EgressKind.ts` (generated), `src/lib/ipc/client.ts` — `egressList()` + type re-exports.
- `src/components/settings/about-section.tsx` (new) + `about-section.test.tsx` (new) — About section: live egress list + two-step consent update flow; 9 tests.
- `src/components/settings/settings-dialog.tsx` (+ its test) — mount `<AboutSection open={open} />`.
- `package.json` / `bun.lock` — `@tauri-apps/plugin-updater`, `@tauri-apps/plugin-process`.
- `.github/workflows/release.yml` — `fetch-depth: 0`; updater signing env; injection-safe "Egress diff note" step.
- `docs/egress.md` (new), `docs/release.md` — canonical egress surface (+ CDN disclosure, sync/single-source notes); updater keypair provisioning + publish-required note.

**Review findings breakdown.** intent_gap 0, bad_spec 0. Patches applied: 9 (medium 5, low 4) — two-step update consent flow (fixes auto-relaunch-loses-drafts + dead state), GitHub asset-CDN egress disclosure, endpoint-sync build-guard test, CI egress-step injection+robustness hardening, draft-must-publish doc note, distinct relaunch-failure state, update-flow unmount guard, `errorMessage` hardening, `BEEPER_API_BASE` single-sourcing. Deferred 0. Rejected 8 (canonical-URL dedup, unreachable/`NOT NULL` boundaries, v1-API assumption, test/capability/coverage nits, macOS-first single endpoint).

**Verification performed (all green, re-run after patches).**
- `bun run check` → biome + tsc + vitest pass; 947 tests / 93 files (incl. 9 About-section tests).
- `bun run test:rust` → 753 tests passed (incl. 9 `compute_egress` matrix tests + the endpoint-sync guard).
- `bun run check:rust` → rustfmt + clippy `-D warnings` clean.
- `bun run tauri:build -- --no-bundle` → built (release profile); updater/process plugins + updater config + committed pubkey compile cleanly.
- `cd src-tauri && cargo deny check licenses bans sources` → `bans ok, licenses ok, sources ok` (new plugins are MIT/Apache; deps unchanged by the review patches).
- `bun run check:licenses` → 0 denied.
- `bindings:check` → only the two new generated files are added (no drift); goes fully green on commit.
- `release.yml` validated structurally (no YAML parser / actionlint in env): the "Egress diff note" step is injection-safe (`REF` env var) and `set -e`-correct.

**Residual risks.**
- Real signing/notarization + the updater keypair are release-time, human-provisioned (Apple secrets + `TAURI_SIGNING_PRIVATE_KEY`); the committed pubkey is a build-valid scaffold. First real `v*` tag must be verified per `docs/release.md` (codesign/spctl/stapler + a prior app applying the signed update). Not exercisable unattended.
- Auto-updates only go live once the drafted release is Published (documented); left as a draft, "Check for updates" reports up-to-date.
- The egress list treats bridges as riding the homeserver (server-side appservices) — correct for MVP; if a future story has keeper contact a bridge/provisioning host directly, the list and `docs/egress.md` must gain that entry.
- `followup_review_recommended: true` — this pass made 9 review-driven changes (5 medium) spanning update-flow behavior, a security-hardening CI edit, a security/honesty-claim doc+copy change, and a new build-guard, warranting an independent follow-up look.

---

**Follow-up review pass (2026-07-06).** An independent follow-up review (Blind Hunter + Edge Case Hunter, 21 raw findings) produced one low-severity in-diff patch, one deferred release-safety hardening, and 15 rejections; no `intent_gap` and no `bad_spec`, so the frozen `<intent-contract>` and the prior implementation stand.

- **Change (patch, low).** `src/components/settings/about-section.tsx` — hardened `onDownloadAndInstall`: consume `pendingUpdate.current` at install start so a rapid double-click can't launch a second concurrent `downloadAndInstall()` on the same handle, and set the terminal `installedNeedsRestart` state after `relaunch()` resolves (not only on throw) so the flow can never hang on "Downloading and verifying…" if `relaunch()` no-ops. `about-section.test.tsx` — strengthened the success-path test to assert the flow reaches the terminal restart-needed state.
- **Deferred (release-safety, logged to the deferred-work ledger).** Nothing binds `TAURI_SIGNING_PRIVATE_KEY` to the committed `plugins.updater.pubkey`; a key/pubkey mismatch would ship a "signed" `latest.json` that every client silently rejects. Inherent to the frozen scaffold + human-release-provisioning model (mirrors 11.1's Apple secrets) — a release-process decision, not a spec defect or an auto-fixable patch. Suggested fix: a CI step that verifies the emitted `latest.json` signature against the committed pubkey before the draft is accepted.
- **Notable rejections (verified against the code).** The headline reviewer finding — raw-string homeserver dedup producing duplicates — is not real: the stored `homeserver_url` is the SDK-canonical `probe.homeserver()` string, so two accounts on one homeserver store byte-identical values. `process:allow-restart` is required (JS `relaunch()` invokes the plugin `restart` command). Static app version and the aarch64-only `latest.json` are covered by `docs/release.md` / macOS-arch-first design. The GitHub asset CDN is disclosed in the About copy and `docs/egress.md`.

**Verification (follow-up patch).** `vitest run about-section.test.tsx` → 10/10 pass; `biome check` (both changed files) → clean; `tsc --noEmit` → clean. Rust and CI files untouched by this pass, so `test:rust`/`check:rust`/`cargo deny` results from the prior pass are unchanged.

- `followup_review_recommended: false` — this follow-up made a single localized, low-consequence UI-robustness patch (with a test); no further independent review is warranted.
