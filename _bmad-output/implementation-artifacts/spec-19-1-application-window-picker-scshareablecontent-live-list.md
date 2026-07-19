---
title: 'Application/Window Picker — SCShareableContent Live List'
type: 'feature'
created: '2026-07-19'
baseline_revision: '33fe172f416da8aa9ddeed98a5202bb2ab42a161'
final_revision: '4691a00'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-19-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** Recording captures the whole main display only — `listSources` returns real displays but an empty `applications` array (16.4 stub), `recording_start` hardcodes the main-display target, and there is no source-picker UI. A user demoing one app leaks their notifications, other apps, and keeper itself into the file, and cannot pick a specific display.

**Approach:** Land the sources/devices leg for **video source selection** only. Teach the `keeper-rec` sidecar to enumerate real Applications (with icons) via `SCShareableContent` and to build an **app-scoped `SCContentFilter`** when a start request names an application; thread a selected source (a display **or** an application) from a new live **Source picker** through `recording_start`; and expose `list_sources` as a command the picker **polls** so the list re-enumerates as apps launch/quit. System-audio scoping (19.2), mic (19.3), and destination/fps (19.5) stay out of scope — 19.1 owns the video target only.

## Boundaries & Constraints

**Always:**
- The Source picker lists **Displays first, then Applications**, each with a name and a leading glyph (a monitor glyph for displays; the app's real icon for applications), **single-select** with radio semantics, ~44px rows. Each active display is an individually selectable row. Default selection is the main display (preserves today's full-main-display behavior).
- The list is **live**: the picker polls `list_sources` on a fixed interval (~3s) while the idle setup surface is visible and again on window focus, shows a subtle "refreshing…" affordance during an in-flight enumeration, and stops polling while a session is recording or the surface is unmounted.
- **App-scoped capture is exclusionary and disclosed.** When an application is selected, the sidecar captures only that application's windows via `SCContentFilter(display:including:[app], exceptingWindows:)`; keeper's own windows, other apps, and incoming notification banners never enter the file. The setup surface discloses this inline: "only {App}'s windows and audio are recorded — keeper, other apps, and notification banners stay out of the file." keeper's own bundle id is excluded from the Applications list (it can never be a target).
- **A vanished source fails cleanly at Start, never hangs.** On `start` naming an application whose pid is absent from the current `SCShareableContent` (or a display id no longer active), the sidecar emits an honest `error` event → `RecordingEvent::Failed` → the session reaches the `Failed` terminal state with no partial recording; the frontend surfaces a clear inline error and the session/tray stay idle. The existing bounded round-trip timeout is the anti-hang backstop for the enumeration/start round-trips.
- Wire logic stays **platform-free** in `keeper-core::recording` (additive `SessionParams`/`start` request fields + pure parsers only) — the `dependency_firewall_holds` guard must keep passing. All `SCShareableContent`/`SCContentFilter`/icon/spawn code lives in the Swift sidecar and `keeper/src/recorder.rs` under `#[cfg(desktop)]`.
- New/changed VMs derive `serde` + `ts_rs::TS` with camelCase + `#[ts(export)]`; regenerated `.ts` in `src/lib/ipc/gen/` is committed, never hand-edited (AD-7). The wire↔VM contract shape is the invariant; exact fields are code-owned (AD-34).
- App icons are bounded: downscaled to ≤64×64px PNG, base64 as a `data:image/png;base64,…` string in `RecordingApplicationVm.icon` (`Option`, `None` when an icon can't be produced). This keeps the polled list small — no large-payload-over-IPC violation. iOS and every non-desktop path return `Unsupported`.

**Block If:**
- App-scoped video capture (excluding keeper/other apps/notification banners) cannot be expressed with `SCContentFilter` within the existing macOS 13.0 capability floor — i.e. it would require raising `minimumSystemVersion`/the `recording` capability floor. Surface, do not raise the floor.
- Real application enumeration or icon extraction requires a TCC permission or entitlement **beyond** the existing Screen Recording grant, or a stored-state migration. Surface rather than proceed.

**Never:**
- No system-audio toggle or per-app audio scoping (19.2), no microphone (19.3), no destination-folder chooser or fps control (19.5), no webcam (20.1). 19.1 changes the **video source** only; audio behavior stays exactly as 16.6 left it.
- No per-**window** picker — "Application/Window" means whole-application capture, not a window-level list (the ACs list Displays and Applications only).
- No persistent-process/Channel lifecycle for the source list — polling the existing one-shot round-trip is the transport (a persistent sidecar for `listSources` is a lifecycle the codebase does not have and is out of scope).
- No new network destination, upload, telemetry, preview, or thumbnail (local-only, FR-76). No hand-edited generated `.ts`. No `.unwrap()` in Rust production paths; no `any` in TS.
- No claim that on-hardware capture-scoping is automatically verified here — the pixel-level "only the app is in the file" check folds into SM-9/SM-10 acceptance (Story 20.6), like every real-capture leg since 16.6. 19.1's automated gates are compile + unit + Swift release build.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Enumerate sources | picker polls `list_sources` with Screen Recording granted | `RecordingSourcesVm` with real `displays` and `applications` (name, pid, bundleId, icon), keeper excluded, apps that own ≥1 on-screen window, name-sorted | No error |
| Icon unavailable | an app whose icon can't be rendered/encoded | that `RecordingApplicationVm.icon` is `None`; picker shows the generic app glyph fallback | graceful, no crash |
| Permission not granted | enumerate without Screen Recording grant | `applications` empty (degraded), never a hang or crash; pre-flight (16.5) still owns the fix path | honest empty |
| Start app-scoped | `recording_start({kind:"application",pid,bundleId})`, app present | wire `start{…,applicationPid,bundleId}` (no `displayId`); sidecar builds app-scoped filter; manifest `CaptureTarget` = application | No error |
| Start display | `recording_start({kind:"display",displayId})` or `None` | wire `start{…,displayId}` (None → main display, unchanged 16.6 path); manifest `CaptureTarget` = display | No error |
| Vanished app at Start | selected app quit before Start | sidecar `error` event → `RecordingEvent::Failed` → `Failed` terminal, no recording; inline error surfaced; session/tray idle | clean Failed, never hangs |
| Selected source vanishes in list | polled re-enumeration no longer contains the selected app | picker marks the selection unavailable; Start against it yields the clean Failed above (selection is not silently swapped) | honest |
| iOS / sidecar absent | any `list_sources`/`start` | `Unsupported`, no spawn, no panic | honest |

</intent-contract>

## Code Map

- `tools/keeper-rec/Sources/keeper-rec/main.swift` -- `import ScreenCaptureKit` + `import AppKit`; fill the `listSources` handler (currently `applications: []` ~line 139) with real enumeration — this makes the handler **async** (`SCShareableContent.current`), so the reply must be serialized off a `Task` through the existing `stdoutLock`/`writeLine`. Emit `applications` as `{bundleId,name,pid,icon}` (icon = `NSRunningApplication(processIdentifier:pid)?.icon` → ≤64px → PNG → base64 data-URI, nil-safe), excluding keeper's own bundle id, apps with no on-screen window, deduped, name-sorted. Extend `startRecording` to accept `applicationPid`/`bundleId`; when present, resolve the `SCRunningApplication` from current shareable content (absent → honest `error` event, no capture).
- `tools/keeper-rec/Sources/keeper-rec/Capture.swift` -- add the app-scoped filter branch (~line 190): `SCContentFilter(display: <app's display, fallback main>, including: [app], exceptingWindows: [])`; keep the display-only `SCContentFilter(display:excludingWindows:)` branch. `excludesCurrentProcessAudio` stays as set by 16.6 (audio unchanged).
- `tools/keeper-rec/Package.swift` -- add `.linkedFramework("AppKit")` for `NSRunningApplication` icons (ScreenCaptureKit/AVFoundation/CoreGraphics already linked). Zero third-party deps.
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `icon: Option<String>` to `RecordingApplicationVm`; add `RecordingTargetVm` — an internally `#[serde(tag = "kind", rename_all = "camelCase")]` enum with `Display { display_id: u32 }` and `Application { pid: i32, bundle_id: String }` (`#[ts(export)]`) as the `recording_start` input type.
- `src-tauri/crates/keeper-core/src/recording.rs` -- add `application: Option<ApplicationTarget>` (`{pid:i32, bundle_id:String}`) to `SessionParams`; `start_recording_request` serializes `applicationPid`+`bundleId` when `application` is `Some` (omitting `displayId`), else `displayId` as today (application target wins). Extend the manifest `CaptureTarget` with an `application` kind (`bundle_id`, `pid`) — the enum whose comment reserved app targets for "a later epic." Pure request/parse only; firewall stays clean.
- `src-tauri/crates/keeper/src/recorder.rs` -- wrap `fetch_sources` in `bounded(PREFLIGHT_TIMEOUT)` (enumeration can now block on SCK). No behavior change for `IosRecorder` (`Unsupported`).
- `src-tauri/crates/keeper/src/ipc.rs` -- new `#[tauri::command] recording_list_sources() -> Result<RecordingSourcesVm, IpcError>` calling `state.recorder.list_sources()` (gated by `recording_supported()`); change `recording_start(target: Option<RecordingTargetVm>)` to map the target into `CaptureTarget` + `SessionParams` (application vs display; `None` → main display, unchanged).
- `src-tauri/crates/keeper/src/lib.rs` -- register `recording_list_sources` in the `invoke_handler`.
- `src/lib/ipc/client.ts` -- add `listRecordingSources(): Promise<RecordingSourcesVm>`; re-export `RecordingSourcesVm`/`RecordingDisplayVm`/`RecordingApplicationVm`/`RecordingTargetVm`; give `recordingStart` a `target?: RecordingTargetVm` arg passed to `invoke`.
- `src/lib/stores/recording-source.ts` -- NEW vanilla zustand mirror: `{ sources: RecordingSourcesVm | null, selected: RecordingTargetVm, refreshing: boolean }`; poll start/stop, default-select the main display, mark/clear a vanished selection, `select(target)`, and `resetRecordingSourceForTest()` (mirror `recording-settings.ts` conventions; the list is a Rust mirror, selection is ephemeral UI).
- `src/components/recording/recording-source-picker.tsx` -- NEW grouped `RadioGroup` (section headers "Displays"/"Applications") of ~44px rows (`min-h-11`), leading glyph (lucide `Monitor` for displays; `<img>` data-URI app icon with a lucide `AppWindow` fallback), name, radio semantics, "refreshing…" affordance, and the inline app-scope disclosure line; export label/testid constants.
- `src/components/layout/recording-pane.tsx` -- replace the `"Source"` placeholder branch in the `SETUP_CARDS.map` with `<RecordingSourcePicker />` (mirror the `"Segmenting"` specialization); the header Start passes `useRecordingSource`'s selected target into `recordingStart`.
- Tests: `recording.rs`/`recorder.rs` unit tests; `recording-source-picker.test.tsx`; `recording-source.test.ts`; update `recording-pane.test.tsx` (mock `listRecordingSources`) and the `fetch_sources` fake-sidecar test (applications no longer asserted empty).

## Tasks & Acceptance

**Execution:**
- [x] `tools/keeper-rec/Sources/keeper-rec/main.swift` -- real async `SCShareableContent` app enumeration (name/pid/bundleId + ≤64px PNG data-URI icon, keeper excluded, on-screen-window apps only, name-sorted, serialized through `stdoutLock`); `startRecording` accepts `applicationPid`/`bundleId` and resolves/validates the app (absent → honest `error` event) -- the sidecar half of the live list + app target.
- [x] `tools/keeper-rec/Sources/keeper-rec/Capture.swift` -- app-scoped `SCContentFilter(display:including:[app], exceptingWindows:)` branch alongside the display branch -- exclusionary app capture (keeper/others/banners out).
- [x] `tools/keeper-rec/Package.swift` -- link AppKit for `NSRunningApplication` icons -- icons without third-party deps.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- `RecordingApplicationVm.icon: Option<String>`; new `RecordingTargetVm` tagged enum -- typed source/target contract (regenerates `.ts`).
- [x] `src-tauri/crates/keeper-core/src/recording.rs` -- `SessionParams.application`; `start_recording_request` app-vs-display serialization (app wins); manifest `CaptureTarget` application kind -- platform-free wire + manifest, firewall clean.
- [x] `src-tauri/crates/keeper-core/src/recording.rs` (tests) -- `start_recording_request` app-target wire shape (`applicationPid`/`bundleId`, no `displayId`) + unchanged display wire; `parse_sources_result` over a canned `applications` array with icons → populated `RecordingSourcesVm`; `CaptureTarget::application` serde round-trip; `dependency_firewall_holds` still green -- no-hardware coverage.
- [x] `src-tauri/crates/keeper/src/recorder.rs` -- `bounded(PREFLIGHT_TIMEOUT)` around `fetch_sources`; update the fake-sidecar sources round-trip test to a canned application -- anti-hang + regression coverage.
- [x] `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- `recording_list_sources` command; `recording_start(target)` mapping to `CaptureTarget`/`SessionParams`; register in `invoke_handler` -- the picker's data source and selection sink.
- [x] `src/lib/ipc/client.ts` -- `listRecordingSources` wrapper, source/target VM re-exports, `recordingStart(target?)` -- first frontend consumer of `list_sources`.
- [x] `src/lib/stores/recording-source.ts` (+ `.test.ts`) -- mirror store with polling, default main-display selection, vanished-selection handling, `resetRecordingSourceForTest()` -- shared selection between picker and header Start.
- [x] `src/components/recording/recording-source-picker.tsx` (+ `.test.tsx`) -- grouped radio picker with glyphs/icons, "refreshing…" affordance, app-scope disclosure, exported labels -- the live Source card.
- [x] `src/components/layout/recording-pane.tsx` -- mount `<RecordingSourcePicker />` in the `"Source"` branch; Start passes the selected target -- wires the picker into the surface (update `recording-pane.test.tsx`).

**Acceptance Criteria:**
- Given the setup surface is visible and idle, when the Source card renders, then it shows a single-select list of Displays then Applications (names + glyphs/icons), each display individually selectable, defaulting to the main display, and re-enumerates on a ~3s poll and on window focus with a "refreshing…" affordance — pausing while recording.
- Given an application is selected, when a recording starts, then `recording_start` sends an application target, the sidecar builds an app-scoped `SCContentFilter`, the manifest records an application `CaptureTarget`, and the surface discloses inline that only that app's windows are recorded (keeper/other apps/notification banners excluded).
- Given a selected application that has quit, when the user presses Start, then the sidecar emits an honest error → the session reaches `Failed` with no partial recording and a clear inline error, never a hung recording; the tray/session stay idle.
- Given the sidecar is unavailable or the platform is iOS, when `list_sources`/`recording_start` are called, then each returns `Unsupported` with no spawn/panic, and `keeper-core::recording` still carries no tauri/Apple/process token (`dependency_firewall_holds` passes).
- Given `bun run check`, `bun run check:rust`, `bun run test:rust`, and `bash scripts/build-keeper-rec.sh`, then biome/tsc/vitest, clippy (`-D warnings`), cargo-nextest (with the regenerated, committed `.ts`), and the Swift release build all pass.

## Design Notes

**Polling, not a Channel.** `listSources` today is one child spawn per call; SCK offers no launch/quit callback, so even a persistent sidecar would internally poll. The frontend polling the existing round-trip (~3s while idle, plus on focus) mirrors how `recording_permission` re-detects and how `use-recording-session` polls `recording_status` — no new process lifecycle. `bounded(PREFLIGHT_TIMEOUT)` on `fetch_sources` guards the now-async enumeration.

**Additive target, application wins.** `SessionParams` keeps `display_id` for 16.6's path and gains `application: Option<ApplicationTarget>`; the wire builder emits `applicationPid`/`bundleId` (omitting `displayId`) only when app-scoped, so the display path is byte-for-byte unchanged. The `RecordingTargetVm` tagged enum gives the frontend a clean `{kind:"display"|"application", …}` union and keeps invalid combos unrepresentable.

**App-scoped filter example (Capture.swift):**
```swift
let filter = SCContentFilter(display: display, including: [app], exceptingWindows: [])
```
Exclusion is the invariant: only `app`'s windows land in the file; keeper (also excluded from the picker list by bundle id), other apps, and notification banners are absent because they are not `app`. On-hardware pixel verification is Story 20.6.

**Vanished source is the sidecar's job.** Rather than trusting the frontend's snapshot, `startRecording` re-resolves the pid from live `SCShareableContent`; absent → honest `error` event → `parse_event` → `RecordingEvent::Failed` → `Failed` terminal. This satisfies the "clear inline error, never a hung recording" AC even under a launch/quit race between the last poll and Start.

## Verification

**Commands:**
- `cd src-tauri && cargo test -p keeper-core recording` -- expected: app-target wire, sources-parse (with icons), `CaptureTarget::application` serde, and firewall tests pass.
- `bun run test:rust` -- expected: cargo-nextest green; regenerates `RecordingApplicationVm.ts`, `RecordingTargetVm.ts`, `RecordingSourcesVm.ts` (commit them).
- `bun run check:rust` -- expected: `cargo fmt --check` + clippy `--all-targets -D warnings` clean across both crates.
- `bun run check` -- expected: biome + tsc + vitest pass (picker + store tests, updated recording-pane test).
- `bash scripts/build-keeper-rec.sh` -- expected: `swift build -c release --arch arm64` succeeds with the SCShareableContent enumeration + AppKit icons + app-scoped filter; smoke green.
- `cd src-tauri && cargo check --workspace --target aarch64-apple-ios` -- expected: compiles; iOS recorder methods return `Unsupported`.

**Manual checks:**
- Confirm `keeper-core/src/recording.rs` imports no tauri/Apple-framework/process API (the new SCK/icon/spawn code is only in the sidecar and `keeper/src/recorder.rs`).

## Review Triage Log

### 2026-07-19 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 5: (high 0, medium 3, low 2)
- defer: 0
- reject: 19
- addressed_findings:
  - `[medium]` `[patch]` The `RadioGroup` was controlled by `value` but had no `onValueChange`; selection rode solely on each row's `onClick`, so Radix keyboard (arrow-key) selection never updated the mirror store — a keyboard user would arrow to an app, see it check, press Start, and silently record the stale target (the main display). Wired `onValueChange` on the group to decode the value string back to the full target (looking the app's `bundleId` up from the list) and select it; added a keyboard-arrow-selection test.
  - `[medium]` `[patch]` The app-scoped `SCContentFilter` was anchored to the **main display** (`CGDisplayIsMain`), deviating from the spec's "the app's display, fallback main" — an app living on a secondary display would record black. It also never checked the app currently owns an on-screen window, so a resolvable-but-windowless app yielded an empty (black) filter with no honest error. Now resolves the display hosting the app's frontmost on-screen window (fallback main, then any), and raises an honest `error` ("no on-screen window to record") when the app has none.
  - `[medium]` `[patch]` The picker polled on mount/unmount only, not session-live; because the setup cards render unconditionally, a fresh `keeper-rec` child spawned every ~3s throughout an active recording, contradicting the store's own documented "stops while recording." Added an `active` prop (`RecordingPane` passes `active={!live}`); the poll effect stops polling when inactive. Added a picker test asserting no enumeration runs while inactive.
  - `[low]` `[patch]` App identity was matched by `pid` alone (frontend `isSameTarget`/`isSelectionAvailable` and the sidecar's live re-resolution), so a pid recycled by the OS to a different app within the ~3s poll window could read back as still-available and capture the wrong app. Now matches on `pid` AND `bundleId` in both the frontend and the sidecar (`bundleId` was already on the wire); updated/added tests for the recycled-pid case.
  - `[low]` `[patch]` `resolve_capture_target` (the `RecordingTargetVm` → `CaptureTarget`/`display_id`/`ApplicationTarget` mapping) had no direct unit test — a tuple-order regression would pass every other test. Added `resolve_capture_target_maps_each_kind` covering all three branches.

_Rejected (noise / by-design / not-this-story):_ `listSources` async-`Task` reply reordering (one request per spawn, id-correlated read tolerates ordering); non-numeric/zero `applicationPid` coercion (host always sends a valid i32; any unmatched pid → honest error); redundant `onScreenWindowsOnly` + `isOnScreen` double-filter (harmless/defensive); icon `data:` URI `<img src>` validation and `onError` fallback (first-party bounded PNG, `<img>` not `href`); square-icon aspect distortion (app icons are square); generic disclosure text when the selected app has vanished (honest fallback alongside the unavailable alert); module-global poll timer under StrictMode (single instance; cleanup-before-re-run leaves a valid timer); per-poll `keeper-rec` spawn cost (the sanctioned polling transport, now bounded to the idle surface by the poll-stop fix); per-poll icon re-encode (a fresh process each call — no cross-poll cache is possible); `NO_APPLICATIONS_NOTE` ambiguity (mitigated by the refreshing affordance + the Permissions card); manifest recording `pid` (spec-mandated informational field; `bundleId` is the durable id); Start enabled against a known-unavailable selection (spec-intended: the sidecar clean-fails, the selection is never silently swapped); `recording_start` not re-gating on `recording_supported()` (pre-existing, honest `Unsupported` downstream via `IosRecorder`); `bundle_id.clone()` in the mapping (one unavoidable clone); stale in-flight poll repopulating after unmount (unobserved store, overwritten on remount); focus-refresh joining an in-flight poll (intentional dedup, staleness bounded by one quick poll); duplicate display-id React keys (CoreGraphics display ids are unique); vanished-selection RadioGroup value pointing at an unrendered item (consistent with the unavailable state — nothing checked + alert).

### 2026-07-19 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 2: (high 0, medium 1, low 1)
- reject: 17
- addressed_findings:
  - none

Independent follow-up review (Blind Hunter + Edge Case Hunter). No new code-level defect survived scrutiny: every correctness/keyboard/poll-lifecycle finding either reproduced only as a theoretical race the same-render `applications` snapshot already closes, or restated a call the prior pass rejected with sound rationale (async-`Task` reply ordering, `NO_APPLICATIONS_NOTE` ambiguity, disclosure-vs-alert coherence, `<img>` `onError`, icon aspect, `recording_start` platform re-gate, module-global poll timer, vanished-selection RadioGroup value). Two genuinely new, real limitations of the app-scoped `SCContentFilter` were deferred (not patchable without cross-display capture work SCK does not offer in one filter, and folded into the on-hardware 20.6 verification): (1) an app whose windows span multiple displays records only the anchor display's windows, silently and undisclosed; (2) the app-scoped stream is sized to the full anchor display, so the file is full-display resolution with the app composited on a black background rather than cropped to app content.

## Auto Run Result

Status: done

**Summary.** Implemented Story 19.1 — the live application/window Source picker. `keeper-rec` now enumerates real Applications via `SCShareableContent` (name/pid/bundleId + an optional ≤64px PNG data-URI icon from `NSRunningApplication.icon`, keeper excluded, on-screen-window owners only, name-sorted) and builds an **app-scoped `SCContentFilter(display:including:[app])`** when a `start` request names an application — exclusionary capture (keeper, other apps, notification banners stay out). A new `RecordingTargetVm` tagged enum + additive `SessionParams.application`/`start` wire fields + a manifest `CaptureTarget::application` kind (all platform-free in `keeper-core`, firewall intact) thread a selected source (display **or** application) from a React `RecordingSourcePicker` (grouped radio list, ~3s poll + focus re-enumeration, "refreshing…" affordance, inline app-scope disclosure) through a new `recording_list_sources` command and `recording_start(target)`. A vanished source fails cleanly at the sidecar (honest `error` → `Failed`), never a hung recording. Audio (19.2), mic (19.3), and destination/fps (19.5) stay out of scope.

**Files changed.**
- `src-tauri/crates/keeper-core/src/vm.rs` — `RecordingApplicationVm.icon: Option<String>`; new `RecordingTargetVm` tagged enum.
- `src-tauri/crates/keeper-core/src/recording.rs` — `SessionParams.application` + `ApplicationTarget`; `start_recording_request` app-vs-display serialization (app wins); manifest `CaptureTarget::application`; tests (app-target wire, sources-parse-with-icons, serde); firewall stays green.
- `src-tauri/crates/keeper/src/ipc.rs` — `recording_list_sources` command (capability-gated); `recording_start(target)` via `resolve_capture_target`; `resolve_capture_target_maps_each_kind` test.
- `src-tauri/crates/keeper/src/{lib.rs,recorder.rs}` — command registration; `fetch_sources` bounded-timeout + fake-sidecar sources test.
- `tools/keeper-rec/Sources/keeper-rec/main.swift` — async `SCShareableContent` app enumeration + icons; `startRecording` threads `applicationPid`+`bundleId`.
- `tools/keeper-rec/Sources/keeper-rec/Capture.swift` — app-scoped filter branch; app resolved by pid+bundleId; anchored to the app's on-screen-window display with an honest no-window error.
- `tools/keeper-rec/Package.swift` — links AppKit for icons.
- `src/components/recording/recording-source-picker.tsx` (+ test) — the live grouped radio picker (keyboard-selectable via `onValueChange`, live-aware `active` prop).
- `src/lib/stores/recording-source.ts` (+ test) — mirror store: polling, default main-display selection, pid+bundleId identity, vanished-selection marking.
- `src/components/layout/recording-pane.tsx` (+ test), `src/hooks/use-recording-session.ts`, `src/lib/ipc/client.ts` — mount the picker (`active={!live}`), thread the target into Start, IPC wrapper + VM re-exports.
- `src/lib/ipc/gen/{RecordingApplicationVm.ts,RecordingTargetVm.ts}` — regenerated ts-rs bindings (committed, not hand-edited).

**Review findings breakdown.** 5 patches applied (3 medium: keyboard-selection `onValueChange`, app-scoped filter display-anchoring + no-window honest error, poll-stop while recording; 2 low: pid+bundleId app identity, `resolve_capture_target` unit test). 0 intent_gap, 0 bad_spec (no repair loopback), 0 defer, 19 rejected (noise / by-design / pre-existing). Details in the Review Triage Log above.

**Follow-up review recommended:** true — the review-driven patches altered capture-target resolution across the language boundary (Swift display-anchoring + app identity, Rust mapping, React keyboard/poll-lifecycle), i.e. three medium correctness fixes spanning all three layers of a foundational new surface. Each is tested and every gate is green, but an independent follow-up over the app-scope resolution + selection-threading path is warranted before 19.2–19.5 build on it.

**Verification.** All gates green after the patches: `cargo test -p keeper-core recording` (73 passed, incl. `dependency_firewall_holds`); `bun run test:rust` → 890 passed; `bun run check:rust` (fmt + clippy `-D warnings`) clean; `bun run check` → biome + tsc + vitest (1334 passed, +3 new) + core-tauri-free check clean; `bash scripts/build-keeper-rec.sh` → swift release build + smoke green; `cargo check --workspace --target aarch64-apple-ios` → exit 0 (`IosRecorder` new methods → `Unsupported`; the lone `parse_macos_major` dead-code warning is pre-existing). Manual: `keeper-core/src/recording.rs` carries no tauri/Apple/process token.

**Residual risks.** On-hardware verification that app-scoped capture yields only the app's windows (pixel-level) folds into SM-9/SM-10 acceptance (Story 20.6), like every real-capture leg since 16.6 — 19.1's automated gates are compile + unit + Swift release build. The source list uses a ~3s process-spawn poll (the sanctioned transport, now bounded to the idle setup surface); a persistent-process/Channel transport was deliberately out of scope. App icons ride the polled list as bounded ≤64px PNG data-URIs (small; not a large-IPC-payload violation).

---

**Follow-up review pass (2026-07-19).** The `followup_review_recommended: true` from the first pass triggered an independent Blind Hunter + Edge Case Hunter review. Outcome: 0 intent_gap, 0 bad_spec, 0 patch, 2 defer, 17 reject — no code changes. No new code-level defect survived scrutiny (the surviving raised items either reproduced only as theoretical races the same-render `applications` snapshot already closes, or restated first-pass rejections with sound rationale). Two genuinely new, real limitations of the app-scoped `SCContentFilter` were logged to the deferred-work ledger: (1) a multi-display app records only its anchor display's windows (silent, undisclosed coverage gap — no leak); (2) the app-scoped stream is sized to the full anchor display, so the file is full-display resolution with the app on a black background rather than cropped to content. Both are non-trivial (no single-filter SCK fix) and fold into the on-hardware 20.6 verification. `followup_review_recommended` is now `false` — this pass altered no code. No re-run of quality gates was needed (no code changed since the first pass's green run).
