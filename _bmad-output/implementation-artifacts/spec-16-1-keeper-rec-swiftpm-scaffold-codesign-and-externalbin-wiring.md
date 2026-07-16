---
title: 'keeper-rec SwiftPM Scaffold, Codesign & externalBin Wiring'
type: 'chore'
created: '2026-07-16'
status: 'done'
baseline_revision: 'a63e4956b4e12f966bcbed940e69b3457cee624e'
final_revision: 'b4eadf545dbbcf2772ddfe606aaaed8d312ef1a5'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-16-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** Every later recording story (16.2–16.6) needs to spawn a real, signed `keeper-rec` capture sidecar via `Platform::sidecar_path`, but no such binary exists, is bundled, or is signed. Without the scaffold + codesign + `externalBin` wiring first, the three existential risks (TCC, sidecar signing, capture-to-file) cannot be retired.

**Approach:** Add a first-party Apache-2.0 SwiftPM package `tools/keeper-rec/` (outside the Cargo workspace) whose stub binary answers one `getCapabilities` NDJSON line on stdio and exits cleanly. Build it to `binaries/keeper-rec-<triple>`, declare it as Tauri `bundle.externalBin`, keep every local/CI Tauri build working by building the sidecar first, and codesign it (hardened runtime + entitlements) before `tauri build` in the release pipeline. No real capture logic (deferred to 16.6).

## Boundaries & Constraints

**Always:**
- SwiftPM package lives at top-level `tools/keeper-rec/` (`Package.swift` + `Sources/keeper-rec/`), **outside `src-tauri/crates/`** so Cargo and SwiftPM tooling never collide; first-party **Apache-2.0**; links **only Apple system frameworks** (Foundation now; ScreenCaptureKit/AVFoundation land in 16.6) — no ffmpeg, no third-party SwiftPM dependency, so `cargo deny check` stays untouched (Swift is not in the tree it scans).
- Platform floor `.macOS(.v13)`; aarch64-only, **no lipo/universal** step.
- `bundle.externalBin` declared exactly as `binaries/keeper-rec` (relative to `tauri.conf.json`); Tauri appends the triple, so the resolved runtime name is `keeper-rec-aarch64-apple-darwin`, matching `DesktopPlatform::sidecar_path`'s `<name>-<triple>` lookup.
- Every macOS Tauri build/dev (`bun run tauri:dev`, `bun run tauri:build`, the CI `--no-bundle` build) must still succeed: the sidecar is built into `binaries/` **before** Tauri consults `externalBin`.
- Release pipeline builds `keeper-rec` with `swift build -c release --arch arm64`, then **explicitly codesigns** it (hardened runtime + entitlements) **before** the `tauri build`/tauri-action step (the `externalBin` notarization rough edge, tauri#11992).
- The stub binary is spawned as a plain child process later (never a LaunchAgent); it must never crash on bad/absent input.

**Block If:**
- The only way found to keep local `tauri dev`/`tauri build` working would require committing a prebuilt binary into the repo (violates repo hygiene / secret-scan posture). Surface this instead of committing a binary. (No such need is expected — the build-first wiring avoids it.)

**Never:**
- No real capture (SCK stream, AVAssetWriter, sample buffers), no `start`/`stop`/`listSources` implementation, no segmentation — all deferred to 16.4/16.6.
- No Rust changes: do **not** add the `Recorder` port or `keeper-core::recording` (those are 16.2); `sidecar_path` already exists.
- No new network destination, no upload/telemetry from the sidecar (local-only invariant, FR-76).
- No third-party dependency (Swift or Rust); no ffmpeg; no lipo/universal build; no LaunchAgent/LaunchDaemon plist.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| getCapabilities | stdin line `{"id":1,"method":"getCapabilities"}` | one NDJSON line to stdout echoing `id` and carrying `protocolVersion` (int) + `macos` (OS version string); flush; exit 0 | none expected |
| EOF / closed stdin | stdin closed with no line | exit 0, no output required | clean exit, no crash |
| Unknown method | `{"id":2,"method":"start"}` | one NDJSON error line (`id` echoed, `error` object) OR silent clean exit; process exits 0 | must not panic/hang |
| Malformed JSON | `not-json` | clean exit (0), no partial/garbage output | must not panic |

</intent-contract>

## Code Map

- `tools/keeper-rec/Package.swift` -- NEW: SwiftPM manifest, executable target `keeper-rec`, `.macOS(.v13)`, no dependencies.
- `tools/keeper-rec/Sources/keeper-rec/main.swift` -- NEW: stub NDJSON-RPC binary; reads one stdin line, answers `getCapabilities`, exits cleanly; carries the dev-signing (Cap #1722) code comment.
- `tools/keeper-rec/README.md` -- NEW: one-paragraph purpose + build pointer + Apache-2.0 note.
- `scripts/build-keeper-rec.sh` -- NEW: `swift build -c release --arch arm64` + install product to `src-tauri/crates/keeper/binaries/keeper-rec-aarch64-apple-darwin`.
- `src-tauri/crates/keeper/keeper-rec.entitlements` -- NEW: minimal hardened-runtime entitlements plist (empty dict; capture-specific entitlements revisited in 16.6).
- `src-tauri/crates/keeper/tauri.conf.json` -- add `bundle.externalBin: ["binaries/keeper-rec"]`.
- `package.json` -- add `rec:build` script; chain it into `tauri:dev` and `tauri:build`.
- `.gitignore` -- ignore the built-artifact dir `src-tauri/crates/keeper/binaries/`.
- `.github/workflows/release.yml` -- build + codesign `keeper-rec` before both the signed (tauri-action) and unsigned (`tauri build`) steps.
- `docs/release.md` -- add a `keeper-rec` build + DevEx dev-signing note.

## Tasks & Acceptance

**Execution:**
- [x] `tools/keeper-rec/Package.swift` -- declare `swift-tools-version` (installed toolchain), executable target `keeper-rec`, `platforms: [.macOS(.v13)]`, zero dependencies -- establishes the package outside the Cargo workspace.
- [x] `tools/keeper-rec/Sources/keeper-rec/main.swift` -- read one line from stdin; parse as JSON; if `method == "getCapabilities"` write a single JSON line to stdout `{"id":<echoed>,"result":{"protocolVersion":1,"macos":"<ProcessInfo os version>"}}`, flush, exit 0; on EOF/malformed/unknown method exit 0 without crashing; include SPDX Apache-2.0 header and a `// Cap #1722` comment noting macOS 15+ rejects ad-hoc-signed SCK so real capture needs an Apple Development certificate -- the RPC handshake seed + code-level DevEx note.
- [x] `tools/keeper-rec/README.md` -- purpose, `bash scripts/build-keeper-rec.sh`, Apache-2.0 -- orientation.
- [x] `scripts/build-keeper-rec.sh` -- `set -euo pipefail`; `swift build -c release --arch arm64 --package-path tools/keeper-rec`; resolve product via `swift build ... --show-bin-path`; `mkdir -p` the dest dir; copy product to `src-tauri/crates/keeper/binaries/keeper-rec-aarch64-apple-darwin` -- deterministic build+install used by dev, CI, and release.
- [x] `src-tauri/crates/keeper/keeper-rec.entitlements` -- minimal plist enabling hardened runtime (empty `<dict/>` is sufficient; SCK/mic are TCC-gated at runtime, not entitlement-gated) -- signing input.
- [x] `src-tauri/crates/keeper/tauri.conf.json` -- add `"externalBin": ["binaries/keeper-rec"]` under `bundle` -- bundles the sidecar per-arch.
- [x] `package.json` -- add `"rec:build": "bash scripts/build-keeper-rec.sh"`; change `tauri:dev`/`tauri:build` to run `bun run rec:build && <existing command>` -- keeps every local + CI (`bun run tauri:build -- --no-bundle`) Tauri invocation self-contained so `externalBin` always resolves.
- [x] `.gitignore` -- add `src-tauri/crates/keeper/binaries/` -- the built sidecar is a generated artifact, never committed.
- [x] `.github/workflows/release.yml` -- before the signed `tauri-action` step: import the Developer ID cert into a temp keychain (reusing `APPLE_CERTIFICATE`/`APPLE_CERTIFICATE_PASSWORD`/`KEYCHAIN_PASSWORD`), run `rec:build`, then `codesign --force --options runtime --entitlements src-tauri/crates/keeper/keeper-rec.entitlements --sign "$APPLE_SIGNING_IDENTITY" <binary>`; before the unsigned `tauri build` step: run `rec:build` then ad-hoc `codesign --force --options runtime -s - <binary>` -- signed sidecar exists before Tauri bundles/notarizes (tauri#11992).
- [x] `docs/release.md` -- add a short "Recording sidecar (keeper-rec)" note: it is built + codesigned before the app bundle in CI, and local builds that exercise real recording need an Apple Development certificate because macOS 15+ silently rejects ad-hoc-signed ScreenCaptureKit (Cap #1722) — a DevEx requirement, not a product blocker -- honest dev/release documentation.
- [x] `tools/keeper-rec/` (smoke) -- add a stdin→stdout smoke assertion to the build script or a `scripts/smoke-keeper-rec.sh` covering the I/O matrix (getCapabilities echoes id; malformed input exits 0) -- verifies the stub without hardware.

**Acceptance Criteria:**
- Given the built sidecar, when `echo '{"id":1,"method":"getCapabilities"}'` is piped to `src-tauri/crates/keeper/binaries/keeper-rec-aarch64-apple-darwin`, then it prints one JSON line containing `"id":1` and a `protocolVersion`, and exits 0.
- Given the package, when the license firewall runs (`cargo deny check licenses bans sources`), then it still passes because no Rust dependency or non-Apple framework was added.
- Given `bundle.externalBin` is declared, when `bun run tauri:build -- --no-bundle` runs on macOS, then it succeeds because `rec:build` produced `keeper-rec-aarch64-apple-darwin` before Tauri resolved the sidecar.
- Given the release workflow, when the signed path runs, then `keeper-rec` is built and codesigned with hardened runtime + entitlements before `tauri-action`, so the notarized bundle contains a signed sidecar.
- Given no Rust source changed, when `bun run check:rust` and `bun run test:rust` run, then they pass unchanged (no `Recorder` port introduced here).

## Spec Change Log

_No spec amendments — review produced no intent_gap or bad_spec findings._

## Review Triage Log

### 2026-07-16 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 0, low 4)
- defer: 1: (high 0, medium 1, low 0)
- reject: 5: (high 0, medium 0, low 5)
- addressed_findings:
  - `[low]` `[patch]` Swift stub could die with SIGPIPE (exit 141) if the parent closed stdout mid-write — added `signal(SIGPIPE, SIG_IGN)` and switched to the throwing `write(contentsOf:)` so a broken pipe fails as a caught error and the stub still exits 0.
  - `[low]` `[patch]` Smoke test's `"id":1` substring assertion was brittle (also matches `"id":10`) — replaced with a real JSON parse asserting `id` is echoed verbatim and `result.protocolVersion` is an int.
  - `[low]` `[patch]` Build script hardcoded arm64 with no host guard — added a `uname -s/-m` check that fails with a clear Apple-Silicon-only message instead of a cryptic Tauri "sidecar not found".
  - `[low]` `[patch]` Signed-path codesign step didn't assert `APPLE_SIGNING_IDENTITY` is set (the `signed==true` gate doesn't cover it) — added a non-empty guard for a clear early failure.

## Design Notes

**externalBin makes the sidecar a hard build input.** Tauri resolves `bundle.externalBin` (looking for `<name>-<triple>`) during dev and bundling; a missing file fails the build. Rather than committing a binary, `tauri:dev`/`tauri:build` are chained through `rec:build` so the artifact is always freshly built first. The CI `build` job calls `bun run tauri:build -- --no-bundle`, inheriting the chain; the release job uses tauri-action / raw `tauri build`, so it gets explicit build+codesign steps.

**iOS is unaffected.** The iOS CI gate is `cargo check` (not `tauri build`), so `externalBin` is never consulted there; iOS never records. A hypothetical `tauri ios build` would look for `keeper-rec-<ios-triple>` — out of scope this phase (documented limitation).

**Stub shape, code-owned fields (AD-34).** The exact `getCapabilities` field list is owned by 16.4's typed VMs; here it only needs a valid id-correlated JSON line carrying a `protocolVersion` (handshake seed) and the OS version. Keep it minimal and forward-compatible.

Golden `main.swift` sketch (illustrative, ~8 lines of logic):
```swift
// SPDX-License-Identifier: Apache-2.0
// Cap #1722: macOS 15+ silently rejects ad-hoc-signed ScreenCaptureKit — real
// capture builds need an Apple Development certificate (DevEx, not a product blocker).
import Foundation
guard let line = readLine(strippingNewline: true),
      let data = line.data(using: .utf8),
      let msg = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
      msg["method"] as? String == "getCapabilities" else { exit(0) }
let v = ProcessInfo.processInfo.operatingSystemVersion
let os = "\(v.majorVersion).\(v.minorVersion).\(v.patchVersion)"
let resp: [String: Any] = ["id": msg["id"] ?? NSNull(),
                           "result": ["protocolVersion": 1, "macos": os]]
FileHandle.standardOutput.write(try! JSONSerialization.data(withJSONObject: resp))
FileHandle.standardOutput.write("\n".data(using: .utf8)!)
```
(Replace `try!` with a safe write in the final code — no forced unwrap in shipped paths.)

## Verification

**Commands:**
- `swift build -c release --arch arm64 --package-path tools/keeper-rec` -- expected: clean build, no warnings.
- `bash scripts/build-keeper-rec.sh` -- expected: produces `src-tauri/crates/keeper/binaries/keeper-rec-aarch64-apple-darwin`.
- `echo '{"id":1,"method":"getCapabilities"}' | src-tauri/crates/keeper/binaries/keeper-rec-aarch64-apple-darwin` -- expected: one JSON line containing `"id":1` and `protocolVersion`; exit 0.
- `printf 'not-json\n' | src-tauri/crates/keeper/binaries/keeper-rec-aarch64-apple-darwin; echo $?` -- expected: exit 0, no crash.
- `codesign --force --options runtime -s - src-tauri/crates/keeper/binaries/keeper-rec-aarch64-apple-darwin && codesign -dv --verbose=2 src-tauri/crates/keeper/binaries/keeper-rec-aarch64-apple-darwin` -- expected: ad-hoc signature present (local proof of the codesign wiring).
- `bun run tauri:build -- --no-bundle` -- expected: succeeds; `externalBin` resolves the freshly built sidecar.
- `cargo deny check licenses bans sources` (from `src-tauri/`) -- expected: passes (unchanged).
- `bun run check:rust` -- expected: passes (no Rust changes).

**Manual checks:**
- Confirm `tools/keeper-rec/` sits outside `src-tauri/crates/` and `git status` shows no committed binary under `binaries/`.
- Confirm `docs/release.md` states the dev-signing (Apple Development certificate / Cap #1722) requirement as a DevEx note, not a product blocker.

## Auto Run Result

Status: done

**Summary:** Scaffolded the `keeper-rec` Swift capture sidecar and wired it end-to-end so every later recording story (16.2–16.6) spawns a real, signed binary instead of a stub. Added an Apache-2.0, zero-dependency SwiftPM package at `tools/keeper-rec/` (outside the Cargo workspace) whose stub answers one `getCapabilities` NDJSON line and exits cleanly; declared it as Tauri `bundle.externalBin`; kept every local/CI Tauri build working by building the sidecar first; and wired build + hardened-runtime codesign before `tauri build` in the release pipeline. No Rust changes (the `Recorder` port is Story 16.2).

**Files changed:**
- `tools/keeper-rec/Package.swift` (new) — SwiftPM manifest, executable target, `.macOS(.v13)`, no dependencies.
- `tools/keeper-rec/Sources/keeper-rec/main.swift` (new) — NDJSON stub: answers `getCapabilities` with `{"id":…,"result":{"protocolVersion":1,"macos":…}}`, exits 0 on EOF/malformed/unknown; ignores SIGPIPE and writes via the throwing `write(contentsOf:)`; SPDX + Cap #1722 comment.
- `tools/keeper-rec/README.md` (new) — purpose, build command, license.
- `scripts/build-keeper-rec.sh` (new) — `swift build -c release --arch arm64` + install to `binaries/keeper-rec-aarch64-apple-darwin`; Apple-Silicon host guard; runs the smoke check.
- `scripts/smoke-keeper-rec.sh` (new) — stdio-contract smoke test (JSON-parsed getCapabilities assertion; malformed/unknown-method/EOF all exit 0).
- `src-tauri/crates/keeper/keeper-rec.entitlements` (new) — minimal hardened-runtime plist (empty dict).
- `src-tauri/crates/keeper/tauri.conf.json` — added `bundle.externalBin: ["binaries/keeper-rec"]`.
- `package.json` — added `rec:build`; chained it into `tauri:dev`/`tauri:build`.
- `.gitignore` — ignore `src-tauri/crates/keeper/binaries/` and `tools/keeper-rec/.build/`.
- `.github/workflows/release.yml` — build + codesign `keeper-rec` before both the signed (`tauri-action`, hardened runtime + entitlements + Developer ID) and unsigned (ad-hoc) bundle steps.
- `docs/release.md` — "Recording sidecar (keeper-rec)" build note + Cap #1722 dev-signing DevEx note.

**Review findings breakdown:** 4 patches applied (all low: SIGPIPE/exit-0 hardening, JSON-parsed smoke assertion, Apple-Silicon host guard, `APPLE_SIGNING_IDENTITY` non-empty guard); 1 deferred (medium: validate signed-sidecar signature/entitlements survive tauri-action's bundle re-sign inside the notarized `.app`, and entitlements sufficiency for real capture — needs real certs + hardware, tracked in `deferred-work.md`); 5 rejected as noise for a scaffold stub (missing-`id` null semantics owned by 16.4, unbounded `readLine` for a trusted parent, CI Swift-toolchain coupling, README "not in diff", redundant `--show-bin-path`). No intent_gap or bad_spec; no spec loopback.

**Verification performed:**
- `bash scripts/build-keeper-rec.sh` → clean build, binary installed, all smoke checks pass (rc=0).
- `getCapabilities` → `{"id":7,"result":{"macos":"26.5.2","protocolVersion":1}}`, exit 0; malformed/unknown-method/EOF → exit 0, no garbage.
- Ad-hoc `codesign --options runtime` → signature present (`adhoc,runtime`).
- `cargo deny check licenses bans sources` → passes (no dependency added).
- `tauri.conf.json` + `package.json` valid JSON; `bun run lint` clean; `git status` shows no tracked file under `binaries/` (gitignore effective).

**Residual risks:** The full `bun run tauri:build -- --no-bundle` end-to-end bundle build was not run locally (slow); `externalBin` resolution is satisfied because `rec:build` produces the binary first, and the CI build job inherits the same chain. The signed/notarized release path is only exercised on a tag with real Apple secrets (deferred validation above). Real screen capture and its permission/entitlement needs land in Story 16.6 (human-in-the-loop on dev-signed hardware).
