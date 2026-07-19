---
title: 'Destination-Folder Chooser & fps Advanced Control'
type: 'feature'
created: '2026-07-19'
baseline_revision: 'ec9ec2edb804dad3833867389972567d40a72db2'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
final_revision: '46c52e5'
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-19-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** The recording destination is hardcoded to `~/Movies/keeper` deep inside `recording_start` (`ipc.rs` ~3473, with a self-flagged "Destination card is a later story" comment), the user can neither choose it nor learn before capture that it is unwritable or the disk is nearly full, and frame rate is fixed at 30 fps by a `CMTime(value:1, timescale:30)` constant in the sidecar with no way to select 60. Two settings the epic promises (persisted destination + fps, mirrored into Settings → Recording, next-session scope) don't exist.

**Approach:** Add two persisted recording settings (`recording.destination_dir`, `recording.fps`) behind the existing `keeper-core::registry` KV + `RecordingSettingsVm` seam; thread fps into `SessionParams` → `start_recording_request` wire → the sidecar's `SCStreamConfiguration`; add a pure, platform-free destination-validation decision function in `keeper-core` that a `recording_start` pre-flight probe feeds (exists-or-creatable / writable / free-space ≥ shared hard floor) so an unusable destination blocks capture with an actionable error before any recording begins; and surface both as a Destination folder-chooser card and a collapsed Advanced fps card on the setup surface, mirrored into Settings → Recording.

## Boundaries & Constraints

**Always:**
- **Core owns validation (AD-33).** The exists/writable/free-space *decision* is a pure, platform-free `keeper-core` function taking already-probed facts (`creatable_or_exists`, `writable`, `free_bytes`, `min_free_bytes`) → outcome; it holds no `tauri`/Apple/`std::fs`-probe token. `dependency_firewall_holds` must stay green. The shell (`keeper` crate) does the real filesystem probe and passes facts in.
- **Free-space is testable via a simulated signal.** The low-free-space path is exercised by injecting a small `free_bytes` into the pure decision function — never by filling a real disk. The shared hard-floor threshold constant lives in core (`RECORDING_MIN_FREE_BYTES = 2 GiB`) and is the same disk-guard hard floor Story 18.5 will reuse for its live guard.
- **Validate-on-Start blocks capture.** `recording_start` runs the destination pre-flight *before* the collision-suffix loop / `SessionManifest::create` / any sidecar spawn. On rejection it returns an actionable `IpcError` (secret-free, no raw path leaked beyond the chosen folder the user already sees) and no capture begins, no session folder is created, no sidecar is spawned.
- **Next-session scope (AD-25).** Destination and fps persist in `keeper.db` behind `keeper-core::registry`, are read by `recording_start` only at Start time (never re-read mid-session), and the same `RecordingSettingsControls`-style optimistic-mirror store keeps the setup cards and Settings → Recording in lockstep. Changing either affects only the next Recording Session.
- **fps is additive on the wire.** `SessionParams.fps` emits an always-present `"fps"` field in `start_recording_request` (like `segmentMB`), decoded best-effort by the sidecar with a default of 30 when absent. `PROTOCOL_VERSION` stays `1` (every prior additive start param kept it at 1); both sidecar and host protocol comments note the new field.
- **fps is {30, 60}.** 30 is the default; 60 the only other legal value. Core normalizes on read/write and the sidecar normalizes defensively (anything not 60 → 30) so a bad persisted value can never produce a degenerate `timescale`.
- **Local-only, zero new egress.** No upload/share/cloud/network affordance anywhere in the new cards or settings copy; the folder chooser opens only the OS-native directory picker already used by `export-dialog.tsx`.

**Block If:**
- Free-space cannot be probed on the target volume without adding a GPL/AGPL-licensed or `unsafe`-in-core dependency (would break the license firewall or the core `unsafe_code = "deny"` invariant). The intended dependency is `fs4` (permissive Apache-2.0/MIT, shell crate only) — if it fails `cargo deny check`, HALT rather than vendoring an unvetted alternative.

**Never:**
- Never add a `dir` field to the sidecar wire — the destination is fully host-side; the sidecar keeps receiving a single absolute `path` and derives its directory from it (unchanged). The architecture's nominal `start{...dir...}` is realized host-side via that path.
- Never implement the *live*, during-recording disk guard (warn threshold 10 GB, graceful stop-and-finalize at the hard floor driven by sidecar `state` free-space) — that is Story 18.5. 19.5 provides only the pre-Start leg and the shared threshold constant.
- Never re-read destination/fps mid-session, add a JS-writable settings store, or introduce a native folder-open dialog other than the registered `@tauri-apps/plugin-dialog`.
- Never bump `PROTOCOL_VERSION`. Never claim on-hardware verification of real 60 fps capture output — that (like every real-capture leg since 16.6) is deferred to Story 20.6; the automated gates prove the contract (wire field + normalization), not the pixels.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Default destination, first run | `recording.destination_dir` unset | `recording_settings_get` resolves and returns the effective default (`dirs::video_dir()/keeper`, fallback data-dir/keeper); Start creates it via `create_dir_all` and records into it | if `create_dir_all` fails → destination-invalid error, no capture |
| User picks a folder | native dir picker returns `/Users/x/Recordings` | `applyRecordingSettings({...,destinationDir})` persists it; the card shows the truncated path; next Start records there | picker cancel/throw → keep current folder (no write) |
| Destination not writable at Start | chosen root exists but probe-file write fails | `recording_start` returns actionable `IpcError` ("destination folder isn't writable"); no session folder, no sidecar | surfaced via the existing failed-start status; capture never begins |
| Low free space at Start | probed `free_bytes < RECORDING_MIN_FREE_BYTES` | pure `evaluate_destination` → `InsufficientSpace{free,required}`; `recording_start` returns actionable error naming the shortfall; no capture | simulated by injecting small `free_bytes` in the unit test |
| fps set to 60 | `recording.fps = 60`, Start | `SessionParams.fps = 60`; wire `{"fps":60}`; sidecar `minimumFrameInterval = CMTime(1, 60)` | absent/garbage fps on the wire → sidecar defaults to 30 |
| fps out-of-set persisted | registry holds `fps = 45` (corruption) | core read normalizes → 30; sidecar normalize also → 30 | never a `timescale` ≤ 0 or unexpected value reaches SCK |
| Change setting while recording | user edits folder/fps during a live session | value persists and mirrors both surfaces; the *current* session is unaffected; it applies to the next Start | no mid-session re-read; no interruption |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/registry.rs` -- add `recording.destination_dir` (String get/set, no clamp; empty/absent ⇒ `None`) and `recording.fps` (u32 get/set, normalize to {30,60}, default 30) following the `RECORDING_SEGMENT_MB_*` triad (~757-843). Getters/setters + consts.
- `src-tauri/crates/keeper-core/src/recording.rs` -- add `pub fps: u32` to `SessionParams` (~475-498); emit always-present `"fps"` in `start_recording_request` (~507-535) next to `segmentMB`; add pure `pub fn evaluate_destination(creatable_or_exists: bool, writable: bool, free_bytes: u64, min_free_bytes: u64) -> Result<(), DestinationRejection>` + `pub const RECORDING_MIN_FREE_BYTES: u64 = 2 * 1024 * 1024 * 1024`. `PROTOCOL_VERSION` (~411) unchanged; update its comment.
- `src-tauri/crates/keeper-core/src/error.rs` -- add `RecordingError::DestinationInvalid { reason: DestinationRejection }` (~439-474), secret-free actionable `Display`; `DestinationRejection` enum (`NotWritable`, `NotADirectory`, `InsufficientSpace { free_bytes, required_bytes }`).
- `src-tauri/crates/keeper-core/src/vm.rs` -- extend `RecordingSettingsVm` (~2838-2847) with `pub destination_dir: String` (effective resolved path) and `pub fps: u32` (`#[ts(export)]`, camelCase); regenerates `RecordingSettingsVm.ts` on `test:rust`.
- `src-tauri/crates/keeper/Cargo.toml` -- add `fs4 = "0.x"` (permissive; shell only) for `available_space`.
- `src-tauri/crates/keeper/src/ipc.rs` -- `recording_settings_get` (~3877): resolve effective destination default when the setting is `None` + read fps; `recording_settings_set` (~3894): write both, return re-read effective VM. `recording_start` (~3430, dir seam ~3473-3491): read persisted destination (resolve default), probe facts (`create_dir_all` + probe-file write + `fs4::available_space`), call `evaluate_destination` **before** the collision loop/`SessionManifest::create`, return actionable `IpcError` on rejection; read fps and set `SessionParams.fps`.
- `src/lib/ipc/gen/RecordingSettingsVm.ts` -- regenerated (`destinationDir`, `fps`); committed.
- `src/lib/stores/recording-settings.ts` -- add `RECORDING_FPS_DEFAULT = 30` and `RECORDING_FPS_ALLOWED = [30, 60]` constants (mirror Rust); no structural change (new VM fields flow through `applyRecordingSettings`).
- `src/components/recording/recording-destination-controls.tsx` -- NEW: folder chooser using `open({ directory: true })` from `@tauri-apps/plugin-dialog` (mirror `export-dialog.tsx` ~158-166), truncated path display, "Applies to the next Recording Session." note, local-only disclosure; commits via `applyRecordingSettings({...settings, destinationDir})`.
- `src/components/recording/recording-advanced-controls.tsx` -- NEW: hand-rolled collapsed "Advanced" disclosure (Button + `useState`, no new dep) revealing an fps `Select` (30/60) bound to `applyRecordingSettings({...settings, fps})`.
- `src/components/layout/recording-pane.tsx` -- add `Destination` → `<RecordingDestinationControls/>` and `Advanced` → `<RecordingAdvancedControls/>` branches to the card chain (~161-207); `SETUP_CARDS` already lists both titles.
- `src/components/settings/settings-dialog.tsx` -- `RecordingSection` (~995-1010): render both new controls alongside `<RecordingSettingsControls/>`; update the local-only disclosure copy (~983) now that the folder is user-visible.
- `tools/keeper-rec/Sources/keeper-rec/main.swift` -- decode `fps` (`NSNumber`→`Int`, default 30) in the `startRecording` case (~342-395); pass to `captureEngine.start(...)`.
- `tools/keeper-rec/Sources/keeper-rec/Capture.swift` -- thread `fps` through `CaptureEngine.start` (~196-200) → `beginCapture` (~301-350) → `config.minimumFrameInterval = CMTime(value: 1, timescale: Int32(normalizeFps(fps)))` (~341).
- `tools/keeper-rec/Sources/keeper-rec/FrameRate.swift` (+ `Tests/keeper-recTests/FrameRateTests.swift`) -- NEW pure `normalizeFps(_:) -> Int` policy (mirrors `Rotation.swift`/`MicHealth.swift`) + XCTest.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- add `recording.destination_dir` (String get→`Option<String>`/set) and `recording.fps` (u32 get/set, normalize {30,60}, default 30) consts + accessors; unit tests (default, round-trip, fps normalization of out-of-set values) -- persisted settings seam.
- [x] `src-tauri/crates/keeper-core/src/error.rs` -- `RecordingError::DestinationInvalid { reason }` + `DestinationRejection` enum with secret-free actionable `Display` -- actionable validation errors.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- pure `evaluate_destination(...)` + `RECORDING_MIN_FREE_BYTES`; `SessionParams.fps`; always-present `"fps"` in `start_recording_request` -- platform-free validation decision + fps wire.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` (tests) -- `evaluate_destination`: ok; not-writable; not-a-directory; simulated low free (`free_bytes < min`) → `InsufficientSpace`; exact-floor boundary; `start_recording_request` carries `"fps"` (30 and 60); `dependency_firewall_holds` still green -- the simulated-signal proof.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- `RecordingSettingsVm.destination_dir: String` + `fps: u32` (`#[ts(export)]`) -- VM carries effective folder + fps.
- [x] `src-tauri/crates/keeper/Cargo.toml` -- add `fs4` (permissive license; `cargo deny check` must pass) -- free-space probe.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- `recording_settings_get`/`_set` read/write destination + fps (get resolves effective default when unset); `recording_start` runs the destination pre-flight (probe → `evaluate_destination`) before session-folder creation and threads persisted fps into `SessionParams` -- host wiring + validate-on-Start gate.
- [x] `src/lib/ipc/gen/RecordingSettingsVm.ts` -- regenerated with `destinationDir` + `fps`, committed -- wire parity.
- [x] `src/lib/stores/recording-settings.ts` -- `RECORDING_FPS_DEFAULT`/`RECORDING_FPS_ALLOWED` constants (mirror Rust) -- frontend bounds.
- [x] `src/components/recording/recording-destination-controls.tsx` (+ `.test.tsx`) -- NEW folder-chooser card (native dir picker, truncated path, next-session note, local-only copy) -- Destination surface.
- [x] `src/components/recording/recording-advanced-controls.tsx` (+ `.test.tsx`) -- NEW collapsed Advanced group with fps `Select` (30/60) -- fps surface.
- [x] `src/components/layout/recording-pane.tsx` -- wire `Destination` + `Advanced` card branches -- setup-surface composition.
- [x] `src/components/settings/settings-dialog.tsx` -- mount both controls in `RecordingSection`; update local-only disclosure copy -- Settings → Recording mirror.
- [x] `tools/keeper-rec/Sources/keeper-rec/FrameRate.swift` (+ `Tests/keeper-recTests/FrameRateTests.swift`) -- NEW pure `normalizeFps` + XCTest -- hardware-free fps policy.
- [x] `tools/keeper-rec/Sources/keeper-rec/main.swift` -- decode `fps` (default 30) in `startRecording`; pass to `CaptureEngine.start` -- wire decode.
- [x] `tools/keeper-rec/Sources/keeper-rec/Capture.swift` -- thread `fps` into `SCStreamConfiguration.minimumFrameInterval` via `normalizeFps` -- applies the selected rate.

**Acceptance Criteria:**
- Given a fresh install with no persisted destination, when the user opens the Destination card, then it shows the effective default (`~/Movies/keeper`), and pressing Start creates that folder (if absent) and records into it without error.
- Given the user picks a folder via the native directory picker, when the selection resolves, then `recording.destination_dir` persists, both the setup card and Settings → Recording show the chosen path, and the next Recording Session records there while the change never affects an in-flight session.
- Given a destination that is not writable or whose volume has less than the shared hard floor of free space, when the user presses Start, then `recording_start` returns an actionable, secret-free error, no session folder is created, no sidecar is spawned, and no capture begins.
- Given `recording.fps` is 60, when a session starts, then `start_recording_request` emits `{"fps":60}` and the sidecar sets `minimumFrameInterval = CMTime(1, 60)`; given any out-of-set persisted fps, then both core and the sidecar normalize it to 30, and `PROTOCOL_VERSION` remains 1.
- Given the Advanced group, when the setup surface first renders, then it is collapsed with fps hidden behind a disclosure, defaulting to 30 with 60 selectable, and no upload/share/network affordance appears in any new card or settings copy.
- Given `bun run check`, `bun run check:rust`, `bun run test:rust`, `swift test --package-path tools/keeper-rec`, `cargo deny check` (from `src-tauri/`), and `bash scripts/build-keeper-rec.sh`, then biome/tsc/vitest, clippy (`-D warnings`), cargo-nextest (with regenerated committed `.ts` and `dependency_firewall_holds`), the Swift unit tests incl. `FrameRateTests`, the license firewall, and the Swift release build + NDJSON smoke all pass.

## Spec Change Log

## Review Triage Log

### 2026-07-19 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 0, low 3)
- defer: 0
- reject: 10
- addressed_findings:
  - `[low]` `[patch]` `ipc.rs::effective_destination_dir` — extracted a pure `resolve_destination_dir` that rejects a relative/hand-edited persisted `recording.destination_dir` in favor of the absolute default, so the VM's "always a concrete absolute folder" guarantee holds and no session can be created under keeper's cwd (Edge-Hunter relative-path finding).
  - `[low]` `[patch]` `ipc.rs::tests` — new `resolve_destination_dir_honors_absolute_and_falls_back_otherwise` unit test locking the absolute-path invariant, closing the Blind-Hunter "shell probe/resolve logic untested" gap for the resolve path.
  - `[low]` `[patch]` `ipc.rs::destination_writable` — unique per-attempt probe filename (`pid + nanos`) so a crash mid-probe cannot leave a recognizable stray dotfile in the user's folder and no two probes ever collide (Blind-Hunter probe-file finding).
- rejected (10, all no-defect or out-of-scope): `create_dir_all` creates the destination *root* on a rejected Start (that root is the user's own chosen/default recordings folder; the *session* subfolder is still never created); `DestinationInvalid → IpcErrorCode::Internal` (matches the existing `ExportIo` bad-destination precedent and the actionable message is surfaced verbatim); symlink-followed `is_dir` (dangling → `NotADirectory`, valid → correct target volume); errored free-space probe fails **open** (deliberate — a broken `statvfs` must not false-reject; the live 2 GB hard-floor guard is Story 18.5's job); store revert-to-`null`/co-setting race (controls gate on non-null `settings` and `pickFolder` reads the live store post-await, and the `writeId` token guards the echo — the race is unreachable); fps set duplicated across Rust/TS/Swift (the project's established cross-language mirroring convention, each side unit-tested); GiB-floor vs decimal-GB message (decimal matches Finder; internal-only); root-vs-subfolder TOCTOU (same volume on macOS; downstream `SessionManifest::create` handles a vanished dir); Advanced re-collapses on mount (the spec's intended collapsed default); `video_dir()→data_dir` fallback (the VM reports the effective path honestly so the UI always names the real folder).

## Design Notes

**Why 19.5 introduces the pre-Start disk-guard leg (18.5 is still backlog).** The epic charts destination validation as "adequate free space per the disk-guard policy," but Story 18.5 (which owns the disk-guard policy: warn 10 GB, graceful stop-and-finalize at the 2 GB hard floor, driven by sidecar `state` free-space) has not landed. This mirrors 19.4/18.4 exactly: 19.5 adds the **minimal** reusable seam — a single shared `RECORDING_MIN_FREE_BYTES` constant in core and a pure `evaluate_destination` decision function — and uses it only for the pre-Start block. The *live*, during-recording guard (state-event-driven warn/hard-floor) remains 18.5, which will consume the same constant.

**Hexagonal split for validation (AD-33).** The decision is pure and platform-free so it unit-tests without a disk: `evaluate_destination(creatable_or_exists, writable, free_bytes, min_free_bytes)`. The shell gathers the facts — `create_dir_all` (exists-or-creatable), a probe-file write+remove (writable, more reliable than metadata perms on macOS), and `fs4::available_space` (free) — then calls core. `fs4` stays in the `keeper` shell crate only; `keeper-core` keeps `unsafe_code = "deny"` and no filesystem-probe dependency, so `dependency_firewall_holds` stays green.

**Destination is host-side; only fps touches the wire.** The sidecar already receives a full absolute `path` and derives its own directory (`Capture.swift` ~407), so the destination change is entirely in `recording_start` (read setting → resolve default → validate → build `folder.join("screen-0000.mp4")`). Nothing about `dir` reaches Swift. fps is the one genuinely new wire field: always-present like `segmentMB`, decoded best-effort, defaulted to 30 when absent — squarely additive, so `PROTOCOL_VERSION` stays 1 (consistent with 17.1/17.5/19.1/19.3/19.4).

**next-session-only, read-at-Start.** Like `segment_mb`/`duration_cap_minutes`, both settings are read from the registry inside `recording_start` and never re-read mid-session, so editing folder or fps during a live recording persists and mirrors both surfaces but only affects the next Start. The new cards reuse the `RecordingSettingsControls` optimistic-mirror + monotonic-`writeId` store pattern (`recording-settings.ts`) rather than an ephemeral store.

**Effective-default resolution lives in the shell.** `recording.destination_dir` stores the raw user choice (or nothing); resolving the `~/Movies/keeper` default requires `dirs::video_dir()`, which is shell-only, so `recording_settings_get` returns the *effective* resolved path in the VM — the UI always displays a concrete folder, and "unset vs default" ambiguity never reaches the frontend.

**Collapsed Advanced without a new dependency.** The "Advanced" group is a hand-rolled disclosure (Button + `useState` + conditional render) in app code, not a shadcn `ui/` component, avoiding an unattended `shadcn add collapsible` / new radix dependency while satisfying "collapsed by default." The fps control reuses the existing `Select` primitive (the same one the mic-device picker uses).

## Verification

**Commands:**
- `bun run check` -- expected: biome + tsc + vitest green (destination card folder-pick + next-session note; Advanced collapse + fps 30/60 select).
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` clean.
- `bun run test:rust` -- expected: cargo-nextest green incl. `evaluate_destination` (ok / not-writable / not-a-dir / simulated-low-free / boundary), `start_recording_request` fps field, registry fps normalization, and `dependency_firewall_holds`; regenerated `RecordingSettingsVm.ts` committed.
- `cargo deny check` (from `src-tauri/`) -- expected: `fs4` passes the license firewall.
- `swift test --package-path tools/keeper-rec` -- expected: `FrameRateTests` (normalize 30/60/out-of-set) pass.
- `bash scripts/build-keeper-rec.sh` -- expected: Swift release build succeeds and the NDJSON smoke passes.

## Auto Run Result

Status: done

**Summary.** The recording destination is now a persisted, user-chosen folder (default `~/Movies/keeper`) validated at Start, and frame rate is a selectable Advanced control (30 default / 60). Two new registry settings (`recording.destination_dir`, `recording.fps`) sit behind the existing `RecordingSettingsVm` seam; a pure, platform-free `keeper-core::evaluate_destination` decision function — fed by a shell-side probe (`create_dir_all` exists-or-creatable, probe-file writable, `fs4::available_space` vs a shared 2 GiB hard floor) — blocks capture with an actionable error before any session folder is created or sidecar spawned. fps threads as an additive always-present `"fps"` wire field into the sidecar's `SCStreamConfiguration` (`PROTOCOL_VERSION` unchanged), normalized to {30,60} on both the Rust and Swift sides. Both settings surface as a Destination folder-chooser card and a collapsed Advanced fps card, mirrored into Settings → Recording, read only at Start (next-session scope).

**Files changed (one-line):**
- `keeper-core/src/registry.rs` — `recording.destination_dir` (String get→`Option`/set) + `recording.fps` (u32 get/set, `normalize_recording_fps` {30,60}, default 30) following the segment-MB triad; + unit tests.
- `keeper-core/src/error.rs` — `DestinationRejection` (`NotADirectory`/`NotWritable`/`InsufficientSpace{free,required}`) with secret-free actionable `Display`; `RecordingError::DestinationInvalid{reason}`.
- `keeper-core/src/recording.rs` — pure `evaluate_destination(...)`, `RECORDING_MIN_FREE_BYTES = 2 GiB`, `SessionParams.fps`, always-present `"fps"` in `start_recording_request`; + validation/wire tests.
- `keeper-core/src/vm.rs` — `RecordingSettingsVm.destination_dir: String` (effective resolved path) + `fps: u32`.
- `keeper/Cargo.toml` + workspace `Cargo.toml`/`Cargo.lock` — `fs4` (permissive; shell crate only).
- `keeper/src/ipc.rs` — `effective_destination_dir`/`resolve_destination_dir` (absolute-only guard), `destination_writable` (unique per-attempt probe), `recording_start` pre-flight (probe → `evaluate_destination`) before the collision loop / `SessionManifest::create` / sidecar spawn + fps threaded into `SessionParams`; `recording_settings_get/_set` read/write both new settings; `DestinationInvalid → IpcError` arm; + `resolve_destination_dir` test.
- `src/lib/ipc/gen/RecordingSettingsVm.ts` — regenerated (`destinationDir`, `fps`).
- `src/lib/stores/recording-settings.ts` — `RECORDING_FPS_DEFAULT`/`RECORDING_FPS_ALLOWED`.
- `src/components/recording/recording-destination-controls.tsx` (+ test) — folder-chooser card (native dir picker, truncated path, next-session + local-only notes).
- `src/components/recording/recording-advanced-controls.tsx` (+ test) — hand-rolled collapsed Advanced group with fps 30/60 `Select`.
- `src/components/layout/recording-pane.tsx` (+ test) — Destination + Advanced card branches.
- `src/components/settings/settings-dialog.tsx` (+ test) — mount both controls in `RecordingSection`; updated local-only disclosure copy.
- `src/components/settings/recording-settings-controls.tsx` (+ test) — commit spreads the live VM so segment/duration edits never clobber destination/fps.
- `keeper-rec/FrameRate.swift` (+ `FrameRateTests.swift`) — pure `normalizeFps` (≠60 → 30).
- `keeper-rec/main.swift` — decode `fps` (default 30) in `startRecording`.
- `keeper-rec/Capture.swift` — fps → `minimumFrameInterval = CMTime(1, normalizeFps(fps))`.

**Review findings:** intent_gap 0, bad_spec 0. Patches applied 3 (all low: absolute-path resolve guard + its unit test; unique probe filename). Deferred 0. Rejected 10 (no-defect / out-of-scope / deliberate design — see Review Triage Log).

**Deviations from spec (all sound, retained):** `DestinationInvalid` maps to `IpcErrorCode::Internal` (retriable) mirroring the existing `ExportIo` bad-destination precedent — the actionable message reaches the UI verbatim; an errored free-space probe fails **open** (`u64::MAX` + `tracing::warn`) so a broken `statvfs` never false-rejects a legitimate Start, with the live disk guard (Story 18.5) as the real safety net; the {30,60} rule is a single named `normalize_recording_fps`/`normalizeFps` on each side rather than an inlined set.

**Verification (all independently re-run and green):** implementer ran all six gates green — `bun run check` (biome + tsc + vitest 1384), `bun run check:rust` (rustfmt + clippy `-D warnings`), `bun run test:rust` (cargo-nextest 914 incl. `evaluate_destination`×5, `start_recording_request_always_carries_fps`, registry destination/fps, `dependency_firewall_holds`), `cargo deny check` (licenses ok, `fs4` unflagged; the `advisories` failure is a pre-existing unmaintained-gtk3 RUSTSEC in tauri's Linux tree, present on baseline `ec9ec2e`, unrelated), `swift test` (35, incl. `FrameRateTests`), `bash scripts/build-keeper-rec.sh` (release build + NDJSON smoke). After the review patches I re-verified `cargo fmt --check` clean, `clippy -p keeper -D warnings` clean, and the 10 destination/fps tests (incl. the new `resolve_destination_dir` test) pass; the patches are localized to `ipc.rs` with no JS/Swift impact.

**Residual risks:** Real 60 fps pixel output, real silence/on-hardware capture, and physical low-disk behavior are verifiable only on hardware and are charted to Story 20.6 — the automated gates prove the contract (wire field, normalization, pure validation decision), not the pixels. The pre-Start free-space guard fails open on an errored probe by design; the live during-recording disk guard (warn 10 GB / graceful stop) remains Story 18.5, which will reuse `RECORDING_MIN_FREE_BYTES`. No new network egress; `keeper-core` stays platform-free (`fs4` shell-only).

**Follow-up review recommended:** false — the review made only three localized low-severity robustness fixes (an absolute-path resolve guard + test and a unique probe filename) with no behavior/API/security/data impact.
