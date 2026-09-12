# Epic 71 — Integration verification record

Status: code implemented, independently reviewed and exercised through frontend, Linux, macOS, iOS compilation, production-browser and live-service gates. Existing dependency advisories, GitHub environment administration and coordinator publication remain explicit limitations; no all-green release/deployment claim.

## Scope and source

- Client slice: `e5ab18b` — Rust consent/export/IPC and generated types, renderer consumers, isolated study, dependency lock and egress documentation together.
- Maintainer slice: `35e9e3e` — provisioning, catalog/cohorts/Endpoints/marketing definitions, credential references, CI/release guards and runbooks.
- Both commits remain on the original `vigorous-chicken` branch. No branch creation/switch, push, history rewrite or PR creation occurred.
- The owner chose existing clients/tooling, not a new Keeper server. Authenticated customer-facing analytical queries and mobile/server/desktop propagation are documented future contracts, not deployed services. No ads integration, cross-device attribution, human metric approval or Replay Vision/AI approval is claimed.

## Exercised client boundary

- Consent is Rust-owned and installation-local (`telemetry-consent-v1.json`), never imported from synchronized TOML. Diagnostics, product statistics and remote configuration default independently off. No installation ID is minted before consent; disabling all categories drops it.
- Closed event kinds and bounded numeric durations exclude arbitrary strings, exception payloads, logs, URLs and identifiers. Normal app code never initializes PostHog's DOM SDK. Product/error events disable person profiles and GeoIP enrichment; the service still sees transport IP addresses.
- Queue capacity 128, 60 admissions/minute, bounded JSON/flags bodies, five-second requests, no redirects/retries/disk spool, and a 60-second queue age limit. Revocation tests prove queue invalidation and cancellation of JSON/log/span collectors. Export failure is not placed on the operational critical path.
- An independent OpenTelemetry protobuf implementation decoded actual Rust-produced log/span bytes, including canonical Status field 3, ERROR code 2, matching W3C trace/span IDs and observed timestamp. Mutating the Status field back to 2 lost ERROR status. Live native OTLP readback confirmed the correlated log/span; uppercase service-side hex was normalized for comparison only.
- Actual Rust JSON batch ingestion produced the custom diagnostics event and a native grouped error issue. HTTP 200 alone was not treated as native error/log/span delivery evidence. Disposable senders added synthetic markers; production envelopes remain closed.
- Actual Rust remote-config fetch succeeded against the public flag; no analytics event was queued, and revocation was exercised. A plain-text support-message renderer exists in Settings; remote payloads cannot change consent, security, grants or destinations.
- Readiness timing uses navigation-relative `performance.now()` after hydrated non-splash rendering, not the later Settings-open time. App controls and independent settings toggles were exercised through the real frontend with explicitly identified IPC fixtures.

## Production study proof

The actual Vite production bundle ran in isolated Chromium on hesperia, with a minimal Tauri IPC fixture and the installed Tauri JavaScript window/event implementation. This is browser evidence, not an installed native-window or physical-iPhone runtime claim. Headless bot detection was disabled only in the harness so PostHog would accept synthetic events.

- Before Start: no collector request, recorder chunk load, study activation or installation-state command. URL query/hash removed before capture.
- After Start: actual public config, `/i/v0/e/` and `/s/` requests returned 200; decoded gzip batches contained `$pageview`, `$snapshot` and `$$heatmap`. Exact study IDs were read back from PostHog separately to establish storage.
- Query/hash, DOM text, attribute, console, request/response URL/header/body sentinels were absent from decoded payloads. Inputs/media/canvas are blocked; text/attributes masked. Identity stays in memory; only transient SDK storage probes (`__mplssupport__`, `test`) occurred, with no persistent local/session storage or cookies remaining.
- A hostile remote-config response enabled console/network/canvas capture in the harness. Actual recorder options still disabled canvas, network headers/bodies/performance and console plugins; private markers remained absent. This did not mutate project capture settings.
- Stop, real page hiding, injected collector 401 and the actual Tauri close-request wrapper each cleared disclosure only after capture shutdown. Direct fetch/beacon attempts after Stop did not reach the original transport; no late collector traffic occurred.
- Native-close regression: the real installed Tauri wrapper destroyed the main window before the fix, in both the production browser fixture and the permanent regression. The handler now prevents default synchronously and awaits shutdown, preserving Keeper's existing hide policy. A separate microtask race regression proved that startup failure cannot be overwritten by a late active transition.
- Controls were driven at 360/720/1100 CSS pixels; screenshots and measured geometry show no study overflow. Settings' telemetry section measured 328/688/912 pixels respectively with controls inside the viewport. Other pre-existing Settings overflow is not claimed fixed.
- Cancel retained the current document; confirmed Leave replaced the whole document before returning to App. The minimal fixture intentionally cannot hydrate the normal application after return; that fixture error is not native-runtime evidence.
- Real-SDK verification found and fixed regional config routing (`asset_host` does not cover `/array`), the modern `/i/v0/e/` collector route, and required heatmap viewport dimensions. Relevant regressions failed before and passed after their fixes. Heatmap API readback confirmed actual coordinates, not just accepted ingestion.
- Stored synthetic recordings include `01a09341-4752-70ed-8a4e-38e92628da9b` (16751 bytes, 30 rrweb events), production session `01a09361-58ee-78bf-9f0d-e8bb9877696d` (14767 bytes, 30 events), hostile-config production session `01a09378-990b-7b14-a2ab-1e20a29bf77b` (14809 bytes, 30 events), and final ordinary production session `01a09386-94d7-7711-961f-0824f4c28d47` (14806 bytes, 30 events). The two final sessions have zero console logs/errors and no person profile. Final heatmap readback returned three coordinate points and 15 interactions over the disclosed synthetic URL/time window.
- Receipt limitation: one earlier accepted session (`01a0937a-90c5-75ea-a3d5-4d0a91cd15a7`) remained 404 after 60 bounded read attempts. Its absence is unexplained, not relabelled success or assumed latency. Subsequent production capture stored successfully with matching pageview/snapshot session IDs. This best-effort analytics path does not promise that every HTTP-accepted event is durably stored.

## Maintainer and credential proof

- Dedicated project `605256` / `keeper`: management `https://us.posthog.com`, ingestion `https://us.i.posthog.com`. Authenticated read-only lookup verified the Keeper-specific 1Password token match. No privileged value was displayed or stored in source; public GitHub build variables were set through stdin.
- tgsite's reference convention is reused, not its project. Public client and privileged maintainer `.env.1p` templates are separate. `op run` inherits ambient variables, so build verification used an environment whitelist plus a synthetic private-key canary, never the real personal key.
- Reviewed manifest SHA-256: `e4b8b6fa83fdac176beba7868779452fa7d4e8f8a4769a7369b91b75c63c8301`. Apply/readback and repeat verification returned every resource unchanged. Public flag fetch, exact synthetic UUID observation and all four private analytical Endpoints succeeded.
- API-specific corrections include normalized tags, 900-second Endpoint cache TTL, marketing `schema_map`, and explicit `confidence: null` on proposed metrics. Model provenance remains truthful; approved metrics cannot be automatically rewritten. Personless event-based installation segments execute; person-profile cohorts are not claimed populated.
- Replay ingestion was explicitly enabled only after the isolated privacy boundary passed. No AI-processing approval setting was changed.
- The protected workflow checks the actual sole reviewer (`tgorka`, ID 1956779), exact main-only branch policy, complete policy pagination and `can_admins_bypass === false` in a secret-free job before the personal-key job. Regression tests reject absent/wider protection. Existing `POSTHOG_PERSONAL_API_KEY` is reused, not duplicated.
- Environment creation returned HTTP 403 (`Resource not accessible by personal access token`). `posthog-maintainers` protection is therefore **not configured**. A repository administrator must complete it; a YAML environment name alone is insufficient.
- Release guards run before upload over frontend assets, the actual uncompressed app and required main/Swift sidecar executables. Syncd upload depends on app success and scans its actual packaged executable. Draft publication remains a human decision. No signed release or release-workflow execution is claimed.
- Streaming scanner smoke: 14 cases passed, including 64-KiB boundaries, long inherited session values, decoded signing material, missing/empty/symlink roots and public-token allowance. Matched values were never printed. Actual Mach-O inspection exposed the broad `ops_` false positive; the scanner now recognizes documented encoded service-account capsules instead. A regression accepts ordinary identifiers and rejects the capsule prefix.
- Source boundary references: [1Password service-account capsule security](https://www.1password.dev/service-accounts/security), [maintained detection grammar](https://raw.githubusercontent.com/gitleaks/gitleaks/master/cmd/generate/config/rules/1password.go). Unknown arbitrary secrets cannot be proven absent by patterns; environment separation remains primary.

## Gates and exact limitations

| Gate | Result |
|---|---|
| Final `taskset -c 0,1,2 bun run check` | Passed: Biome, TypeScript, 347 Vitest files / 5827 tests, all three architecture dependency guards. Existing four Biome warnings/one information message remain. |
| Client-only staged tree, without later operations/doc changes | TypeScript passed from an independent `git checkout-index` snapshot; native source and generated consumers are all in that same client commit. |
| `bun run check:design` | Passed: 424 web + 249 native files, theme arithmetic and bot palette checks. |
| Node provisioning/environment/scanner suites | 26 passed. |
| Linux core/sync nextest | 4474 passed, 1 skipped. |
| Linux core/sync clippy | Passed. No Linux shell-compilation claim. |
| macOS workspace fmt/clippy/tests | Passed in exclusive `keeper-posthog-71.LrwJse`, including all workspace tests and 333 generated bindings with no drift. Rust sources/manifests/lockfile compared byte-for-byte to the committed client with checksum rsync: no differences. |
| iOS keeper-library compile | Passed in that same exclusive checkout: `cargo check -p keeper --lib --target aarch64-apple-ios`. Emitted 36 unused-import/dead-code warnings; this was not an iOS clippy or physical-device gate. |
| Vite production build | Passed: 4451 modules; final scanner passed all 141 asset files. Build used only public configuration and a fake privileged-key canary. |
| Actual macOS app artifact scan | Exclusive unsigned debug app built successfully. Four explicit roots (dist, app tree, main executable, Swift sidecar) passed, 147 file visits. Debug linker emitted an unwind-table-size warning; no signed release or installation was performed. |
| JS licenses | 679 packages, zero denied; four pre-existing unknown-license warnings. |
| cargo-deny | Licenses/bans/sources passed. Advisories fail on baseline locked dependencies; not suppressed. |

Baseline advisories: `h2 0.4.15` (RUSTSEC-2026-0258); `quick-xml 0.39.4` (RUSTSEC-2026-0194/0195); `bitmaps 3.2.1` (RUSTSEC-2026-0247); `proc-macro-error 1.0.4` (RUSTSEC-2024-0370); `proc-macro-error2 2.0.1` (RUSTSEC-2026-0173); existing `unic-* 0.9.0` notices (RUSTSEC-2025-0075/0080/0081/0098/0100); `bisync 0.3.0` has a yanked-version warning. These versions were already present at the starting revision. This epic does not silently upgrade unrelated dependency families or claim the full deny gate green.

One earlier full frontend run hit the existing files-pane first-list restoration timing test; the targeted test and two subsequent full runs passed unchanged. No timeout widening, suppression or unrelated source change was used. Container memory pressure was handled through bounded jobs/CPU affinity, not by disguising OOM as a code failure.

## Independent review and residual risks

Telemetry, study privacy, provisioning and final boundary reviews were independent. Confirmed lifecycle, SDK routing, viewport, native error, OTLP and environment-protection findings were fixed with evidence. The final review accepted the four-concurrent/five-second study transport limit as a deliberate fail-closed resource policy, not a latency availability guarantee; the configured-origin fence permits public config paths, while the real SDK rewrites its lookup to the exact configured project. No claim of a broader authorization boundary is made.

The shared macOS scratch directory later showed a different build stamp, so it is not used as final source provenance. The exclusive checkout's build stamp is the expected pre-commit `f9a337d37a63-dirty`; checksum comparison proves its native sources match client commit `e5ab18b`. The verified unsigned app is retained at `hesperia:~/keeper-posthog-71.LrwJse/src-tauri/target/debug/bundle/macos/keeper.app`. LSP references for the edited recorder remained stale even after reload/absolute-path lookup; the tool defect was reported and actual callers recovered by exact search before editing.

## Stack handoff

The coordinator can publish three cumulative rungs without splitting a generated-type/consumer boundary:

1. `e5ab18b`: complete client consent, diagnostics, remote configuration and isolated studies.
2. `35e9e3e`: governed PostHog provisioning, marketing definitions and CI/release protection.
3. Final BMAD evidence/status commit: documentation only; no required code fix deferred here.

The originally proposed finer client split was consolidated because the actual settings/IPC/generated-type/study surface is coupled. The client-only snapshot typechecks; the full code tip passed the frontend and operations gates. Coordinator-owned branch topology, pushing, PR creation and CI publication remain outstanding. Do not present these local commits as published PRs, an installed app, or a signed release.
