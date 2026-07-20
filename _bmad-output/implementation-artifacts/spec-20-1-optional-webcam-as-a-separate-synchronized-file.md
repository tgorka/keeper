---
title: 'Story 20.1: Optional Webcam as a Separate Synchronized File'
type: 'feature'
created: '2026-07-19'
status: 'done'
baseline_revision: '6abbe967c09f3c292c42050e555bcb986cd7e93e'
final_revision: '6bae089'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-20-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Recording captures screen + audio only. Users who want a talking-head track must burn the webcam into the screen video (destructive, no way to remove it later). FR-70 requires the webcam as its own separately-synchronized file.

**Approach:** Add an optional webcam source that records `camera-####.mp4` into the same session folder from a **second in-sidecar `AVAssetWriter`**, host-clock anchored and rotated at the **same segment boundaries** as `screen-####`, so any two same-index segments stay aligned within one video frame. Mirror the mic-as-separate-track wiring (Story 19.3) end-to-end: ephemeral device selection, lazy Camera-TCC request on enable, non-fatal camera-loss warning. No PiP, no self-view, no burn-in.

## Boundaries & Constraints

**Always:**
- Webcam defaults **off**. When off: no `camera-####` files, no Camera-TCC request, `SessionDevices.camera == false`, no `AVCaptureSession` for video.
- Camera records to a **separate file** `camera-####.mp4` from a dedicated `AVAssetWriter`/`AVCaptureSession` — never a track inside `screen-####`, never premixed, never composited (no PiP/self-view).
- Camera writer is **host-clock anchored** (`startSession(atSourceTime:)` on its first host-clock PTS) and rotates at the **screen's** keyframe boundary PTS (screen is the master rotation clock; the camera never rotates on its own byte budget).
- Each camera segment records `track:"camera"` plus `ptsStart`/`ptsEnd` host-clock bounds in the ledger/manifest, exactly like screen.
- Camera loss mid-recording is **non-fatal**: screen recording continues, the camera writer finalizes its current file, and a sticky `warning` (`code:"cameraLost"`) is raised. Never `error`, never abort.
- Camera-TCC is requested via the system prompt **only on enable** of the Webcam switch (never preemptively). Camera denial does **not** block Start (mic precedent).
- Recording stays macOS ≥ 13.0 desktop-only behind `CapabilitiesVm.recording`; iOS never records. NDJSON-RPC additions stay additive → `PROTOCOL_VERSION` remains `1`.
- Zero new network destinations (FR-76). `NSCameraUsageDescription` present in keeper's `Info.plist`, honest local-only framing.

**Block If:**
- Adding webcam would require a new network egress, a non-permissive/AGPL dependency, or `ffmpeg` (system AVFoundation only).
- The `PROTOCOL_VERSION` must change to make camera work (it must not — additions are additive).

**Never:**
- No picture-in-picture, no self-view bubble, no burning the camera into the screen video (copy may note macOS 14+ system presenter-overlay compositing is an OS behavior, not a keeper feature — UX-DR34).
- No persisting camera device selection to settings (ephemeral per-session, like mic).
- No device-class VM field / grouped picker — a flat `name` list (the `localizedName` already distinguishes built-in / external / Continuity Camera).
- No blocking Start on Camera permission; do not extend `RecordingPermissionVm.canStart`.
- No real-hardware capture validation here — end-to-end capture on a Development-signed Mac is SM-9/SM-10 (Story 20.6). This story ships code + unit/fake-sidecar/harness tests only.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Webcam off (default) | `startRecording` with no `cameraEnabled` | No `cameraEnabled`/`cameraDeviceId` on wire; `SessionDevices.camera=false`; no camera file/session/TCC | No error |
| Webcam on, default device | `camera_enabled=true`, `camera_device_id=None` | `CameraSelection{device_id:None}`; sidecar picks `AVCaptureDevice.default(.video)`; `camera-0000.mp4` written; manifest `camera:true` | No error |
| Webcam on, specific device | `camera_device_id=Some("uid")` | Sidecar binds `AVCaptureDevice(uniqueID:)`; falls back to default if uid absent | Fallback to default, no abort |
| Segment rotation | Screen byte/duration cap hit at keyframe | Both `screen-####` and `camera-####` cut at the same keyframe PTS; each emits `segmentClosed{track,...}` | `rotationInFlight` guard prevents overlap |
| Camera enumeration | `listSources` called | `cameras: [{id,name}]` from `.video` DiscoverySession (built-in/external/Continuity/DeskView) | Empty list if none; no error |
| Camera hot-unplug | Device disconnect / `simulateCameraRemoval` | `warning code:"cameraLost"`; screen continues; camera writer finalizes current file | Non-fatal; never `error`/abort |
| Camera permission denied | User denies on enable | Inline denied caption; screen + other sources still record; Start not blocked | Non-blocking |
| Clean stop with camera on | `stop` | Camera final segment finalized (no illegal `segmentClosed` during Stopping); session reaches `finalized` | Kill-timeout guard unchanged |
| Reconcile screen+camera folder | Recovery over folder with both prefixes | Both tracks ingested into ledger, disambiguated by `track`; `screen-0000`/`camera-0000` indices do not clobber | Bounds preserved per (track,index) |

</intent-contract>

## Code Map

**Swift sidecar — `tools/keeper-rec/Sources/keeper-rec/`**
- `main.swift` — NDJSON-RPC: `capabilitiesResult()` (`camera:true`), `permissionsPayload()` (real `.video`), `sourcesResult()` (`listCameras()`), new `requestCamera`, `startRecording` decode (`cameraEnabled`/`cameraDeviceId`), optional `simulateCameraRemoval`.
- `Capture.swift` — engine: second `SegmentWriter` for camera (video-only), `AVCaptureSession`+`AVCaptureVideoDataOutput` (mirror `startMicCaptureSession`), camera rotation in lockstep inside `rotate(...)`, finalize in `finishAndExit`/`stop`, `handleCameraLost` + disconnect observers.
- `CameraHealth.swift` — **new**, pure Foundation-only policy mirroring `MicHealth.swift` (`decide(...) -> Decision`).
- `Rotation.swift` — `nextSegmentPath` already increments any stem; no change (camera path derives from host-supplied basename).

**Rust core — `src-tauri/crates/keeper-core/src/`**
- `recording.rs` — `CameraSelection` struct; `SessionParams.camera`; `start_recording_request` emits `cameraEnabled`/`cameraDeviceId`; `request_camera_request`/`parse_request_camera_result`; `Recorder::request_camera`; generalize `SEGMENT_STEM_PREFIX`/`reconcile_from_dir` for `camera-`; fix `known_bounds` key to `(track,index)`. `SessionDevices.camera` + `SegmentEntry.track` already exist.
- `vm.rs` — `RecordingSourcesVm.cameras`, `RecordingDeviceVm`, `RecordingFeaturesVm.camera` already declared (no shape change).

**Rust shell — `src-tauri/crates/keeper/src/`**
- `recorder.rs` — `REQUEST_CAMERA_REQUEST_ID`, `fetch_request_camera` (reuse 120s prompt timeout), implement `request_camera` on `DesktopRecorder` + `IosRecorder`.
- `ipc.rs` — `recording_start` camera args → `CameraSelection`; `request_camera_permission` command; live-sink basename branch on `track` (`camera-{index:04}.mp4`).
- `lib.rs` — register `request_camera_permission` in the invoke handler.
- `Info.plist` — add `NSCameraUsageDescription`.

**Frontend — `src/`**
- `lib/stores/recording-webcam.ts` — **new** ephemeral store (mirror `recording-mic.ts`).
- `components/recording/recording-webcam-controls.tsx` — **new** Switch + camera Select (mirror `recording-audio-controls.tsx` mic row).
- `components/layout/recording-pane.tsx` — add `title==="Webcam"` branch; append camera getters to `start(...)` at both call sites.
- `hooks/use-recording-session.ts` — `start` gains `cameraEnabled?`/`cameraDeviceId?`.
- `lib/ipc/client.ts` — `recordingStart` camera args + payload; `requestCameraPermission()` wrapper.

## Tasks & Acceptance

**Execution:**
- [x] `tools/keeper-rec/Sources/keeper-rec/main.swift` -- flip `features.camera` true, wire `permissionsPayload` to real `.video`, add `listCameras()` (`.video` DiscoverySession incl. `.builtInWideAngleCamera`/`.external`/`.continuityCamera`/`.deskViewCamera`, macOS-14 type-name split) into `sourcesResult().cameras`, add `requestCamera` method, decode `cameraEnabled`/`cameraDeviceId` in `startRecording`, add optional `simulateCameraRemoval` -- expose camera capability, devices, permission, and start params on the wire.
- [x] `tools/keeper-rec/Sources/keeper-rec/CameraHealth.swift` (new) + `Capture.swift` -- add a pure `CameraHealth` policy (mirror `MicHealth`); stand up a camera `AVCaptureSession`+`AVCaptureVideoDataOutput` on `mediaQueue`, a second video-only `SegmentWriter` writing `camera-####.mp4` host-clock anchored, rotate it at the screen keyframe PTS inside `rotate(...)`, emit `segmentClosed{track:"camera",ptsStart,ptsEnd}`, finalize in `finishAndExit`/`stop`, and route disconnect/`simulateCameraRemoval` through `handleCameraLost` → non-fatal `warning code:"cameraLost"` -- second synchronized file that never aborts the screen recording.
- [x] `tools/keeper-rec/Tests/keeper-recTests/CameraHealthTests.swift` (new) + extend `ConcatAssert`/`FixtureSegments` -- unit-test `CameraHealth.decide` (unrelated-device ignore, unknown-identity conservative warn, fallback), and extend the NFR-22 concat harness to generate `camera-####` fixtures and assert screen↔camera same-index boundaries align within one video frame (populates the Story 17.4 alignment hook) -- prove lockstep alignment without hardware capture.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- add `CameraSelection { device_id: Option<String> }` and `SessionParams.camera`; emit `cameraEnabled`(+`cameraDeviceId`) in `start_recording_request` (omitted when off, byte-compatible wire); add `request_camera_request`/`parse_request_camera_result` and the `Recorder::request_camera` trait method; generalize `reconcile_from_dir` to ingest `camera-` alongside `screen-` into one `segments` vec disambiguated by `track`, and re-key `known_bounds` on `(track, index)` so `screen-0000`/`camera-0000` never clobber; set `SessionDevices.camera` -- core state, manifest, and recovery understand the second track.
- [x] `src-tauri/crates/keeper/src/recorder.rs` -- add `REQUEST_CAMERA_REQUEST_ID`, `fetch_request_camera` (reuse the 120s human-prompt timeout), and implement `request_camera` on `DesktopRecorder` (round-trips the sidecar) and `IosRecorder` (returns `Unsupported`/`NotDetermined`) -- camera TCC pre-flight over the port.
- [x] `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- `recording_start` accepts `camera_enabled`/`camera_device_id` → `CameraSelection` on `SessionParams` and `SessionDevices.camera`; add `request_camera_permission` command and register it; branch the live-sink basename on the event `track` for `camera-{index:04}.mp4` -- IPC threads camera end-to-end.
- [x] `src-tauri/crates/keeper/Info.plist` -- add `NSCameraUsageDescription` with honest local-only framing (mirror the mic string) -- OS prompt copy present when camera TCC is requested.
- [x] `src/lib/stores/recording-webcam.ts` (new) + test -- ephemeral zustand store: `webcamEnabled:boolean=false`, `cameraDeviceId:string|null=null`, `useWebcamEnabled`/`useCameraDeviceId` hooks, imperative `webcamEnabled()`/`cameraDeviceId()`, setters, `isCameraSelectionAvailable(id, sources)` against `sources.cameras`, `resetRecordingWebcamForTest()` -- session-scoped camera choice (mirror `recording-mic.ts`).
- [x] `src/components/recording/recording-webcam-controls.tsx` (new) + test -- `Switch` (default off, reveals a camera `Select`: "System default camera" sentinel + `sources.cameras`), lazy `requestCameraPermission()` on enable with a generation-seq guard, granted/denied inline captions, `active` prop + pre-Start reconciliation `useEffect`; export label/testid constants; render presenter-overlay + "records to a separate file, synced to the screen" copy, no self-view/PiP -- the Webcam card (mirror `recording-audio-controls.tsx`).
- [x] `src/components/layout/recording-pane.tsx` -- add the `title === "Webcam"` branch rendering `<RecordingWebcamControls active={!live} />`; append `webcamEnabled()`, `cameraDeviceId()` to the `start(...)` calls at the Start `onClick` and the banner Restart -- wire the card into the pane and the start path.
- [x] `src/hooks/use-recording-session.ts` + `src/lib/ipc/client.ts` -- extend `start` with `cameraEnabled?`/`cameraDeviceId?` passed to `recordingStart`; add `cameraEnabled`/`cameraDeviceId` args + invoke payload to `recordingStart`; add `requestCameraPermission()` wrapping `invoke<TccPermission>("request_camera_permission")` -- frontend→backend camera plumbing.

**Acceptance Criteria:**
- Given the Webcam card renders, when the user has not toggled it, then the switch is **off**, no camera picker/permission is shown, and starting a recording produces no `camera-####` files and issues no Camera-TCC request (FR-70, AD-36).
- Given the webcam switch is enabled, when recording runs, then a `camera-####.mp4` is written from a second in-sidecar `AVAssetWriter` in the same session folder, host-clock anchored and rotated at the same segment boundaries as `screen-####`, with each camera segment carrying `track:"camera"` and PTS bounds in the manifest so same-index segments stay aligned within one video frame (FR-70, AD-37, NFR-22).
- Given an enabled webcam, when the camera is lost mid-recording, then the screen recording continues (never aborts), a sticky `cameraLost` warning surfaces in the active-recording banner, the camera file finalizes cleanly, and there is no PiP burn-in or self-view (FR-70, UX-DR34, FR-75).
- Given the capability flag, when recording surfaces render, then the Webcam card appears only behind `CapabilitiesVm.recording` (macOS ≥ 13.0, never iOS) and the NDJSON `PROTOCOL_VERSION` remains `1` (additive only) (AD-35).
- Given a folder holding both `screen-####` and `camera-####` files, when recovery reconciles it, then both tracks are ingested into the ledger disambiguated by `track` with no index collision or lost PTS bounds (FR-73).

## Design Notes

**Why a second writer, not a second track:** the mic (19.3) rides one `AVAssetWriter` as an extra audio track inside `screen-####`. The story title mandates a **separate file**, so the camera needs its own `AVCaptureSession` + video-only `SegmentWriter`. Synchronization is not shared-container: it's shared host-clock. Both `SCStream` and `AVCaptureSession` sample buffers carry `CMClockGetHostTimeClock` PTS, so anchoring each file with `startSession(atSourceTime:firstHostPTS)` and cutting the camera at the screen's rotation `keyframePTS` keeps the two on one timeline. Record `ptsStart`/`ptsEnd` **before** `startSession` rebases each file to 0 — that's the existing NFR-22 machinery the concat harness asserts against.

**Master/reactive rotation:** screen stays the rotation master (byte/duration budget on `onDiskBytes`). When screen `rotate()` fires, the camera writer is cut at the same `keyframePTS` in the same critical section — camera never runs its own `RotationPolicy`. This guarantees same-index files share a boundary.

**Camera loss has no silence-fill analog:** unlike mic (which silence-fills its audio track), a lost camera simply stops appending and finalizes its current `camera-####.mp4` (which ends early). Alignment is anchored at segment start, so a short camera file is still frame-aligned from its start. Continuity Camera drops (phone locks/moves) are the common case, so the observer + `cameraLost` path matters here.

**Ledger dual-track (the one real core hazard):** `reconcile_from_dir`'s `known_bounds: HashMap<u32,…>` keys on index alone; `screen-0000` and `camera-0000` would collide. Re-key on `(track, index)` (or `(String, u32)`) and generalize the `screen-`-only prefix filter to accept both prefixes, keeping one `segments: Vec<SegmentEntry>` disambiguated by `track`.

**Automation boundary:** macOS 15+ rejects ad-hoc-signed ScreenCaptureKit (Cap #1722), so real end-to-end camera capture is deferred to the Development-signed SM-9/SM-10 acceptance (Story 20.6), exactly as mic (19.3) was. This story is fully implementable with compile gates, pure-policy unit tests (`CameraHealth`), a fake-sidecar `request_camera` round-trip, the dual-track reconcile tests, the extended concat-assert alignment harness, and frontend store/component tests.

## Verification

**Commands:**
- `bun run rec:build` -- expected: `keeper-rec` compiles with the camera writer/session and `CameraHealth`.
- `bun run rec:test` -- expected: Swift tests pass, incl. `CameraHealthTests` and the extended screen↔camera concat-assert alignment.
- `bun run check:rust` -- expected: `cargo fmt --check` clean + `clippy --all-targets -- -D warnings` clean (no `.unwrap()` in production paths).
- `bun run test:rust` -- expected: keeper-core recording tests pass, incl. dual-track reconcile and the `request_camera` fake-sidecar round-trip.
- `bun run check` -- expected: biome + tsc + vitest green, incl. `recording-webcam` store and `recording-webcam-controls` tests.

**Manual checks (if no CLI):**
- Inspect `src/lib/ipc/gen/RecordingFeaturesVm.ts` regenerates with `camera: boolean` still present and `RecordingSourcesVm.cameras` populated end-to-end (ts-rs artifacts, not hand-edited).

## Review Triage Log

### 2026-07-19 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 1, medium 1, low 1)
- defer: 1: (medium 1)
- reject: 13
- addressed_findings:
  - `[high]` `[patch]` Camera segment `ptsStart` was recorded as the camera's own first appended frame, not the shared screen anchor/boundary — a webcam warms up 1–3 s after the screen anchors, so segment 0 (and slower rotated segments) failed the FR-70/NFR-22 one-frame alignment gate on real hardware (fixtures passed only by hand-authored matching bounds). Fixed in `tools/keeper-rec/Sources/keeper-rec/Capture.swift`: added a write-once `sessionAnchorPTS`, anchored the camera's first segment at the screen session anchor and rotated segments at `keyframePTS`, and now record the shared anchor/boundary as `ptsStart` (ptsEnd stays the last real frame). Added `ConcatAssertTests` warm-up alignment test + a negative control (pre-fix shape fails).
  - `[medium]` `[patch]` Empty-writer rotation branch could leave the replacement camera writer un-anchored / `cameraSessionStarted` stale (append to an unstarted writer). Fixed by anchoring the replacement at `keyframePTS`, marking it started, and separating the "received a real frame" signal (`cameraSegmentHasVideo`) from the reported `ptsStart` so anchored-but-frameless segments are dropped, never finalized into a zero-frame file.
  - `[low]` `[patch]` `finishAndExit` screen-never-anchored guard exited without cancelling/dropping the (empty) camera writer, orphaning a `camera-0000.mp4`. Fixed to cancel + remove it.

### 2026-07-19 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 0, low 1)
- defer: 1: (medium 1)
- reject: 18
- addressed_findings:
  - `[low]` `[patch]` The camera retiring-segment finalize-failure branch (`rotateCameraAtScreenBoundary`, Capture.swift ~1103) emitted a sticky terminal `cameraLost` warning ("the camera file ends here; screen recording continues") but did NOT actually stop the camera leg — by the time that async completion runs the replacement writer is already live, so the camera kept recording later segments while the host banner claimed it was lost. It was the only camera-fault path that warned without ending the leg. Fixed by routing it through the existing `failCameraNonFatally` (stop + drop the replacement writer, one honest sticky warning), matching every other camera-fault path (device loss, rotation-writer creation failure). Screen's own retiring-finalize failure remains fatal-by-design (master clock). `rec:build` + all 48 Swift tests pass.

## Auto Run Result

Status: done

### Summary
Implemented Story 20.1 end-to-end: an optional webcam recorded as its own separately-synchronized `camera-####.mp4` from a second in-sidecar `AVAssetWriter`, host-clock anchored and rotated in lockstep at the screen's segment boundaries; a flat camera device picker; lazy Camera-TCC on enable (non-blocking Start); non-fatal `cameraLost` handling; and a dual-track manifest/ledger + recovery. NDJSON protocol stayed additive (`PROTOCOL_VERSION` 1). One HIGH-severity review finding — a universal camera segment-0 alignment defect — plus two lower patches were fixed in review; the fix records the shared anchor/boundary as each camera segment's `ptsStart` and is pinned by a new alignment test with a negative control.

### Files changed
- `tools/keeper-rec/Sources/keeper-rec/main.swift` — NDJSON wire: `camera:true` capability, real `.video` permission, `listCameras()`, `requestCamera`, `startRecording` camera params, `simulateCameraRemoval`.
- `tools/keeper-rec/Sources/keeper-rec/Capture.swift` — second video-only camera writer + `AVCaptureSession`, lockstep rotation, shared-anchor `ptsStart`, non-fatal camera-loss + finalize paths.
- `tools/keeper-rec/Sources/keeper-rec/CameraHealth.swift` (new) — pure camera-loss policy mirroring `MicHealth`.
- `tools/keeper-rec/Tests/keeper-recTests/{CameraHealthTests.swift (new), ConcatAssert.swift, ConcatAssertTests.swift, FixtureSegments.swift}` — camera-loss unit tests + screen↔camera alignment harness with warm-up + negative-control tests.
- `src-tauri/crates/keeper-core/src/recording.rs` — `CameraSelection`, `SessionParams.camera`, wire emit, `request_camera`, dual-track `reconcile_from_dir` re-keyed on `(track, index)`.
- `src-tauri/crates/keeper-core/src/vm.rs` — stale `camera` VM doc comments refreshed (shapes already existed).
- `src-tauri/crates/keeper/src/{recorder.rs, ipc.rs, lib.rs}` — camera-permission port + command, `recording_start` camera args, track-branched live sink.
- `src-tauri/crates/keeper/Info.plist` — `NSCameraUsageDescription`.
- `src/lib/stores/recording-webcam.ts (+test)` — ephemeral webcam store.
- `src/components/recording/recording-webcam-controls.tsx (+test)` — Webcam card (Switch + flat camera Select + lazy permission).
- `src/components/layout/recording-pane.tsx (+test)`, `src/hooks/use-recording-session.ts`, `src/lib/ipc/client.ts (+test)`, `src/lib/ipc/gen/Recording{Capabilities,Device,Features,Sources}Vm.ts` — pane wiring, start plumbing, `requestCameraPermission`, regenerated ts-rs bindings.

### Review findings breakdown
- Patches applied: 3 (1 high, 1 medium, 1 low) — all camera-lifecycle correctness in `Capture.swift`.
- Deferred: 1 (medium) — a camera that is added but never yields frames (held by another app / TCC revoked mid-session) produces no file and no `cameraLost` warning; recorded to the deferred-work ledger.
- Rejected: 13 — noise / pathological / mirror already-accepted mic patterns (cross-thread `cameraSession` nil, spurious cameraLost-at-stop, `requestCamera` fixed-id, orientation/mirroring, hardcoded 30 fps/4 Mbps which the screen writer also uses, stale-uniqueID device, deskView in picker, minor nits).

### Verification
- `bun run check:rust` — PASS (fmt + clippy `-D warnings`).
- `bun run check` — PASS (biome + tsc + 1420 vitest).
- `bun run test:rust` — PASS (956 nextest, incl. dual-track reconcile + `request_camera` round-trip).
- `bun run rec:build` — PASS (wire reports `camera:true`, protocolVersion 1; smoke checks green).
- `bun run rec:test` — PASS (48 Swift tests incl. `CameraHealthTests` + the new alignment + negative-control tests).

### Residual risks
- Real end-to-end capture alignment (live SCK + AVCaptureSession clocks) is covered only by fixtures in CI; hardware validation is the SM-9/SM-10 acceptance (Story 20.6), per the AD-38 posture.
- A follow-up independent review is recommended: the HIGH-severity alignment fix reworked subtle camera-writer lifecycle/anchoring in `Capture.swift` and was not itself passed back through the adversarial reviewers.

### Follow-up review (2026-07-19)
The recommended independent review of the reworked `Capture.swift` camera-writer lifecycle ran (Blind Hunter + Edge Case Hunter, fresh context). The rework holds up: the `(track,index)` reconcile keying, the shared-anchor segment-0 fix, the frameless-rotation drop, the stop-path teardown, and the additive `PROTOCOL_VERSION`-stays-1 wire were all confirmed sound; the `finalizeGroup` concurrency concern was verified non-issue (serial `mediaQueue` + `stopping` guard eliminate any enter-after-notify window). One low-severity messaging/consistency defect was found and **fixed**: the retiring-camera-segment finalize-failure branch fired a sticky terminal `cameraLost` warning without actually ending the camera leg — now routed through `failCameraNonFatally` so the warning is honest. One new medium item was **deferred** (camera-setup failure at Start — device busy / no camera at all — aborts the whole session incl. screen; recorded to the deferred-work ledger for the SM-9/SM-10 hardware acceptance). Verification: `bun run rec:build` PASS, `bun run rec:test` PASS (48 Swift tests). Swift-only change; Rust/TS gates untouched. No further follow-up review warranted (single localized low-severity fix).
