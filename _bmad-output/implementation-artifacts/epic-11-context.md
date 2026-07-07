# Epic 11 Context: Packaging, Release & Quality Gates

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

This epic turns keeper into a shippable, trustworthy macOS product. It stands up a reproducible CI release pipeline that produces signed, notarized Apple Silicon builds and delivers signed auto-updates, then wires the project's cross-cutting guarantees into that pipeline as enforced gates rather than aspirations. Three guarantees are made verifiable: the Apache-2.0 licensing firewall (no GPL/AGPL contamination), network egress honesty (keeper talks only to the endpoints the user configured plus the update endpoint, with that list rendered in-app), and the PRD's hard performance/reliability numbers (cold start, search latency, palette latency, memory, crash safety, bridge-health surfacing), measured in CI so regressions fail builds instead of reaching users. It can land any time after Epic 1 is buildable, with final validation gated at release.

## Stories

- Story 11.1: Signed, Notarized Release Pipeline
- Story 11.2: Signed Auto-Updates and Egress Honesty
- Story 11.3: Performance and Reliability Release Gates

## Requirements & Constraints

**Packaging & signing.** Release builds must be Developer-ID-signed with hardened runtime and notarized, native Apple Silicon (aarch64 first; universal is a later concern). Notarization uses an App Store Connect API key held in CI secrets. Builds must be reproducible from GitHub Actions.

**Auto-updates.** Updater artifacts must be signed with the Tauri updater key. The running app must detect, download, cryptographically verify, and apply updates through the updater plugin.

**Licensing firewall (must block, not warn).** keeper is Apache-2.0. No GPL/AGPL code or crates on either the Rust or npm side; the check runs in CI and blocks merges. AGPL-ecosystem projects are study-only; MPL files are never ported. Every PR that ports code must carry a provenance note (PR template checklist).

**Required PR checks.** Dependency-license check (cargo-deny plus the equivalent policy for JS deps), formatting and lint at zero-tolerance (rustfmt, clippy `-D warnings`, biome, tsc), unit tests (vitest, cargo-nextest), and a bundle-less build compile. All are required checks, not advisory.

**Egress honesty.** The only permitted network destinations are user-configured homeservers/bridges, Beeper's API when a Beeper account exists, and the signed-update endpoint. No telemetry, analytics, or crash reporting may exist without explicit opt-in. The egress surface must be documented and diffable per release — the release job emits an egress diff note, and the app itself renders the live egress list as UI (not a doc link).

**Performance gates (measured on reference Apple Silicon, seeded 100k+-event archive):**
- Cold start to interactive inbox under 2 s.
- Full-text search first results under 200 ms p95 (extends the search story's existing test to the 100k+ corpus, offline).
- Command-palette results within ~100 ms at 10k chats.
- Idle memory recorded for sign-off against the assumed budgets (~500 MB with 5 accounts, ~300 MB with 1); these numeric budgets are assumption-tagged and need owner confirmation before they become hard gates — measure and flag if over, rather than silently failing.

**Reliability gates.**
- Crash safety: killing the process mid-write (archive ingest, outbox insert, settings write) must relaunch to a consistent state with zero lost previously-persisted events.
- Bridge-health surfacing: an induced bridge-session drop must be reflected and notified within 60 s; verified as part of the release checklist.

## Technical Decisions

**Release pipeline (AD-23).** GitHub Actions on macOS arm64 using tauri-action. It performs Developer ID signing, hardened runtime, and notarization, and produces three outputs per release: the dmg, the signed updater bundle, and an egress diff note. cargo-deny and all quality gates are required PR checks on this pipeline.

**Licensing & egress firewall (AD-5).** No server-side components live in this repo. Egress is constrained to the destinations listed above. The same GPL/AGPL-blocking policy applies to both Rust (cargo-deny) and JS dependencies. This decision is the root that both the licensing gate and the egress list enforce.

**Settings surface.** The egress list lives under Settings → About and is populated from live app configuration (accounts, bridges, Beeper presence, update endpoint), rendered as UI. Settings themselves live in Rust (`keeper.db` behind `keeper-core::settings`), exposed via commands and a settings stream — there is no JS-writable settings store to read from.

**Plugin set.** The updater and autostart plugins are part of the adopted plugin set; these are desktop-only and are among the platform pieces explicitly out of scope for the future mobile path.

**Conventions.** English everywhere; bun only for JS tooling (never npm/pnpm/yarn); the quality-gate commands are the project's `check`, `check:rust`, and `test:rust` runners; tests run under cargo-nextest; logs carry ids only, never message content or tokens; secrets live only in the macOS Keychain (service `dev.tgorka.keeper`).

## Cross-Story Dependencies

- Story 11.2 depends on 11.1 (updater signing and the About/egress surface build on the release pipeline).
- Story 11.3 depends on 11.1, and on the full-text search work (Epic 5) and command palette (Epic 9) being in place so their latency gates have something real to measure — the FTS gate extends the search story's existing latency test to the 100k+ corpus.
- Story 11.1 is otherwise unblocked once Epic 1 produces a release-buildable app; feature epics need only be buildable, with final end-to-end validation deferred to release time.
