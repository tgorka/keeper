---
title: 'Story 1.1 — Cargo Workspace Split and Typed IPC Foundation'
type: 'refactor'
created: '2026-07-03'
status: 'done'
review_loop_iteration: 0
baseline_revision: '3ff61174af7bd873c1ebb6fb7385606f638d8ece'
final_revision: '51f98503dbb677c26f56acad1323c9064f48936b'
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** The Rust backend is a single `keeper` crate that mixes (future) Matrix/business logic with the Tauri shell, has no typed IPC contract, and no generated TypeScript bindings — so every later Epic-1 story would be built on the wrong seam and refactored onto the hexagon later.

**Approach:** Restructure `src-tauri/` into a cargo workspace with a tauri-free `keeper-core` hexagon crate and a thin `keeper` Tauri-shell crate, establish the AD-8 IPC conventions (IpcError envelope, `domain_verb` commands, snapshot-then-diff channels), and stand up an AD-7 ts-rs binding pipeline that emits camelCase TypeScript types to `src/lib/ipc/gen/` with a drift check — all demonstrated end-to-end by a single `app_ping` command and one demo snapshot-then-diff subscription, with all quality gates green.

## Boundaries & Constraints

**Always:**
- `keeper-core` carries **no `tauri` dependency** anywhere in its dependency tree; it depends only on platform-neutral crates and reaches the OS solely through the `Platform` port trait (AD-6, AD-24).
- Every type crossing IPC lives in `keeper-core` (a `vm`/ipc module), derives `serde` + `ts_rs::TS`, is `#[ts(export)]`, and uses `#[serde(rename_all = "camelCase")]`; timestamps are `i64` ms-since-epoch integers, never strings.
- Fallible commands are `domain_verb` snake_case and return `Result<T, IpcError>` where `IpcError = { code, message, accountId?, retriable }` and `code` is a stable string-serialized enum. `CoreError → IpcError` mapping happens **exactly once**, in the shell's command layer (AD-8, AD-21).
- Every subscription stream opens with a full snapshot/reset batch **before** any diff batch (AD-8).
- Rust: no `unsafe`, no `.unwrap()`/bare `.expect()` in production paths, no `todo!()`/`unimplemented!()` on any reachable path, `tracing` not `println!`. TS: no `any`, `import type` for types.
- `keeper.db`/accounts/keychain layout and Matrix logic are NOT introduced here — only the seams that later stories fill.

**Block If:**
- Adding any new dependency (e.g. `ts-rs`) would fail the cargo-deny license firewall (non-permissive/GPL/AGPL license) and no permissively-licensed equivalent exists.
- Satisfying the "no `tauri` in `keeper-core`" invariant would require dropping or downgrading a pinned stack-anchor version (matrix-sdk 0.18.0, tauri 2.11.x, ts-rs 12.x).

**Never:**
- No `zustand` stores, no Matrix protocol/crypto/DB code, no login/sync/timeline logic — those are stories 1.2+.
- No business logic in the `keeper` shell crate (IPC/plugin/protocol glue only).
- No hand-written types in `src/lib/ipc/gen/` (generated only); no Matrix JS SDK in the frontend.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Ping happy path | frontend calls typed `app_ping` wrapper | resolves to `PingVm` (camelCase fields, `ts` integer) | No error expected |
| Command failure | a command returns `Err(CoreError)` | shell maps once to `IpcError`; wrapper rejects with `{ code, message, accountId?, retriable }` | Rejected promise carries the envelope, not a raw string |
| Snapshot-then-diff | frontend opens the demo subscription | first delivered batch is the `Snapshot`/reset variant, subsequent are `Diff`; returns a subscription id | Handler must never emit a diff before the snapshot |
| Binding drift | committed `src/lib/ipc/gen/**` differs from freshly generated output | the bindings check fails (non-zero exit) | Fails CI; regenerate + commit to fix |
| core purity | `keeper-core` gains a transitive `tauri` dep | the core-purity check fails (non-zero exit) | Fails CI |

</intent-contract>

## Code Map

- `src-tauri/Cargo.toml` -- becomes the **virtual workspace** manifest (`[workspace]`, `resolver = "2"`, members `crates/keeper-core` + `crates/keeper`, shared `[workspace.dependencies]`). No `[package]`.
- `src-tauri/crates/keeper-core/` -- NEW hexagon crate: `vm` (IPC types incl. `PingVm`, `IpcError`, `IpcErrorCode`, `DemoBatch`), `error` (`CoreError` root), `platform` (`Platform` port trait), `demo` (pure `snapshot_then_diff()` batch producer). No `tauri` dep.
- `src-tauri/crates/keeper/` -- NEW shell crate: relocated `src/lib.rs`+`src/main.rs`, `build.rs`, `tauri.conf.json`, `capabilities/`, `icons/`, `gen/`. Owns `#[tauri::command] app_ping`, the demo subscribe command, the concrete `Platform` impl, and the single `CoreError → IpcError` mapping. Depends on `keeper-core`.
- `src-tauri/src/lib.rs`, `src-tauri/src/main.rs`, `src-tauri/build.rs`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/`, `src-tauri/icons/` -- MOVED into `crates/keeper/` (git-tracked move).
- `src-tauri/deny.toml`, `src-tauri/Cargo.lock` -- stay at workspace root (`deny.toml` unchanged; `cargo deny check` still runs from `src-tauri/`).
- `src/lib/ipc/gen/` -- NEW generated TS bindings (ts-rs output; committed).
- `src/lib/ipc/client.ts` -- NEW thin typed `invoke`/`Channel` wrappers (+ colocated `client.test.ts`).
- `package.json` -- add `ts-rs` binding, core-purity, and drift-check scripts; wire into `check`/`check:all`.
- `src/App.tsx` -- unchanged (does not call `greet`; safe to delete `greet`).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/Cargo.toml` -- convert to virtual workspace: `[workspace]` + `resolver = "2"` + members; hoist shared deps (serde, serde_json, tokio, tracing, thiserror, matrix-sdk*, ts-rs, tauri*) into `[workspace.dependencies]`; move `[lints]` to `[workspace.lints]` and have both crates `[lints] workspace = true` -- keeps one lint source of truth.
- [x] `src-tauri/crates/keeper/**` -- move the existing Tauri shell here (git `mv` lib.rs/main.rs/build.rs/tauri.conf.json/capabilities/icons); update `tauri.conf.json` `frontendDist` to reach project-root `dist` from the new depth and keep `beforeDevCommand`/`beforeBuildCommand` running at project root; keep lib name `keeper_lib`. Delete the `greet` command.
- [x] `src-tauri/crates/keeper-core/src/error.rs` -- define `CoreError` thiserror root (with at least one module-level enum rolling up) -- AD-21 root.
- [x] `src-tauri/crates/keeper-core/src/platform.rs` -- define the `Platform` port trait covering the dirs/keychain/notifier/sidecar ports (AD-24); the `dirs`/data-dir port is fully wired end-to-end; other ports are declared and return `CoreError::Unsupported` from the shell impl for now (honest, non-panicking) -- proves the platform-free seam.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- define `PingVm` (incl. an `i64` ms-epoch `ts`), `IpcError { code: IpcErrorCode, message, account_id: Option<String>, retriable }`, `IpcErrorCode` (stable string enum), and `DemoBatch` (snapshot/diff tagged enum); all derive `serde` + `ts_rs::TS`, `#[ts(export)]`, camelCase.
- [x] `src-tauri/crates/keeper-core/src/demo.rs` -- pure `snapshot_then_diff()` returning an ordered `Vec<DemoBatch>` (or stream) whose first element is the snapshot -- testable without Tauri.
- [x] `src-tauri/crates/keeper/src/ipc.rs` (or in lib.rs) -- `#[tauri::command] app_ping() -> Result<PingVm, IpcError>`; demo subscribe command taking `Channel<DemoBatch>`, returning a `Result<u64, IpcError>` subscription id, sending snapshot-first then diff; single `fn to_ipc_error(CoreError) -> IpcError` mapping; concrete `Platform` impl; register both commands in the builder.
- [x] `src-tauri/crates/keeper-core/tests` or `#[cfg(test)]` -- unit tests: (a) `snapshot_then_diff()`'s first batch is the snapshot variant; (b) `IpcErrorCode`/`IpcError` serde round-trips camelCase; ts-rs `#[ts(export)]` auto-generates the export test.
- [x] `src/lib/ipc/client.ts` -- typed `invoke<T>(cmd, args)` that awaits `@tauri-apps/api/core` `invoke` and rethrows the `IpcError` envelope, and a `subscribe(cmd, args, onBatch)` using `Channel`; import generated types with `import type`.
- [x] `src/lib/ipc/client.test.ts` -- colocated Vitest: mock `@tauri-apps/api/core`; assert `invoke` rejects with the envelope on error and that the subscribe wrapper forwards batches in order (snapshot before diff).
- [x] `package.json` -- add scripts: `bindings:check` (regenerate via `test:rust` then fail if `src/lib/ipc/gen` has a git diff), `check:core-tauri-free` (fail if `keeper-core`'s cargo tree contains `tauri`); fold both into `check`/`check:all`. Add `ts-rs` to `[workspace.dependencies]` (Rust) — no npm dep needed for generation.

**Acceptance Criteria:**
- Given the restructure is complete, when `bun run tauri dev` runs, then the app builds and launches, and `cargo build --manifest-path src-tauri/Cargo.toml` compiles both workspace members.
- Given the core-purity check, when it inspects `keeper-core`, then it reports **no** `tauri` in the dependency tree and exits zero (and fails if one is introduced).
- Given `keeper-core`, when inspected, then it exposes the `Platform` port trait and a `CoreError` root per AD-21/AD-24.
- Given `bun run test:rust` regenerates bindings, when `bindings:check` runs, then committed `src/lib/ipc/gen/**` matches generated output (fails on drift).
- Given `bun run check`, `bun run check:rust`, `bun run test:rust`, and `cargo deny check` (from `src-tauri/`), when each runs, then all pass.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 5: (high 0, medium 2, low 3)
- defer: 0
- reject: 4: (high 0, medium 0, low 4)
- addressed_findings:
  - `[medium]` `[patch]` `PingVm.ts` typed `ts` as `bigint` while Tauri IPC delivers a JS `number` (contract lie in the foundational timestamp binding) — annotated `vm::PingVm::ts` with `#[ts(type = "number")]` and regenerated bindings; `ts: number` now matches the wire.
  - `[medium]` `[patch]` `bindings:check` used `git diff --exit-code` (ignores untracked/new/deleted files), leaving the drift guard vacuous — switched to `test -z "$(git status --porcelain -- src/lib/ipc/gen)"` so add/delete/untracked drift now fails the gate.
  - `[low]` `[patch]` `check:core-tauri-free` swallowed `cargo tree` stderr (`2>/dev/null`), false-greening on a resolution failure — rewrote to capture the tree first (`tree=$(…) && ! … grep`), so a non-resolving tree fails closed (verified: EXIT 101 on a bogus package, EXIT 1 when tauri is present).
  - `[low]` `[patch]` `now_ms()` clamped a skewed clock silently — added `tracing::warn!` on both clamp branches per the project observability rule.
  - `[low]` `[patch]` documented that arming `channel.onmessage` before the id-returning `invoke` is load-bearing for future async streams (comment in `subscribe`).
- rejected (noise, dropped): `isIpcError` not validating the `code` enum (would break forward-compat with new Rust codes); demo partial-delivery on `channel.send` failure (over-engineering a synchronous demo); global `NEXT_SUBSCRIPTION_ID` not registry-backed (intended 1.1 seam); `{...args, channel}` shadowing a caller `channel` key (reserved arg).

## Design Notes

**Workspace layout risk (primary):** moving `tauri.conf.json` into `crates/keeper/` changes paths relative to it. `frontendDist` must point back to project-root `dist` (e.g. `../../../dist` from `src-tauri/crates/keeper/`); `icons/` and `capabilities/` move with the config so their relative paths stay stable. The `@tauri-apps/cli` (`bun run tauri`) auto-discovers the single `tauri.conf.json` by recursive search; if it does not, pass `--config crates/keeper/tauri.conf.json` in the `tauri` npm script. `beforeDevCommand: "bun run dev"` must still resolve at project root. Verify `bun run tauri dev` actually launches before declaring done.

**ts-rs export path:** `#[ts(export)]` writes relative to the crate dir; point output at project-root `src/lib/ipc/gen/` — e.g. `TS_RS_EXPORT_DIR` (via `src-tauri/.cargo/config.toml [env]` or the test) or per-type `#[ts(export_to = "…/src/lib/ipc/gen/")]`. The `export_bindings_*` tests run under `cargo nextest`, so `bun run test:rust` regenerates; `bindings:check` = run it then `git diff --exit-code -- src/lib/ipc/gen`.

**Snapshot-then-diff seam:** keep the ordering invariant in `keeper-core::demo` (pure, unit-tested) so it is verifiable without constructing a Tauri `Channel`; the shell command just forwards the produced batches over the `Channel<DemoBatch>` in order. This is the reusable pattern every later stream story copies.

**Matrix deps:** home `matrix-sdk`/`matrix-sdk-ui` in `keeper-core`'s manifest (its architectural owner) even though 1.1 does not call them yet — avoids churn in 1.3; they must not pull `tauri`.

**Implementation note — cargo-deny (pre-existing, out of scope):** `cargo deny check licenses`/`bans`/`sources` all pass (license firewall green, incl. `ts-rs` 12). Full `cargo deny check` exits non-zero only on `advisories` → RUSTSEC-2025-0098 (unmaintained `unic-ucd*` via `tauri → tauri-utils → urlpattern`). This is **pre-existing** (present in the baseline HEAD `Cargo.lock`, not introduced by ts-rs) and `deny.toml` is pinned unchanged by this spec, so it is left untouched. Follow-up (deferred, not this story): add `[advisories] ignore = ["RUSTSEC-2025-0098"]` if a fully-green `cargo deny check` becomes a release gate.

## Verification

**Commands:**
- `cargo build --manifest-path src-tauri/Cargo.toml` -- expected: both members compile.
- `bun run check` -- expected: biome + tsc + vitest (incl. `client.test.ts`) pass.
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` clean across the workspace.
- `bun run test:rust` -- expected: nextest green; regenerates `src/lib/ipc/gen/**`.
- `bun run bindings:check` -- expected: zero git diff under `src/lib/ipc/gen/` after regeneration.
- `bun run check:core-tauri-free` -- expected: no `tauri` in `keeper-core`'s tree.
- `cd src-tauri && cargo deny check` -- expected: license firewall passes (including `ts-rs`).

**Manual checks:**
- `bun run tauri dev` launches the window; from devtools, the typed `app_ping` wrapper resolves a `PingVm` and the demo subscription's first batch is the snapshot.

## Auto Run Result

Status: **done**

### Summary
Restructured `src-tauri/` from a single `keeper` crate into a cargo virtual workspace: `crates/keeper-core` (tauri-free hexagon) + `crates/keeper` (Tauri shell). Established the AD-8 IPC conventions (single `IpcError` envelope, `domain_verb` commands, snapshot-then-diff `Channel` batches), the AD-7 ts-rs binding pipeline emitting camelCase TypeScript to `src/lib/ipc/gen/` with a drift check, the AD-21 `CoreError` root mapped once to `IpcError` in the shell, and the AD-24 `Platform` port trait (data-dir wired; keychain/notifier/sidecar honestly `Unsupported`). Demonstrated end-to-end by `app_ping` and a demo snapshot-then-diff subscription, plus thin typed `invoke`/`Channel` wrappers with tests. The old `greet` command was removed (frontend never referenced it).

### Files changed
- `src-tauri/Cargo.toml` — single crate → virtual workspace (`[workspace]`, `resolver = "2"`, `[workspace.package]`, `[workspace.dependencies]`, shared `[workspace.lints]`); added `ts-rs = "12"`, `dirs = "6"`.
- `src-tauri/crates/keeper-core/` — new hexagon crate: `vm.rs` (`PingVm`, `IpcError`, `IpcErrorCode`, `DemoItem`, `DemoBatch`), `error.rs` (`CoreError`/`PlatformError`), `platform.rs` (`Platform` port), `demo.rs` (pure `snapshot_then_diff`), unit tests.
- `src-tauri/crates/keeper/` — relocated Tauri shell (`lib.rs`, `main.rs`, `build.rs`, `tauri.conf.json`, `capabilities/`, `icons/`); `ipc.rs` (`app_ping`, `demo_subscribe`, `to_ipc_error`, `DesktopPlatform`, `AppState`).
- `.cargo/config.toml` — `TS_RS_EXPORT_DIR` → `src/lib/ipc/gen`.
- `src/lib/ipc/client.ts` + `client.test.ts` — thin typed `invoke`/`subscribe` wrappers and tests.
- `src/lib/ipc/gen/*.ts` — generated bindings (committed).
- `package.json` — `bindings:check`, `check:core-tauri-free`, folded into `check`/`check:all`; `tauri` script points at the relocated config.
- `biome.json`, `src-tauri/.gitignore` — ignore generated bindings / relocated `gen/schemas`.

### Review findings
- Two reviewers (adversarial-general + edge-case-hunter). 5 patches applied (2 medium, 3 low), 0 deferred, 4 rejected, 0 intent-gap, 0 bad-spec. See Review Triage Log.
- Patches: `PingVm.ts` `bigint`→`number` timestamp binding; `bindings:check` now catches untracked/new/deleted drift (`git status --porcelain`); `check:core-tauri-free` fails closed on a resolution error; `now_ms()` warns via `tracing` on clock clamp; documented the load-bearing `onmessage`-before-`invoke` ordering.

### Verification
- `cargo build` (workspace) ✅ · `bun run check:rust` (fmt + clippy `-D warnings`) ✅ · `bun run test:rust` (17 tests, incl. 5 `export_bindings_*`) ✅ · `bun run check` (biome + tsc + vitest 5 + core-tauri-free) ✅ · `bun run bindings:check` ✅ (post-commit) · `bun run check:core-tauri-free` ✅ (detection EXIT 1, fail-closed EXIT 101 verified) · `cargo deny check licenses/bans/sources` ✅.
- Not run: `bun run tauri dev` (blocking GUI launch — compile verified via `cargo build`).

### Residual risks
- **Pre-existing, out of scope:** full `cargo deny check` still fails `advisories` on RUSTSEC-2025-0098 (unmaintained `unic-ucd*` pulled transitively by `tauri`). Present in the baseline `Cargo.lock`, not introduced here; `deny.toml` left unchanged per spec. Follow-up if a fully-green `cargo deny check` becomes a release gate: add `[advisories] ignore = ["RUSTSEC-2025-0098"]`.
- The `Platform` keychain/notifier/sidecar ports return `Unsupported` until their consuming stories; the demo subscription delivers synchronously (real async streams land in 1.4+).
