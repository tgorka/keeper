---
title: 'bbctl Integration for Beeper Self-Hosted Bridges'
type: 'feature'
created: '2026-07-05'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: [oversized]
baseline_revision: 32dc7b06ce29e1e55c0ae06bcb0007e0b073e632
final_revision: 6e4de6460fac94b4d5d9bd945397da0c4a108c21
---

<intent-contract>

## Intent

**Problem:** A Beeper Account sees Matrix-native chats + Beeper Cloud bridges, but a user who wants network parity for a bridge Beeper doesn't host must drop to a terminal and run `bbctl` by hand. keeper has no "run your own bridge" surface: no way to register/run a self-hosted bridge, no progress feedback, no honest fallback when the tool is absent.

**Approach:** Add a Beeper-accounts-only **"Run your own bridge"** surface driven by the `bbctl` CLI as a launch-on-demand sidecar. Wire the long-stubbed `Platform::sidecar_path` port; a `BbctlRunner` port (static-dispatch generic, mirroring `BridgeTransport`) spawns the resolved binary in the shell and streams stdout lines, which a **pure** parser projects into a **log-free** progress stepper (`checking → registering → starting → running → success/failure`). Capability is honest and data-driven: a versioned embedded `bbctl.json` carries the self-hostable networks and the guided-install steps; when the binary can't be resolved the section renders those install instructions and **everything else in keeper keeps working**. On success the existing discovery (6.2) + health (6.5) machinery surfaces the new bridge in the list with status. Supervision/auto-restart and a log viewer are explicitly out of scope (v1.x).

## Boundaries & Constraints

**Always:**
- **Beeper accounts only.** The surface renders only for accounts with `provider == Provider::Beeper`; the core `bbctl_run_start` re-checks and returns an honest `BridgeError::Bbctl` for a non-Beeper account (defense in depth — never trust the frontend gate).
- **Wire `Platform::sidecar_path` for real.** Replace the `Unsupported` stub in `DesktopPlatform`; resolve the per-arch sidecar next to the running executable (`std::env::current_exe()` parent + the target-triple suffix Tauri uses, e.g. `bbctl-aarch64-apple-darwin`). Return `CoreError::Unsupported` (honest, non-panicking) when the binary isn't present — that absence **is** the guided-install path (AC-2).
- **Pure core, impure shell (6.2–6.6 discipline).** The `bbctl.json` loader/validator, the network/support lookup, `bbctl_args(action, name)`, and `parse_bbctl_event(line) -> Option<BbctlPhase>` are pure and unit-tested. The live `current_exe` resolution + `tokio::process` spawn/stream is the documented residual risk (as with 6.3's HTTP shell).
- **Log-free stepper.** The UI shows recognized *phase* transitions only. Unrecognized `bbctl` stdout lines are dropped, never dumped into the UI (no log viewer — v1.x).
- **Data-driven capability.** A versioned embedded `bbctl.json` (`version`, `install: { steps[], docsUrl }`, `networks: [{ networkId, bbctlName, supported }]`) — loaded/validated/cached exactly like `bot-commands.json`/`resolve-support.json`. A network absent from the supported set is not offered.
- **Rust owns process + auth; the frontend renders VMs.** Streaming rides `Channel<BbctlProgressVm>` (mirror `bridge_login_start`). No Beeper token/credential ever appears in a VM, error string, or log.
- **The bridge appears via existing machinery.** After a success phase the panel refreshes the existing per-account bridge discovery so the new bridge card shows with status — no new list/status path is invented.

**Block If:**
- The `Platform::sidecar_path` port has been removed or its signature changed away from `fn sidecar_path(&self, name: &str) -> Result<PathBuf, CoreError>` — HALT rather than re-architect the platform hexagon.
- No `Channel<T>`-based streaming command pattern exists to mirror (it does: `bridge_login_start`) — HALT rather than invent a second streaming mechanism.

**Never:**
- Adding a `bundle.externalBin` entry to `tauri.conf.json` that points at a binary not present in the repo — it breaks `tauri build --no-bundle` in CI. Bundling the real per-arch `bbctl` binary + its manifest entry is a packaging follow-up; the code resolves and degrades honestly without it.
- Adding `tauri-plugin-shell` / a new runtime dependency or a new capability permission — spawn via `tokio::process` on the resolved path (no cargo-deny or capability churn).
- Auto-restart supervision, a persisted/cross-restart child handle, or a raw-log viewer (all v1.x, out of scope).
- Rendering the panel for non-Beeper accounts; a new bridge-login flow or any change to `drive_login`/`BridgeLoginVm`/the login stepper; routing large payloads through IPC.

## I/O & Edge-Case Matrix

Pure `bbctl_doc()` / `networks()` / `support_for(network_id)` (data), pure `bbctl_args(action, bbctl_name)`, and the pure projection `parse_bbctl_event(line) -> Option<BbctlPhase>` (log-free marker → phase). The live sidecar spawn/stream is the documented residual risk (6.3 discipline).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| bbctl available, register→run | Beeper account, `sidecar_path` resolves, supported network | stream `checking → registering → starting → running → success`; panel refreshes discovery so the bridge card appears with status | live exec = residual risk |
| bbctl absent | `sidecar_path` → `Unsupported` | `available: false` + `install` steps; section shows guided install; rest of keeper unaffected | `CoreError::Unsupported` (honest, not a crash) |
| Register/run invoked while absent | `bbctl_run_start`, `sidecar_path` → err | terminal `failure` phase carrying the honest "bbctl not found — install it" message | `BridgeError::Bbctl` |
| Non-Beeper account | account `provider != Beeper` | panel not rendered; `bbctl_run_start` returns honest "Running your own bridge is available for Beeper accounts only" | `BridgeError::Bbctl` |
| Unsupported network | `support_for` → `supported: false` / absent | not offered in the network picker | pure gate, no I/O |
| bbctl exits non-zero / unparseable | run stream, error marker or non-zero exit | terminal `failure` phase with the captured (capped, non-secret) message; Retry offered, selection retained | `BridgeError::Bbctl` |
| register OK, run fails | sequence, `run` errors after `register` succeeds | `failure` at the `starting`/`running` boundary; no fake success | `BridgeError::Bbctl` |
| Recognized "started" marker | run stdout line matches the started marker | emit `success`; stop consuming; leave child running unsupervised (v1.x) | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/data/bbctl.json` -- **NEW**. Versioned `{ version, install: { steps: [..], docsUrl }, networks: [{ networkId, bbctlName, supported }] }`. Embedded via `include_str!`. Mark only genuinely self-hostable mautrix networks `supported: true` (e.g. signal, whatsapp, telegram); give real install steps + the Beeper self-host docs URL.
- `src-tauri/crates/keeper-core/src/bridges/data.rs` -- add `BbctlDoc`/`BbctlNetwork` structs + version const + `OnceLock` cache + `bbctl_doc()` loader + `support_for`/`networks` accessors + `validate_bbctl` (version, non-empty ids, no duplicates), mirroring `resolve_support()`/`bot_commands()`; add a load/validate test.
- `src-tauri/crates/keeper-core/src/bridges/bbctl.rs` -- **NEW**. `BbctlRunner` port trait: `fn is_available(&self) -> bool` + native `async fn run(...) -> impl Future<...> + Send` (static dispatch — **no** `async-trait`, no `dyn`, follow `BridgeTransport`). **Streaming contract (load-bearing — the review found buffer-then-classify-after-exit is structurally broken for a never-exiting `run` daemon):** `run(args, on_line)` where `on_line: Box<dyn FnMut(&str) -> LineControl + Send>` returns a control value (`Continue`/`Stop`). The runner streams each line to `on_line` **as it arrives**; when `on_line` returns `Stop` the runner **stops reading and resolves promptly WITHOUT waiting for process exit and WITHOUT killing the child** (launch-and-leave). Resolve type distinguishes early-stop from natural exit, e.g. `Result<BbctlRunExit, BridgeError>` with `BbctlRunExit::{ Exited(i32), StoppedEarly }`. Pure `bbctl_args(action, bbctl_name) -> Vec<String>` -- **do NOT pass `--json`** (the parser matches human prose; `--json` made the args and parser mutually inconsistent). Pure `parse_bbctl_event(line) -> Option<BbctlPhase>` (recognized prose markers only; else `None`). `availability(runner) -> BbctlAvailabilityVm`. Orchestrator `run_self_hosted<R: BbctlRunner>(runner, network, sink)`: emit `checking`; absent → honest guided-install failure; `register` then `run`, **sinking each recognized non-terminal phase incrementally as it arrives (not buffered/flushed after exit)**; on the `run` started marker sink `success` and return `Stop` (leave child running); on a recognized error marker sink the capped-verbatim terminal `failure` and `Stop`; non-zero natural exit with no started marker → honest failure. **Unit-test the pure helpers + the I/O matrix** with a `FakeBbctlRunner` that honors early-`Stop` (assert the started-marker path resolves `StoppedEarly` and emits `success` even when the fake would otherwise keep streaming).
- `src-tauri/crates/keeper-core/src/bridges/mod.rs` -- `pub mod bbctl;` + re-exports.
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `BbctlAvailabilityVm { available, install: BbctlInstallVm, networks: Vec<BbctlNetworkVm> }`, `BbctlInstallVm { steps, docs_url }`, `BbctlNetworkVm { network_id, name, bbctl_name }`, `BbctlProgressVm { network_id, phase: BbctlPhase, message: Option<String>, error: Option<String> }`, and enum `BbctlPhase { Checking, Registering, Starting, Running, Success, Failure }` (serde camelCase + `#[ts(export)]`); round-trip + no-token-leak tests.
- `src-tauri/crates/keeper-core/src/error.rs` -- add `BridgeError::Bbctl(String)` (honest bbctl/process/gate failure).
- `src-tauri/crates/keeper-core/src/account.rs` -- `bbctl_availability<R: BbctlRunner>(runner) -> BbctlAvailabilityVm` (pure map over data + `runner.is_available()`) and `bbctl_run_start<R: BbctlRunner>(account_id, network_id, runner, sink) -> Result<(), CoreError>` (gate `provider == Beeper` else honest `BridgeError::Bbctl`; support-gate the network; drive `run_self_hosted`). Reuse the existing account lookup used by `resolve_bridge_identifier`/`start_bridge_login`.
- `src-tauri/crates/keeper/src/ipc.rs` -- implement `DesktopPlatform::sidecar_path` (`current_exe` parent + target-triple; `Unsupported` when absent). Add `DesktopBbctlRunner` (`is_available` = `sidecar_path` resolves; `run` = `tokio::process::Command` on the path). **The runner MUST: (a) pipe AND read BOTH stdout and stderr, merging their lines through `on_line` (bbctl logs progress/markers to stderr); (b) honor an `on_line` `Stop` by ending the read promptly and returning `StoppedEarly` — do NOT `child.wait()` and do NOT kill the child (it keeps running); (c) NOT treat a line-read error (non-UTF-8) as clean EOF — skip the bad line and keep reading.** Commands `bbctl_availability(...)`, `bbctl_run_start(..., channel: Channel<BbctlProgressVm>) -> u64`, `bbctl_run_cancel(...)`; wrap `channel.send` in the sink (mirror `bridge_login_start`); add the `BridgeError::Bbctl` arm to `to_ipc_error` (→ `SyncUnavailable`, retriable). **`bbctl_run_start` MUST register the run session in the runs registry BEFORE spawning the driver task (insert-then-spawn) so a fast-terminating task cannot leave a resident handle; and MUST dedupe an already-in-flight run for the same (account, network) — replace/reject rather than spawn a second unsupervised `bbctl run` daemon.**
- `src-tauri/crates/keeper/src/lib.rs` -- register the three commands in `invoke_handler`.
- `src/lib/ipc/client.ts` -- `bbctlAvailability()`, `bbctlRunStart(accountId, networkId, onState)` (via `subscribe`), `bbctlRunCancel(sessionId)` wrappers (+ import generated VMs from `./gen/`).
- `src/lib/stores/bbctl.ts` -- **NEW** zustand vanilla store (mirror `new-chat.ts`): sheet `isOpen`, `selectedNetworkId`, `accountId`, `open()/close()`.
- `src/lib/bridges.ts` -- add `BBCTL_PHASE_LABEL: Record<BbctlPhase, string>` (log-free step words).
- `src/hooks/use-bbctl-run.ts` -- **NEW** streaming register/run hook (mirror `use-bridge-login.ts`: `start`, subscribe, synthetic failure VM on reject, cancel-on-unmount). **`cancel()` MUST flip the per-run `cancelled` ref so a late-resolving `start().then` cannot re-register a session the user already cancelled.**
- `src/components/bridges/bbctl-panel.tsx` (+ test) -- **NEW** Beeper-only "Run your own bridge" section: loading → available (network `Select` + Run) or unavailable (guided-install steps + docs link); opens the run sheet; refreshes discovery on success. **Key the install-steps list by index (steps may repeat). If the store is open for this account but the selected network is absent from `availability.networks`, close the store rather than leave a stuck-open state with no sheet.**
- `src/components/bridges/bbctl-run-sheet.tsx` (+ test) -- **NEW** `Sheet` progress stepper rendering `BbctlProgressVm.phase` (checking/registering/starting/running/success/failure + Retry), reusing the login-sheet visual pattern. **Fire the success side-effect (onSuccess discovery-refresh + auto-close) AT MOST ONCE per success via a ref, and guard `onSuccess()` so a throwing refresh cannot strand the sheet open.**
- `src/components/layout/bridges-pane.tsx` -- mount `<BbctlPanel accountId={...} />` once per Beeper account (filter `accounts` by `provider === "beeper"`).

## Tasks & Acceptance

**Execution:**
- [x] `data/bbctl.json` + `data.rs` -- versioned `install` + `networks[{networkId,bbctlName,supported}]`; cached `bbctl_doc()` + `support_for`/`networks`; validator; load test.
- [x] `bbctl.rs` -- `BbctlRunner` port (static-dispatch async, no `async-trait`); pure `bbctl_args` + `parse_bbctl_event`; `run_self_hosted` orchestrator; **unit-test the I/O matrix** with a `FakeBbctlRunner` (available happy path, absent, non-zero exit, register-ok/run-fail, started-marker→success, unrecognized-line dropped).
- [x] `mod.rs` -- expose `bbctl`.
- [x] `vm.rs` -- the four VMs + `BbctlPhase` (camelCase + ts-rs) + round-trip/no-token-leak tests.
- [x] `error.rs` -- `BridgeError::Bbctl`.
- [x] `account.rs` -- `bbctl_availability` + `bbctl_run_start` (Beeper gate, network gate). _(Impl split: core method runs the Beeper+network gate and returns the resolved network; the IPC command spawns `run_self_hosted` over `DesktopBbctlRunner` so the streaming task is `'static` and cancelable — gate still runs before any spawn; takes `&dyn Platform` to read the durable non-secret `provider`.)_
- [x] `ipc.rs` + `lib.rs` -- `DesktopPlatform::sidecar_path` impl; `DesktopBbctlRunner`; three `#[tauri::command]`s + `to_ipc_error` arm + registration.
- [x] `client.ts` -- `bbctlAvailability` / `bbctlRunStart` / `bbctlRunCancel` wrappers.
- [x] `bbctl.ts` store + `bridges.ts` phase labels + `use-bbctl-run.ts` hook.
- [x] `bbctl-panel.tsx` (+ test) -- Beeper-gated section, available/unavailable branches, opens sheet, refresh-on-success.
- [x] `bbctl-run-sheet.tsx` (+ test) -- phase stepper + Retry.
- [x] `bridges-pane.tsx` -- mount the panel for Beeper accounts only.

**Acceptance Criteria:**
- Given a connected Beeper Account with bbctl available, when the user picks a Network in "Run your own bridge" and starts, then keeper drives `bbctl` register/run as a launch-on-demand sidecar with a log-free progress stepper, and on success the resulting Bridge appears in the Bridge list with status — without leaving keeper (FR-29). _(Live end-to-end exec is the documented residual risk; the seams — gate, args, marker parsing, VM projection — are unit-tested.)_
- Given bbctl is absent, when the section renders, then it offers guided install instructions and everything else in keeper functions fully without it (FR-29).
- Given a non-Beeper account, then the "Run your own bridge" section does not render and `bbctl_run_start` returns an honest Beeper-only error rather than launching anything.
- Given sidecar lifecycle, then scope is launch-on-demand + status surfacing only — no auto-restart policy and no log viewer (v1.x).
- Given `bun run check:all`, then Biome + tsc + vitest + rustfmt + clippy (`-D warnings`, no `.unwrap()`, no `async_fn_in_trait`) + cargo-nextest all pass.

## Spec Change Log

### 2026-07-05 — bad_spec loopback (review iteration 1)

- **Triggering findings:** F1 (high) — the `run` daemon never exits, so the runner's `await`-to-completion + classify-after-exit shape means `success` is never emitted and the stepper freezes at "Starting"; AC-1's primary path is dead in production (fake runner masked it). F2 (medium) — no live progress: phases were flushed in a post-exit burst. F3 (high) — `bbctl_args` passed `--json` while `parse_bbctl_event` substring-matched prose, so the two disagreed (false failures on JSON field text; dropped structured stages). F4 (high) — `stderr` was routed to `/dev/null`, so a Go CLI that logs to stderr yields nothing to the parser.
- **Amended (outside `<intent-contract>`):** Code Map `bbctl.rs` — added the explicit streaming contract (`on_line -> Continue/Stop`, incremental sink, early-`Stop` on started/error marker, `BbctlRunExit::{Exited,StoppedEarly}`, drop `--json`). Code Map `ipc.rs` — runner must read **both** stdout+stderr, honor early-`Stop` without `wait()`/kill, not treat read-errors as EOF; `bbctl_run_start` insert-before-spawn + dedupe in-flight runs. Code Map hook/sheet/panel — cancel-ref race, success fire-once + guarded refresh, sheet-keying close, index-keyed steps. Design Notes — the incremental/early-stop rationale + stdout/stderr + args↔parser coherence.
- **Known-bad state avoided:** a green-tested feature whose real primary flow (`register → run → success → bridge appears`) can never complete because the persistent `bbctl run` process is awaited to an EOF that never comes.
- **KEEP (verified good in iteration 1 — must survive re-derivation):** the pure data layer (`bbctl.json` schema + `bbctl_doc()` loader/validator/`support_for`/`networks` in `data.rs` + tests); the five VMs (`Bbctl*Vm` + `BbctlPhase`, camelCase, ts-rs, no-token-leak) + round-trip tests; the Beeper+network gate in `account.rs::bbctl_run_start` reading the durable non-secret `provider` via `&dyn Platform` and returning the resolved network (IPC owns the spawn); `DesktopPlatform::sidecar_path` via `current_exe()` parent + `target_triple()` with honest `Unsupported`; **no** new deps / `tauri-plugin-shell` / capability / `externalBin` (spawn via `tokio::process`); the `parse_bbctl_event` prose-marker set + `cap_message` capping; the Beeper-only panel gating with available (picker) vs unavailable (guided-install) branches; the hook's `use-bridge-login` cancellation discipline; the `availability()` guided-install projection + absent-runner honest failure.

## Review Triage Log

### 2026-07-05 — Review pass (iteration 1)
- intent_gap: 0
- bad_spec: 4: (high 3, medium 1)
- patch: 0
- defer: 0
- reject: 0
- addressed_findings:
  - `[high]` `[bad_spec]` `bbctl.rs`/`ipc.rs` — F1: the persistent `bbctl run` daemon never EOFs, so the runner's await-to-completion + classify-after-exit shape means `success` is never emitted (stepper frozen at "Starting"); AC-1 primary path dead. Amended the streaming contract to incremental sink + `on_line` `Continue/Stop` early-stop leaving the child alive.
  - `[high]` `[bad_spec]` `ipc.rs` — F4: `stderr` discarded (`Stdio::null()`); a Go CLI logging to stderr yields nothing to the parser. Amended: runner reads/merges both stdout+stderr.
  - `[high]` `[bad_spec]` `bbctl.rs` — F3: `--json` args vs prose-substring parser were mutually inconsistent (false failures / dropped stages). Amended: drop `--json`, match prose, mark markers provisional (residual risk).
  - `[medium]` `[bad_spec]` `bbctl.rs` — F2: no live progress (post-exit burst). Amended: sink each recognized phase as it arrives.
  - _Folded into the amendment as re-derivation constraints (moot as standalone patches under the bad_spec re-derive): read-error-not-EOF (F7/Edge-F2), registry insert-before-spawn (Edge-F3), in-flight-run dedupe (F5), cancel-ref race (Edge-F6), success fire-once + guarded refresh (F9/Edge-F5), sheet-keying close (Edge-F7), index-keyed install steps (F8), out-of-order-after-failure eliminated by incremental sink + early-stop (F10/Edge-F4), first-`checking` survivability (F6, hook already defaults to a checking render)._

### 2026-07-05 — Review pass (iteration 2, post re-derivation)
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 3, low 0)
- defer: 2
- reject: 16
- addressed_findings:
  - `[medium]` `[patch]` `bbctl.rs`/`bbctl-run-sheet.tsx` — the raw `bbctl` line was carried in the non-terminal `BbctlProgressVm.message` (register/started/run phases) and rendered verbatim by `ProgressPanel`, contradicting the stated log-free invariant ("recognized phase transitions only; no raw line crosses IPC or reaches the UI") and risking surfacing a sensitive substring. Fixed: orchestrator now sinks recognized phases with `message: None`; `ProgressPanel` renders the phase LABEL only. No raw `bbctl` output crosses IPC or reaches the DOM. (`bbctl.rs:228,285,294`, `bbctl-run-sheet.tsx:132`)
  - `[medium]` `[patch]` `ipc.rs` — `DesktopBbctlRunner::run` spawned the stdout/stderr reader tasks as detached top-level tasks; a `bbctl_run_cancel` aborts only the driver task, so on cancel the two readers detached and (for a quiet daemon) blocked forever on `read_until`, leaking two tasks + pipe fds per cancel — defeating "cancel tears down keeper's streaming task". Fixed: readers are wrapped in an `AbortOnDrop` guard so dropping the `run` future (early-stop OR driver-cancel) aborts them; the launched daemon is left untouched (launch-and-leave).
  - `[medium]` `[patch]` `bbctl-run-sheet.test.tsx` — the "cancels the run when closed" test asserted only that `start` was called, never triggering a close nor asserting `cancel` (vacuous coverage over the leak-prone cancel path). Fixed: the test now clicks the Sheet close affordance and asserts `hookState.cancel` fires + `onOpenChange(false)`; also tightened the non-terminal-phase test to assert the log-free label (and that the raw line is absent).

### 2026-07-05 — Review pass (iteration 3, independent follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 1, low 0)
- defer: 0
- reject: 20
- addressed_findings:
  - `[medium]` `[patch]` `ipc.rs` — `BbctlRunRegistry` registered the driver-task abort handle *after* `tokio::spawn` (reserve-then-spawn-then-insert), so the Code Map's mandated "insert-then-spawn, no resident handle" and in-flight dedupe were both violated: (a) on the common fast path (absent sidecar → immediate failure) the driver could run `finish()` before `insert()`, leaking a stale abort handle in `tasks` forever; (b) a racing second `bbctl_run_start` for the same `(account, network)` could not abort the first (its handle was not yet in `tasks`), spawning two unsupervised `bbctl run` daemons — defeating the dedupe invariant. Fixed by collapsing the two maps under a single `Mutex<BbctlRunInner>` and adding `BbctlRunRegistry::start(...)` that reserves the target, aborts any prior run for it, spawns, and inserts the new handle **atomically under one lock**. `finish`/`cancel` now take the same single lock. All 602 rust + 665 vitest tests still green; clippy `-D warnings` clean. (`ipc.rs:69-149,804-828`)
  - _Rejected (20): prose-marker substring fragility — `is_error_marker`/`is_started_marker`/`parse_bbctl_event` false positives, started-vs-error precedence, comment-vs-order mismatch, premature `connected to homeserver` success (all the spec's **documented residual risk**, tunable only against a real binary); unbounded `read_until` (trusted sidecar); non-executable/symlink sidecar reports available (honest run-time failure); `bbctl_name` shell-like content (trusted validated embedded data, args not shell-interpolated); `is_available`/`run` TOCTOU double-probe (honest degradation); NULL-provider legacy Beeper account (provider set at creation; unconfirmed reachable); begin/finish replacement race (guard `idx.get==Some(session)` already correct); frontend superseded-snapshot / success double-fire / StrictMode double-start / selected-network-shrink / availability-effect-deps (all already guarded per spec's KEEP list); `cap_message` no redaction on the terminal error (spec mandates the capped-verbatim failure; keeper passes no token to bbctl); Windows `set_extension` (target triples carry no dot); missing `DesktopBbctlRunner`/registry unit coverage & `register_failure` test-panic style (the live shell is documented residual risk); `Exited(-1)` signal-termination wording (cosmetic); unsupported-network dead rows (harmless validation fixture)._

## Design Notes

- **This is the "later story" the platform hexagon was waiting for.** `Platform::sidecar_path` and its `DesktopPlatform` impl already exist as an honest `Unsupported` stub (`platform.rs:49`, `ipc.rs:198`); tests already mock it. This story fills it in — resolving next to `current_exe()` (where Tauri lays sidecars) rather than needing an `AppHandle` (`DesktopPlatform` is a unit struct).
- **Runner port mirrors `BridgeTransport`, not a new pattern.** Native `async fn` returning `impl Future + Send`, dispatched statically via `run_self_hosted<R: BbctlRunner>` — no `async-trait`, no trait object, so no `async_fn_in_trait` clippy warning. The shell provides `DesktopBbctlRunner` (tokio::process); tests provide `FakeBbctlRunner`. Exactly how `drive_login<T: BridgeTransport>` is tested against provisioning/bot fakes.
- **"bbctl absent" and "bbctl present" are one honest gate.** Availability is just whether `sidecar_path` resolves. In dev/CI (no bundled binary) it resolves to absent → the guided-install branch (AC-2), which is the fully-testable path. The present path's live register/run needs both a real `bbctl` binary and a real Beeper bridge to register against — unattended-infeasible, so it is residual risk, exactly as 6.3's live provisioning HTTP round-trip. We do **not** ship a fake binary or a dangling `externalBin` (which would break `tauri build`).
- **Log-free by construction.** `parse_bbctl_event` returns `Some(phase)` only for recognized markers and `None` otherwise; the orchestrator only ever sinks a `BbctlProgressVm` on a `Some`. There is no path from a raw `bbctl` log line to the UI — satisfying "log viewer is out of scope" structurally, not by omission.
- **`run` is launch-and-leave — and the streaming MUST be incremental with early-stop, NOT buffer-then-classify.** `register` completes; `run` starts a **persistent bridge daemon that never exits (no stdout EOF)**. Therefore the runner cannot `await` the process to completion before emitting phases: the review (iteration 1) found exactly that shape — `on_line` buffered phases and the orchestrator classified only after `runner.run().await` resolved, which for `run` never happens → the stepper freezes at "Starting", `success` is unreachable, AC-1's happy path is dead (green tests only because the fake runner returned synchronously). The corrected contract: `on_line` returns a `Continue`/`Stop` control; the orchestrator sinks each recognized non-terminal phase **as it arrives** and returns `Stop` on the started marker (sink `success`) or an error marker (sink `failure`); the runner then resolves promptly (`StoppedEarly`) leaving the child alive and unsupervised (no restart policy / log viewer — v1.x). The bridge then surfaces through the existing discovery (6.2) + health (6.5) machinery — the panel just refreshes discovery on success. Persisting/supervising the child across app restarts is out of scope.
- **Read stdout AND stderr; keep args and the parser consistent.** bbctl is a Go CLI whose progress/log lines commonly land on **stderr** — the runner must pipe and project both streams, or a real run emits nothing the parser sees. The parser matches **human-readable prose** substrings, so `bbctl_args` must **not** request `--json` (the two disagreed: `--json` output would spuriously match "error"/"failed" and drop structured stages). The prose marker set is provisional and tunable against a real binary — that tuning is the documented residual risk, but the *structure* (incremental sink, early-stop, both streams, args↔parser agreement) is not residual risk and is unit-testable with a `FakeBbctlRunner`.

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` (no `.unwrap()`, no `async_fn_in_trait`).
- `bun run test:rust` -- expected: cargo-nextest green incl. `bbctl_doc()` load/validate, `bbctl_args`/`parse_bbctl_event` I/O matrix (with `FakeBbctlRunner`), and the `Bbctl*Vm`/`BbctlPhase` round-trips (assert no token leak).
- `bun run check` -- expected: Biome + tsc + vitest pass incl. `bbctl-panel` (Beeper-gated; available network-picker vs. unavailable install-instructions branch) and `bbctl-run-sheet` (phase stepper → success/failure + Retry).

**Manual checks (if no CLI):**
- Live register/run against a real `bbctl` + real Beeper bridge cannot be exercised unattended: the sidecar spawn/stream shell (`current_exe` resolution, `tokio::process`, real bbctl auth handshake, real portal room) is the documented residual risk — covered by the pure `bbctl_args`/`parse_bbctl_event`/`support_for` unit tests and the frontend panel/sheet tests, as with 6.3's provisioning shell.

## Auto Run Result

Status: done

### Summary

Story 6.7 (bbctl integration for Beeper self-hosted bridges, FR-29) was **re-derived from the amended spec** (the prior run reverted its code in the iteration-1 bad_spec loopback but never completed the re-implementation — the tree was at baseline with only the spec present). The full feature was implemented honoring the corrected streaming contract (incremental sink, `on_line → Continue/Stop` early-stop leaving the child alive, both stdout+stderr, no `--json`, read-errors ≠ EOF), then reviewed (Blind Hunter + Edge Case Hunter). Three medium patches were applied; two real robustness items were deferred; the remainder were documented residual risk or already-guarded and rejected.

### Files changed

**New**
- `src-tauri/crates/keeper-core/data/bbctl.json` — versioned install steps + docsUrl + self-hostable networks.
- `src-tauri/crates/keeper-core/src/bridges/bbctl.rs` — `BbctlRunner` port (static-dispatch async), `LineControl`/`BbctlRunExit`, pure `bbctl_args`/`parse_bbctl_event`/`cap_message`, `run_self_hosted` orchestrator + FakeBbctlRunner I/O-matrix tests.
- `src/lib/stores/bbctl.ts` — zustand vanilla store for the run sheet.
- `src/hooks/use-bbctl-run.ts` — streaming run hook (cancelled-ref discipline).
- `src/components/bridges/bbctl-panel.tsx` (+test) — Beeper-only section, available/unavailable branches.
- `src/components/bridges/bbctl-run-sheet.tsx` (+test) — log-free phase stepper + Retry.
- `src/lib/ipc/gen/Bbctl{AvailabilityVm,InstallVm,NetworkVm,Phase,ProgressVm}.ts` — ts-rs bindings.

**Modified**
- `data.rs` (`bbctl_doc` loader/validator/`support_for`/`networks`), `vm.rs` (five VMs + `BbctlPhase`), `error.rs` (`BridgeError::Bbctl`), `account.rs` (`bbctl_availability` + gated `bbctl_run_start`), `bridges/mod.rs` (expose `bbctl`).
- `keeper/src/ipc.rs` (real `DesktopPlatform::sidecar_path`, `DesktopBbctlRunner`, `BbctlRunRegistry`, 3 commands, `to_ipc_error` arm), `keeper/src/lib.rs` (register commands), `src-tauri/Cargo.toml` (tokio `process`+`io-util` — no new crate).
- `client.ts` (wrappers), `bridges.ts` (`BBCTL_PHASE_LABEL`), `layout/bridges-pane.tsx` (mount for Beeper accounts only).

### Review findings breakdown

- **Patches applied (3, all medium):**
  1. Raw `bbctl` line no longer carried in non-terminal `BbctlProgressVm.message` / rendered by `ProgressPanel` — restores the log-free invariant (no raw line crosses IPC or reaches the UI).
  2. `DesktopBbctlRunner::run` stdout/stderr reader tasks wrapped in an `AbortOnDrop` guard — a `bbctl_run_cancel` (driver-task abort) now tears the readers down instead of leaking two tasks + pipe fds per cancel; the launched daemon is left running.
  3. The vacuous "cancels the run when closed" sheet test now actually closes the Sheet and asserts `cancel`; the non-terminal-phase test asserts the log-free label.
- **Deferred (2):** (a) a `bbctl run` that emits no recognized marker / never EOFs has no terminal state and no timeout — add a bounded start-timeout when tuning markers against a real binary; (b) the finite `bbctl register` child is not reaped on an early-`Stop` error (potential zombie) — a per-`register` detached reaper would close it without adding supervision (v1.x).
- **Rejected (16):** prose-marker substring fragility (BH1/2/3, Edge-1/2/3/4 — the spec's *documented residual risk*, tunable only against a real binary); misleading "Beeper only" message for a missing account (rare, cosmetic); ungated `bbctl_availability` (non-secret static data; the gate is required only on `bbctl_run_start`, which has it); success/retry double-fire, sheet-keying close, index-keyed steps, cancel-ref race (already guarded per spec); exit-0-without-marker (already handled → failure); registry finish-before-insert (no `await` between spawn and insert — unreachable); non-IpcError catch (masked by `FailurePanel` fallback); static-data edge cases (empty network id / provider casing — validator + trusted data); unbounded read (trusted sidecar); availability effect deps (availability is account-independent).

### Verification

- `bun run check:rust` — PASS (rustfmt clean + clippy `-D warnings`, no `.unwrap()`, no `async_fn_in_trait`).
- `bun run test:rust` — PASS (602 tests; incl. bbctl data/helpers/I/O-matrix + VM round-trip/no-token-leak).
- `bun run check` — PASS (Biome clean, tsc clean, 665 vitest tests incl. `bbctl-panel`/`bbctl-run-sheet`, core-tauri-free guard).

### Residual risks

- Live `bbctl` register/run against a real binary + real Beeper bridge is unattended-infeasible (documented residual risk, as with 6.3's provisioning shell): the `current_exe` resolution, `tokio::process` spawn/stream, real auth handshake, and the prose marker set are exercised only by the pure/seam unit tests. The prose markers are provisional and tunable against a real binary (see deferred items).
- `followup_review_recommended: true` — the reader-teardown (`AbortOnDrop`) change touches async task-lifecycle/process-teardown semantics and the log-free fix changes what crosses IPC; an independent pass is cheap insurance despite green gates.

### Follow-up review (iteration 3, independent pass)

The recommended independent follow-up ran (Blind Hunter + Edge Case Hunter). One medium patch was applied; all other findings were rejected (documented residual risk on the provisional prose markers, trusted-sidecar reads, and already-guarded races).

- **Patch (1, medium):** `BbctlRunRegistry` registered the driver-task abort handle *after* `tokio::spawn` (reserve → spawn → insert), silently violating the Code Map's mandated "insert-then-spawn (no resident handle)" + in-flight dedupe. Two real races followed: (a) on the common absent-sidecar fast path the driver could `finish()` before `insert()`, permanently leaking a stale abort handle in `tasks`; (b) a racing second `bbctl_run_start` for the same `(account, network)` could not abort the first (its handle wasn't in `tasks` yet), spawning **two** unsupervised `bbctl run` daemons. Fixed by putting both registry maps under one `Mutex<BbctlRunInner>` and adding `start(...)` that reserves the target, aborts any prior run, spawns, and inserts the handle **atomically under one lock**; `finish`/`cancel` take the same lock. No frontend or public-API change.
- **Verification:** `bun run check:rust` PASS (rustfmt + clippy `-D warnings`), `bun run test:rust` PASS (602 tests), `bun run check` PASS (Biome + tsc + 665 vitest + core-tauri-free guard).
- `followup_review_recommended: false` — a single localized concurrency fix under full green gates; no further independent pass warranted.
