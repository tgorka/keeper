# Epic 71 — Observability without private content

Status: implemented and independently reviewed; protected-environment administration and coordinator publication remain outstanding. Exact evidence and limitations: [integration record](../implementation-artifacts/epic-71-verification.md).

## Owner intent and scope

The owner requests PostHog observability and product tooling across Keeper, using the tgsite 1Password convention and GitHub's POSTHOG_PERSONAL_API_KEY where necessary. Keeper is public; every distributed binary is an untrusted client. The owner explicitly selected **Existing clients and tooling**: do not introduce a Keeper-owned server. Future server-mediated analytical Endpoints and mobile/server/desktop trace propagation are designed here, not represented as shipped services.

This is an explicit proposed replacement of the unconditional no-telemetry claim with **no collection or PostHog contact without the relevant local consent**. It does not authorize transmission of messages, filenames, paths, account identifiers, credentials, bot inputs/outputs, recordings, or notes. Existing recording zero-egress and on-device speech guarantees remain unchanged.

## Intent contract

### Always

- Keep operational state, user settings, permissions, and secrets in Keeper's existing stores.
- Separate optional diagnostics, optional product statistics, optional remote configuration, and explicitly initiated usability studies.
- Enforce data minimization before export, using a closed schema; exclude arbitrary log/error strings.
- Treat PostHog project ingestion tokens as public, forgeable identifiers, never authorization.
- Keep personal/secure API keys solely in maintainer processes or trusted CI secret environments. Never use VITE_ for a privileged key.
- Bound memory, event sizes, network deadlines and retries. Failure of observability never prevents normal app use.
- Disclose active destinations from the same state controlling network use.
- Default new/existing installations to off. Revocation discards queued unsent data and stops future collection; already transmitted data cannot be recalled by a local toggle.
- Preserve standard OpenTelemetry trace context and keep instrumentation independent of PostHog's beta API.

### Block if

- Any privileged key reaches source, generated assets, binaries, artifacts, command output or fork-PR execution.
- Any SDK initializes or fetches flags before consent.
- A remote setting can enable collection, widen a grant, change a server destination, weaken encryption, or override a user's settings.
- A test/demo capture can observe real account, drive, note, bot or recording content.
- A claim of platform support lacks an exercised build/runtime gate.

### Never

- Forward the existing app log wholesale.
- Capture DOM text, URL queries, network bodies, console values, raw exception messages, credentials or local paths from the normal app.
- Identify a person using Matrix IDs or email, or link devices implicitly.
- Put PostHog on the critical sync, startup, authorization or persistence path.
- Treat analytical events as a durable audit ledger or an authorization source.
- Enable Replay Vision or other AI processing implicitly through ordinary diagnostics consent.

## Stories and acceptance

### 71.1 — Consent and destination honesty

Rust-owned versioned consent and generated view models; reachable settings controls; fail-closed persistence/validation; live destination disclosure; policy documentation updated only with working enforcement. Acceptance: a fresh install makes zero PostHog requests; enabling one category enables only that category; disabling clears unsent records and stops capture across windows; corrupt state fails closed.

### 71.2 — Bounded diagnostics and traces

Content-free structured diagnostic records, operation timings and OpenTelemetry-compatible trace export. Instrument real client operations with closed operation names and numeric outcomes; retain local logs unchanged. Acceptance: real operation produces connected spans and correlated diagnostic records; invalid attributes cannot serialize; offline/export failure does not block the operation; queue/time limits hold; no recording path is exported.

### 71.3 — Product statistics, errors and performance

Explicit closed-vocabulary product events and sanitized error categories; application readiness and interaction duration measurements rather than claiming web vitals measure Rust. Acceptance: reachable controls produce the declared event only with product consent; errors never include exception payloads; sampling and missing clients are documented in metric definitions.

### 71.4 — Maintainer remote configuration

Read public non-secret configuration through PostHog flags only after separate consent. Validate a small fixed schema with safe shipped defaults; demonstrate a reachable configuration effect. Never synchronize user preferences through PostHog person properties. Acceptance: malformed/offline values fall back; payload cannot change security/collection/destinations; user preferences win.

### 71.5 — Restricted usability studies

Implement a content-isolated synthetic study surface supporting replay and heatmaps after explicit per-session start. Real account/drive/bot/recording components cannot mount in that surface. Stop/unmount ends capture. AI analysis is a separate maintainer-side approval and scope, not an automatic project setting. Acceptance: browser study produces replay/heatmap evidence, private content sentinel remains absent from captured requests, normal app never starts DOM capture.

### 71.6 — PostHog project tooling

Safe idempotent maintainer provisioning for a Keeper-specific project: explicit events, cohorts/personas, governed metric catalog, remote-config defaults and analytical Endpoints. Dry run is default; apply requires explicit invocation; responses and errors never print secrets. Reuse tgsite's credential *reference conventions*, not its analytics project. Acceptance: dry run is non-mutating; repeated apply creates no duplicates; live API readback confirms managed resources without exposing keys. Human metric approvals and AI-processing approvals are not forged by automation.

### 71.7 — Marketing and future-server contracts

Provide maintainer tooling for campaign-source mappings and conversion definitions over explicitly consented events. No guessed ad-account integrations or cross-device attribution. Keeper repository has no marketing website: integration boundaries with tgsite are documented rather than modifying its live deployment silently. Analytical Endpoints are exercised through maintainer tooling only. Future server contract: authenticate caller, derive tenant scope server-side, proxy privileged queries; propagate traceparent only to owned participating hosts, use span links for delayed delivery and separate durable operation IDs. Acceptance: no query key in client; endpoint variables are never described as authorization; future server flow is labelled unimplemented.

### 71.8 — CI, security and platform evidence

Privileged provisioning runs only on an explicitly trusted/manual path, never pull_request_target or untrusted PR code. Build/test jobs receive no personal key. Inspect generated output for secrets using synthetic sentinels and secret-pattern guards without dumping matching content. Run frontend, Linux-buildable Rust, macOS shell and iOS gates; inspect actual application/browser requests and live PostHog ingestion. Acceptance: evidence names exactly which platforms/surfaces ran; no story is marked done on fixture-only evidence.

## Coordinator-owned PR stack

1. `e5ab18b`: complete client consent, diagnostics, analytics, remote configuration and isolated study surface, including all generated types and consumers.
2. `35e9e3e`: maintainer provisioning, governed catalog/cohorts/Endpoints/marketing definitions and CI/release protection.
3. Final BMAD evidence/status commit: documentation only, not a place for fixes required by earlier rungs.

The initially proposed finer client split was consolidated at the actual shared settings/IPC/generated-type boundary. The client-only indexed snapshot typechecks without later operations changes; the full code tip passed frontend and operations gates.

Any fix required for a rung to compile belongs in that rung, not the final one. The current checkout prohibits branch creation/switching/pushing by automation; commits remain on the starting branch and stack publication requires the coordinator. No gh stack mutation or push is authorized under those session rules.

## Initial grounding and prerequisites

- PostHog docs checked live: distributed tracing beta `/i/v1/traces`; logs OTLP; remote-config payloads via flags; Endpoints early beta; Data Catalog/Semantic Layer beta; heatmap viewer beta; marketing beta; Replay Vision sends rendered video/events to Gemini.
- `deploy/companion-stack/README.md:3-10`: configuration for Synapse/bridges, no Keeper-owned application service.
- `src-tauri/crates/keeper/src/debug_log.rs`: existing gated local tracing subscriber and rotated file; do not export its arbitrary strings.
- `src-tauri/crates/keeper-core/src/registry.rs:186-239`: settings read through local config overlays; collection consent must not be accidentally enabled by synced config.
- `src/lib/ipc/client.ts`: generated Rust view-model convention; no handwritten generated files.
- `src/components/settings/about-section.tsx:35-36`: unconditional no-telemetry disclosure must change with enforcement, not in advance.
- `.github/workflows/ci.yml`: public PR validation jobs; privileged integration work must remain separate.
- Installed BMAD module manifest identifies bmad-spec/build/architecture; their skill:// routes are unavailable in this session. This artifact follows the available BMAD story-chain intent/acceptance/stack procedure. Grounding completed against Keeper sources and tgsite's PostHog wrappers, privacy gates and 1Password references.
- Keeper-specific project verified by authenticated read-only API: project `605256`, name `keeper`, management `https://us.posthog.com`, ingestion `https://us.i.posthog.com`; its public token matches the Keeper-specific vault entry. No secret values were displayed. GitHub public build variables were set from that reference; the existing private management secret remains separate.

## Future server contract — deliberately not implemented in this epic

The owner selected existing clients and tooling. An authenticated Keeper API,
deployment, and server participation do not exist in this change.

### Trace propagation

- Use W3C `traceparent` and bounded `tracestate` only on explicitly owned,
  participating server boundaries. Do not add them to arbitrary homeserver,
  bridge, provider or repository requests.
- Trace metadata is untrusted data, never authentication. Reject malformed
  context and start a fresh trace; do not echo baggage or user-controlled
  span names/attributes.
- HTTP processing uses parent/child spans. Queued delivery and offline
  desktop processing use short spans plus causal links where supported,
  not one span held open while a phone or desktop is offline.
- Maintain durable operation IDs separately from sampled telemetry.
  Retries get attempt spans; deduplication depends on the operation ledger,
  not a trace ID.
- A server cannot grant telemetry consent on behalf of a client. Sampling
  and correlation must preserve each participating device's decision.
- Verify real mobile → service → desktop propagation, queue delay,
  cancellation, retries, clock skew, and missing spans before describing
  a future installation as end-to-end traced.

### Analytical Endpoints

- The server authenticates the user, derives an authorized tenant/user
  scope, and supplies that scope to a fixed allowlisted Endpoint. A caller's
  `user_id` variable is not authorization.
- Personal query credentials remain in the server's secret environment.
  No frontend or desktop binary receives them.
- Responses contain only authorized aggregates, with bounded query cost,
  timeouts and cache isolation. Avoid cross-tenant cache keys and permit
  no arbitrary SQL supplied by a client.
- Analytical query failure does not block message delivery, synchronization,
  settings reads, permissions or other operational state.

### Marketing and identity boundaries

Keeper has no marketing website in this repository. Existing event categories
contain no campaign parameters and no personal profiles. Marketing definitions
must label missing attribution as unknown; settings activity is engagement,
not a purchase, signup, install or customer conversion. Website-to-app and
cross-device attribution require a separate explicit design/consent boundary.
Personless event-based installation segments are usable analytical personas;
empty person-profile cohorts must not be presented as populated user segments.
