---
title: 'NDJSON-RPC Handshake — getCapabilities & listSources'
type: 'feature'
created: '2026-07-16'
status: 'done'
baseline_revision: '6dd5ee58e7b8bc31467331a92335f4a588fc4da4'
final_revision: '516bbb2d1a87eeb9c1f5701173ed6a8ac029bdf2'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-16-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** Story 16.2 landed the platform-free session machine plus a shell `Recorder` port that only *reads* `keeper-rec`'s unsolicited stdout events — there is no host→sidecar request channel, no id-correlation, no protocol-version handshake, and no typed VMs for the capabilities/source contract. Without them keeper and `keeper-rec` can silently drift, a version skew could crash instead of degrading, and 16.5 (permission pre-flight) has no `getCapabilities` mechanism to consume.

**Approach:** Define the id-correlated host→sidecar request/response half of the NDJSON-RPC contract (AD-34) for the two read-only methods this story owns — `getCapabilities` (carries the protocol-version handshake; returns macOS version + feature flags + per-TCC permission states) and `listSources` (displays / applications / microphones / cameras) — surfaced as ts-rs VMs (AD-7). Keep the wire logic (request builders, response parsers, version check) pure and platform-free in `keeper-core::recording`; add the shell round-trip (piped stdin, id-correlated read, reap) to `DesktopRecorder`; extend the Swift sidecar from a one-shot answerer into a request loop. A version mismatch surfaces `CoreError::Unsupported` (never a crash); a malformed response surfaces the re-introduced `RecordingError::Protocol`. `start`/`stop`/live capture stay in 16.6.

## Boundaries & Constraints

**Always:**
- Wire format is **one JSON object per line on stdio** (AD-34). Requests are `{"id":<u64>,"method":<str>}`; responses are `{"id":<u64>,"result":{…}}` or `{"id":<u64>,"error":{"code":<str>,"message":<str>}}`. The host correlates by `id`, tolerating (skipping) any interleaved unsolicited event line while awaiting the matching response.
- The **contract shape is the invariant**; exact field lists stay code-owned (AD-34). New VMs live in `keeper-core::vm`, derive `serde` + `ts_rs::TS` with `#[serde(rename_all = "camelCase")]` + `#[ts(export)]`, and regenerate into `src/lib/ipc/gen/` via the existing export step (AD-7). Generated `.ts` is committed, never hand-edited.
- Protocol-version handshake: a single `keeper-core::recording::PROTOCOL_VERSION` (`= 1`) is the host's expected version; `getCapabilities.result.protocolVersion` is compared against it. Equal → proceed; unequal → `CoreError::Unsupported` (honest "unsupported keeper-rec protocol version N"), never a panic. The Swift stub already emits `protocolVersion: 1`.
- Wire serialization/parsing stays **platform-free** in `keeper-core::recording` (pure functions + the `Recorder` trait signature) — no `tauri`, no Apple framework, no process API; the existing `tests::dependency_firewall_holds` guard must keep passing. All process I/O (spawn, piped stdin, reap) lives only in `keeper/src/recorder.rs` under `#[cfg(desktop)]`.
- `getCapabilities`/`listSources` values that need only **CoreGraphics** are real in this story: `screenRecording` permission via `CGPreflightScreenCaptureAccess()` (the capturing process's authoritative, non-prompting grant) and `displays` via `CGGetActiveDisplayList`. CoreGraphics is a base Apple system framework (not ScreenCaptureKit/AVFoundation), so the Apache-2.0 / `cargo deny` posture is untouched.
- The macOS `Recorder` round-trip: resolve the sidecar via `Platform::sidecar_path("keeper-rec")` (absent → `CoreError::Unsupported`, no spawn), spawn with **stdin piped + stdout piped + `kill_on_drop`**, write one request line and flush, read stdout lines until the id-correlated response, then close stdin (EOF) so the sidecar exits, and reap. A response that arrived is honored even if a late non-zero exit follows (resolving deferred entry: "exit-code contract masking a reported terminal"). `run_session` (16.2) is untouched — it keeps stdin null.
- iOS (`IosRecorder`) and every non-desktop path return `CoreError::Unsupported` for the new methods — honest, no spawn, no panic (AD-27).

**Block If:**
- `listSources` is asked to surface anything beyond the typed VM contract (a real source-picker UI, live thumbnails, per-source config) — that is Epic 19 scope; do not invent it.
- The handshake is found to require changing the app-wide `minimumSystemVersion` (stays 11.0) or a stored-state migration — surface rather than proceed.

**Never:**
- No `start`/`stop`, no ScreenCaptureKit stream, no AVAssetWriter, no live capture, no segmentation (16.6).
- No AVFoundation/ScreenCaptureKit in the sidecar this story: `applications` (needs `SCShareableContent`) is an empty array; `microphones`/`cameras` (need AVFoundation) are empty arrays; `microphone`/`camera` TCC states are provisional `notDetermined`. The VM fields exist (shape locked); real enumeration/detection is 16.6/19, and the authoritative live permission pre-flight UI is 16.5.
- No tauri `#[command]` and no frontend consumer for the new VMs yet — this story stops at the port + typed contract + tests (the port is an allowed dead-code seam, like `run_session`); 16.5 wires the first IPC consumer.
- No new network destination, no upload/telemetry (local-only invariant, FR-76).
- No hand-editing generated `.ts`; no `os_info`/objc2 dependency.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| getCapabilities round-trip | host writes `{"id":1,"method":"getCapabilities"}` to a healthy sidecar | reads `{"id":1,"result":{protocolVersion,macos,features,permissions}}`; parsed to `RecordingCapabilitiesVm`; version matches | No error |
| Protocol version mismatch | response `protocolVersion` ≠ `PROTOCOL_VERSION` | `CoreError::Unsupported` ("unsupported … protocol version N"), never a crash | clean Unsupported |
| Malformed capabilities response | non-JSON, missing `result`, or `{"id":1,"error":{…}}` | `RecordingError::Protocol` → `CoreError::Recording` | surfaced, no panic |
| listSources round-trip | host writes `{"id":2,"method":"listSources"}` | `{"id":2,"result":{displays:[…real…],applications:[],microphones:[],cameras:[]}}` → `RecordingSourcesVm` | No error |
| Interleaved event before response | an unsolicited `{"event":"state",…}` line precedes the id-matched response | host skips the non-response line, returns the id-correlated response | No error |
| Event fixture stream | recorded NDJSON with `state`/`segmentClosed`/`error` lines + a blank + a garbage line | each recognized line → the matching `RecordingEvent`; blank/garbage skipped | no panic |
| Sidecar unavailable (dev/CI) | `sidecar_path` → `Unsupported` | `get_capabilities`/`list_sources` return `CoreError::Unsupported`; no spawn | honest |
| iOS | any call | `IosRecorder` → `CoreError::Unsupported` | no spawn/panic |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- NEW VMs (`#[ts(export)]`): `TccPermission` enum (`Granted|Denied|NotDetermined`, `rename_all="camelCase"`); `RecordingFeaturesVm` (`systemAudio`, `microphone`, `camera` bool flags); `RecordingCapabilitiesVm` (`protocolVersion:u32`, `macosVersion:String`, `features`, `screenRecording`, `microphone`, `camera` TCC states); `RecordingDisplayVm`/`RecordingApplicationVm`/`RecordingDeviceVm`; `RecordingSourcesVm` (`displays`, `applications`, `microphones`, `cameras`).
- `src-tauri/crates/keeper-core/src/recording.rs` -- add `pub const PROTOCOL_VERSION: u32 = 1;`; pure `capabilities_request(id)`/`list_sources_request(id) -> String`; pure `response_id(line) -> Option<u64>`; pure `parse_capabilities_result(line) -> Result<vm::RecordingCapabilitiesVm, RecordingError>` and `parse_sources_result(line) -> Result<vm::RecordingSourcesVm, RecordingError>` (surface a sidecar `error` object and any malformed/missing `result` as `RecordingError::Protocol`); extend the `Recorder` trait with `get_capabilities()` + `list_sources()` (async, `impl Future<…> + Send`); new unit tests (request shape, response parse, version check, event fixture-stream, protocol/error surfacing). Dependency-firewall guard must still pass.
- `src-tauri/crates/keeper-core/src/error.rs` -- re-introduce `RecordingError::Protocol(String)` (the variant 16.2 removed, reserved for "16.4 adds protocol errors with the typed wire contract"); `CoreError::Recording(_)` funnel stays exhaustive (no `to_ipc_error` arm change).
- `src-tauri/crates/keeper/src/recorder.rs` -- implement `get_capabilities`/`list_sources` on `DesktopRecorder`: extract a private `async fn request_response(path, request_line, id) -> Result<String, CoreError>` (spawn stdin-piped + stdout-piped + `kill_on_drop`, write+flush, read byte-level lines until `response_id == id` skipping others, close stdin, reap honoring an already-received response) that both methods call; version-check + parse via the core pure fns; mismatch → `Unsupported`. `IosRecorder` returns `Unsupported`. Add a `#[cfg(all(test, desktop))]` fake-executable NDJSON harness (write a tiny temp script, drive `request_response` against it) exercising spawn/stream/reap without hardware (resolves the deferred "run_session spawn/stream/reap untested" entry for the request path).
- `src-tauri/crates/keeper-core/src/recording.rs` (FakeRecorder in tests) & `keeper/src/recorder.rs` (`IosRecorder`) -- implement the two new trait methods (canned VMs / `Unsupported`) so the trait stays object-safe-free and all impls compile.
- `tools/keeper-rec/Sources/keeper-rec/main.swift` -- convert the single-line answerer into a **request loop**: read lines until EOF; per line parse `{id,method}`; `getCapabilities` → result with `protocolVersion`, `macos`, `features`, and `permissions{screenRecording via CGPreflightScreenCaptureAccess, microphone:"notDetermined", camera:"notDetermined"}`; `listSources` → result with real `displays` (CGGetActiveDisplayList) + empty `applications`/`microphones`/`cameras`; unknown method → `{id,error}`; malformed line → skip; EOF → exit 0. Keep the SIGPIPE-ignore + no-forced-unwrap safety.
- `tools/keeper-rec/Package.swift` -- link CoreGraphics if the SDK requires it for `CGPreflightScreenCaptureAccess`/`CGGetActiveDisplayList` (`.linkedFramework("CoreGraphics")`); no third-party dependency.
- `src/lib/ipc/gen/*.ts` -- GENERATED for each new VM by `bun run test:rust`; commit, never hand-edit.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/error.rs` -- add `RecordingError::Protocol(String)` (`#[derive(Debug, Error)]`, honest non-secret message) -- the typed-wire-contract error slot 16.2 reserved; confirm `CoreError::Recording(_)` `to_ipc_error` arm stays exhaustive.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `TccPermission`, `RecordingFeaturesVm`, `RecordingCapabilitiesVm`, `RecordingDisplayVm`, `RecordingApplicationVm`, `RecordingDeviceVm`, `RecordingSourcesVm` with the `serde`+`ts_rs::TS`+`#[ts(export)]`+camelCase pattern -- the code-owned typed contract (AD-7); regenerates the `.ts` bindings.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- add `PROTOCOL_VERSION`; pure request builders, `response_id`, and `parse_capabilities_result`/`parse_sources_result` (map sidecar `error`/malformed/missing `result` → `RecordingError::Protocol`); extend the `Recorder` trait with `get_capabilities()`/`list_sources()`; update `FakeRecorder` to implement them -- the platform-free wire half (AD-34), staying inside the dependency firewall.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` (tests) -- add: request-line shape tests; `parse_capabilities_result`/`parse_sources_result` happy + malformed + sidecar-`error` cases; a `PROTOCOL_VERSION` match/mismatch test; and an **event fixture-stream** test driving a multi-line `const` NDJSON stream (with `state`/`segmentClosed`/`error` + a blank + a garbage line) through `parse_event`, asserting the exact `RecordingEvent` sequence with blank/garbage skipped -- satisfies the AC "unit tests against a recorded fixture stream," no hardware.
- [x] `src-tauri/crates/keeper/src/recorder.rs` -- implement `DesktopRecorder::{get_capabilities,list_sources}` via a shared private `request_response(path, request, id)` (piped stdin, id-correlated stdout read skipping interleaved events, close stdin, reap honoring a received response even on a late non-zero exit); run the version handshake (mismatch → `CoreError::Unsupported`) and parse via the core fns; `IosRecorder` returns `Unsupported` -- the shell round-trip seam (AD-24/AD-27).
- [x] `src-tauri/crates/keeper/src/recorder.rs` (tests) -- add a `#[cfg(all(test, desktop))]` fake-executable harness: write a minimal temp executable that reads a request line and echoes a canned NDJSON response, then assert `request_response` returns the id-correlated line and reaps cleanly (and a mismatched-version canned response → `Unsupported`) -- exercises spawn/stream/reap without a signed sidecar.
- [x] `tools/keeper-rec/Sources/keeper-rec/main.swift` -- convert to the request loop answering `getCapabilities` (real `screenRecording` preflight, provisional mic/camera, features) and `listSources` (real `displays`, empty apps/mics/cameras); unknown method → `{id,error}`; malformed line skipped; EOF → clean exit 0 -- the sidecar half of the contract.
- [x] `tools/keeper-rec/Package.swift` -- link CoreGraphics if required by the SDK; keep zero third-party dependencies -- capabilities/displays without SCK/AVFoundation.

**Acceptance Criteria:**
- Given the handshake, when `getCapabilities` round-trips against a healthy sidecar reporting `protocolVersion == PROTOCOL_VERSION`, then the host parses a `RecordingCapabilitiesVm` (macOS version, feature flags, screen-recording TCC state) and proceeds; when the reported version differs, it resolves `CoreError::Unsupported` with no panic (AD-34).
- Given `listSources`, when it round-trips, then the host parses a `RecordingSourcesVm` whose `displays` reflect the real active displays and whose `applications`/`microphones`/`cameras` are empty (shape-complete, enumeration deferred), surfaced as ts-rs VMs (AD-7).
- Given the parser, when the event fixture stream is replayed line-by-line, then `state`/`segmentClosed`/`error` lines map to their `RecordingEvent`s and blank/garbage lines are skipped, all without a live signed capture (AD-34).
- Given the sidecar is unavailable (dev/CI) or the platform is iOS, when `get_capabilities`/`list_sources` are called, then each returns `CoreError::Unsupported` with no spawn and no panic; and `keeper-core::recording` still carries no tauri/Apple/process token (`dependency_firewall_holds` passes).
- Given `bun run check`, `bun run check:rust`, `bun run test:rust`, and `bash scripts/build-keeper-rec.sh`, then biome/tsc/vitest, clippy (`-D warnings`), cargo-nextest (with the new VM `.ts` regenerated and committed), and the Swift release build all pass.

## Design Notes

**Pure/shell split (keeps it testable without a Mac).** All wire *logic* is pure `keeper-core::recording` fns — request serialization, `response_id`, `parse_*_result`, and the `PROTOCOL_VERSION` comparison — unit-tested directly. The shell `request_response` is the only untested-by-pure-logic seam; the fake-executable harness closes it. This mirrors 16.2's design (pure `parse_event` + `drive_session`, thin shell port) and keeps the dependency firewall intact (no process/Apple token enters `recording.rs`).

**Id-correlation is new (bbctl has none).** bbctl is launch-and-leave line-streaming with no request envelope; there is no existing pattern to reuse, so 16.4 introduces the minimal one: write one `{"id",…}` request, read lines until `response_id == id`, skipping interleaved events (none occur during the handshake today, but the reader must tolerate them for 16.6). One spawn per request keeps the handshake path simple; the persistent `start`/`stop`/event session is 16.6's concern.

**CoreGraphics-level honesty, SCK/AVFoundation-level deferral.** `CGPreflightScreenCaptureAccess()` reports the *sidecar's own* screen-recording grant — exactly the process that will capture in 16.6 — so reporting it now is authoritative and directly feeds 16.5's pre-flight (which still owns the request/prompt, deep-link, live re-detection, and UI). `CGGetActiveDisplayList` gives real displays. Everything needing `SCShareableContent` (apps) or AVFoundation (mic/camera devices + their TCC states) stays empty/`notDetermined` — the VM shape is the locked invariant, and real enumeration lands with capture (16.6/19).

**Version-mismatch → Unsupported, malformed → Protocol.** A reachable, understood sidecar that simply speaks a different protocol is an *unsupported* condition (reuse the honest not-available funnel, like `sidecar_path`); a response we cannot even parse is a *protocol* fault (`RecordingError::Protocol` → `CoreError::Recording`). Both are clean and non-panicking, satisfying "a version mismatch yields a clean Unsupported/error surface, never a crash."

## Verification

**Commands:**
- `cd src-tauri && cargo test -p keeper-core recording` -- expected: request/response/version/fixture-stream + existing lifecycle & firewall tests pass.
- `bun run test:rust` -- expected: cargo-nextest green; regenerates `src/lib/ipc/gen/RecordingCapabilitiesVm.ts`, `RecordingSourcesVm.ts`, `RecordingFeaturesVm.ts`, `TccPermission.ts`, `RecordingDisplayVm.ts`, `RecordingApplicationVm.ts`, `RecordingDeviceVm.ts` (commit them).
- `bun run check:rust` -- expected: `cargo fmt --check` + clippy `--all-targets -D warnings` clean across both crates.
- `bun run check` -- expected: biome + tsc + vitest pass (generated `.ts` compiles; no frontend consumer added).
- `bash scripts/build-keeper-rec.sh` -- expected: `swift build -c release --arch arm64` succeeds with the request loop + CoreGraphics; installs `keeper-rec-aarch64-apple-darwin`.
- `cd src-tauri && cargo check --workspace --target aarch64-apple-ios` -- expected: compiles; `IosRecorder` new methods return `Unsupported`.

**Manual checks:**
- Confirm `keeper-core/src/recording.rs` imports no tauri/Apple-framework/process API and the new round-trip I/O lives only in `keeper/src/recorder.rs` under `#[cfg(desktop)]`.

## Review Triage Log

### 2026-07-16 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 5: (high 0, medium 1, low 4)
- defer: 4
- reject: 7
- addressed_findings:
  - `[medium]` `[patch]` The protocol-version handshake was defeated by parse order: `fetch_capabilities` parsed the full `RecordingCapabilitiesVm` (which hard-requires the v1 `features`/`permissions` shape) *before* calling `verify_protocol_version`, so a future sidecar whose result shape changed across a version bump would surface an opaque `Protocol` fault instead of the intended honest `Unsupported` — defeating the handshake's whole purpose. Added a pure `recording::response_protocol_version(line)` that extracts only `result.protocolVersion`; `fetch_capabilities` now verifies the version *before* validating the full shape. Strengthened the mismatch test to use a shape-changed v2 fixture (asserting `Unsupported`, not `Protocol`) and added a pure `response_protocol_version` unit test — the prior test only bumped the number while keeping the v1 shape, giving false confidence.
  - `[low]` `[patch]` The sidecar mapped `CGPreflightScreenCaptureAccess() == false` to `"denied"`, but the boolean preflight cannot distinguish an explicit denial from a never-requested state; asserting `denied` to a first-run user would wrongly steer 16.5 to a dead-end "open System Settings" prompt. Now maps false → `"notDetermined"` and corrected the `TccPermission` / `screen_recording` VM docs (which had over-claimed "authoritative") to state the two-valued limitation and 16.5's ownership of the granted/not-requested/denied tri-state.
  - `[low]` `[patch]` `main.swift` hard-imports CoreGraphics but `Package.swift` linked nothing — the build relied on implicit SDK umbrella linking that a stricter toolchain could break with no test to catch it. Added an explicit `.linkedFramework("CoreGraphics")` for reproducibility (ScreenCaptureKit/AVFoundation still unlinked — 16.6).
  - `[low]` `[patch]` The Swift request loop ignored `writeLine`'s return, so a broken stdout (host closed the read end) would spin the loop instead of exiting — violating the "always exits cleanly" invariant. Now breaks the loop on a failed write (single write per iteration).
  - `[low]` `[patch]` Added a cross-reference comment tying `keeper_core::recording::PROTOCOL_VERSION` to the Swift `capabilitiesResult()` literal (no shared source of truth across the language boundary; a drift is caught at runtime as an honest `Unsupported`, but the two must be bumped together).

_Deferred (real, tracked to 16.6 hardening — see deferred-work.md):_ (1) `request_response` has no round-trip/read timeout, so a spawned sidecar that reads the request then hangs blocks the call forever (bounded round-trips especially warrant a timeout; 16.6 owns lifecycle timeout policy); (2) unbounded response-line buffering (`read_until` with no length cap) on the request path; (3) `RecordingError::Protocol` and the sidecar `error.code`/`error.message` interpolate sidecar-supplied strings verbatim — safe now (keeper-rec emits only benign strings) but must be capped/scrubbed alongside the deferred `Failed`-message sanitization once 16.6 flows real capture paths/device names; (4) fixed per-method correlation ids (1/2) with no method-echo verification work only under one-request-per-spawn — 16.6's persistent multi-request session needs monotonic request ids + response/method validation.

_Rejected (noise / by-design / ruled out):_ a `getCapabilities` `{error}` answer surfacing as `Protocol` rather than `Unsupported` (a sidecar that won't answer getCapabilities cannot have its version negotiated — `Protocol` is the honest surface, and the AC accepts an "Unsupported/error" surface); the CoreGraphics display over-read on a hot-plug race (`CGGetActiveDisplayList`'s `maxDisplays` arg caps writes at the allocated size, so `.prefix(count)` can never over-read; a count-*grow* silently drops new displays, acceptable best-effort); "EOF mid-line drops the final response" (`read_until` delivers a final unterminated line on the read that hits EOF, and the sidecar always newline-terminates — false positive); the Swift null-id / non-u64-id correlation paths (the host always sends a `u64` id per the wire contract, so these are unreachable); `listSources` not re-verifying the protocol version (the contract puts the handshake on `getCapabilities` by design); the write-before-read deadlock (requests are a single tiny line — not reachable); `protocolVersion > u32::MAX` → `Protocol` (correctly handled — a huge version is malformed, not a skew).

### 2026-07-16 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 0
- reject: 13
- addressed_findings:
  - none

_Follow-up independent review (Blind Hunter + Edge Case Hunter, no prior context) over the full 16.4 diff since `6dd5ee5`. All ~13 distinct findings resolved to reject — no new defect, no new deferrable item, no spec gap. Rationale by cluster:_
- _Already in the deferred-work ledger (orchestrator-owned; not re-opened/duplicated per invocation directive): no round-trip/reap timeout (item 1), unbounded `read_until` line cap (item 2), verbatim sidecar-string interpolation in `Protocol`/`error` messages (item 3). The Blind Hunter's argument to reprioritize the timeout for 16.5 is a scheduling call the orchestrator owns, not a new finding._
- _Already rejected in the prior pass and unchanged: non-`u64` id echo (host always sends `u64`), write-before-read deadlock (single tiny request line)._
- _Already an acknowledged residual risk: no automated test drives the real Rust host against the real signed Swift `JSONSerialization` producer (all round-trip tests use hand-authored JSON) — the real host-vs-signed-sidecar pass arrives with 16.6's human-in-the-loop step. F3 (cross-language numeric boundary) and F10 fold into this._
- _New but not actionable: `"macos"` wire key not camelCase like its siblings (valid, working, tested, code-owned choice; the drift risk is a specific instance of the tracked no-cross-boundary-test gap — churning a green two-language wire contract for style parity in an unattended run is unwarranted); `displays` empty-array conflates CoreGraphics failure with no-displays and the `CGGetActiveDisplayList` count TOCTOU (by-design best-effort degrade, consistent with the prior pass's accepted display-race posture); strict-within-v1 shape for `features`/all-four source lists (code-owned parse strictness; the sidecar always emits them); `to_ipc_error` exhaustiveness for the new `Protocol` variant (the workspace compiles green, so the funnel match is exhaustive by construction); version test asserting only `+1` (the equality check is symmetric and the prior pass already added a shape-changed v2 mismatch fixture)._

## Auto Run Result

Status: done

**Summary.** Implemented Story 16.4 — the host↔`keeper-rec` NDJSON-RPC request/response contract for the two read-only, id-correlated methods this story owns: `getCapabilities` (protocol-version handshake + macOS version + feature flags + per-TCC permission states) and `listSources` (displays / applications / microphones / cameras), surfaced as ts-rs VMs. Wire logic (request builders, `response_id`, `response_protocol_version`, `parse_capabilities_result`/`parse_sources_result`, `verify_protocol_version`) is pure and platform-free in `keeper-core::recording`; the shell round-trip (piped stdin, id-correlated read skipping interleaved events, reap honoring a received response even on a late non-zero exit) lives in `keeper/src/recorder.rs`; the Swift sidecar became a request loop. A protocol-version mismatch resolves `CoreError::Unsupported` (verified *before* full-shape validation); a malformed/`error` response resolves the re-introduced `RecordingError::Protocol`. `start`/`stop`/live capture stay in 16.6.

**Files changed.**
- `src-tauri/crates/keeper-core/src/error.rs` — re-introduced `RecordingError::Protocol(String)` (the typed-wire-contract slot 16.2 reserved); `CoreError::Recording(_)` funnel stays exhaustive.
- `src-tauri/crates/keeper-core/src/vm.rs` — 7 new ts-rs VMs (`TccPermission`, `RecordingFeaturesVm`, `RecordingCapabilitiesVm`, `RecordingDisplayVm`, `RecordingApplicationVm`, `RecordingDeviceVm`, `RecordingSourcesVm`); honesty-corrected `screen_recording`/`TccPermission` docs.
- `src-tauri/crates/keeper-core/src/recording.rs` — `PROTOCOL_VERSION`, pure request/response wire fns incl. `response_protocol_version` (version-before-shape), the extended `Recorder` trait (`get_capabilities`/`list_sources`), and unit tests (request shape, parsing, version match/mismatch, `response_protocol_version`, event fixture-stream).
- `src-tauri/crates/keeper/src/recorder.rs` — `request_response` round-trip + `fetch_capabilities` (verify version before parse) / `fetch_sources`; `DesktopRecorder`/`IosRecorder` new methods; fake-executable NDJSON harness (interleaved-skip, late-exit honored, silent-exit→Protocol, shape-changed-v2→Unsupported, sources round-trip).
- `tools/keeper-rec/Sources/keeper-rec/main.swift` — one-shot answerer → request loop; real `screenRecording` preflight (false→`notDetermined`) + real `displays` (CoreGraphics); broken-pipe `break`; protocol-version cross-ref.
- `tools/keeper-rec/Package.swift` — explicit `.linkedFramework("CoreGraphics")`.
- `src/lib/ipc/gen/*.ts` — 7 new generated bindings (regenerated, committed, not hand-edited).

**Review findings breakdown.** 5 patches applied (1 medium: version-before-shape handshake correctness + regression test; 4 low: screen-recording honesty, explicit CoreGraphics linking, broken-pipe break, protocol-version cross-ref). 4 deferred to `deferred-work.md` (round-trip timeout, unbounded line cap, Protocol-message sanitization, monotonic ids for 16.6's persistent session). 7 rejected (noise / by-design / ruled out). No intent_gap, no bad_spec, no repair loopback.

**Follow-up review recommended:** true — the review-driven changes altered the core handshake's control flow (version-before-shape) and a wire-value semantic (`screenRecording` now `notDetermined` for a false preflight) that Story 16.5 consumes directly, across five files in two languages. Though each is tested and all gates are green, an independent follow-up on this foundational cross-language contract before 16.5 builds on it is warranted.

**Verification.** All six gates re-run green after the patches: `bun run check:rust` (fmt + clippy `-D warnings`) clean; `bun run test:rust` → 840 passed (7 new `.ts` bindings regenerated, docs updated for the honesty fix); `bun run check` → biome + tsc + vitest (1261 passed) + core-tauri-free check clean; `bash scripts/build-keeper-rec.sh` → swift release build + smoke green (smoke now reports `screenRecording:"notDetermined"`); `cargo check --workspace --target aarch64-apple-ios` → exit 0 (`IosRecorder` new methods → `Unsupported`; the lone `dead_code` warning is the pre-existing Story 16.3 `parse_macos_major` iOS warning, unchanged).

**Residual risks.** No automated test drives the real Rust host against the real signed Swift sidecar (needs the bundled per-triple binary next to the test executable — arrives with 16.6's human-in-the-loop pass; the fake-executable harness covers the spawn/stream/reap round-trip in the meantime). Feature flags and mic/camera states are honest placeholders (real detection is 16.6/19); `listSources` applications/microphones/cameras are empty until SCK/AVFoundation enumeration lands. The four deferred items (round-trip timeout, line cap, message sanitization, monotonic ids) are process-lifecycle/robustness hardening that 16.6 owns.

---

**Follow-up review pass (2026-07-16).** An independent Blind Hunter + Edge Case Hunter pass (no prior context) re-examined the full 16.4 diff since `6dd5ee5`. Outcome: **no actionable findings** — 13 distinct findings all resolved to reject (0 intent_gap, 0 bad_spec, 0 patch, 0 defer). Every real robustness item raised (round-trip timeout, unbounded line cap, message sanitization, id correlation) is already an orchestrator-owned deferred-work entry; the systemic "no real Rust↔signed-Swift round-trip test" gap is already an acknowledged residual risk landing with 16.6; and the remaining items (the `"macos"` non-camelCase wire key, the `displays` empty-vs-failure conflation, strict-within-v1 shape parsing) are working, tested, code-owned choices, not defects. The prior work holds; `followup_review_recommended` is now `false` (this pass made no code changes). No new deferred-work entries were appended.
