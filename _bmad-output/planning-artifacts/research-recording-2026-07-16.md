# Technical + Product Research: macOS Screen Recording to Local Files

- **Date:** 2026-07-16
- **Researcher:** BMAD technical-research pass (Claude)
- **Scope:** New keeper capability — record the owner's screen activity (meetings, presentations)
  to local files: full screen or a selected application, with selectable system-audio /
  microphone / webcam sources, continuous recording saved in size-bounded segments, tray
  state (recording / error), files landing in a user-chosen folder.
- **Repo grounding:** Tauri 2 workspace (`src-tauri/crates/keeper-core` platform-free +
  `keeper` shell), `unsafe_code = "deny"` workspace-wide with function-level audited
  exceptions only (docs/constraints-and-limitations.md, policy 2026-07-11), cargo-deny
  Apache/MIT firewall (`src-tauri/deny.toml`), `Platform` port trait with `sidecar_path`
  (`crates/keeper-core/src/platform.rs`), bbctl launch-on-demand sidecar precedent
  (Story 6.7, AD-16), opt-in tray (`crates/keeper/src/tray.rs`, Story 10.3, FR-53),
  `CapabilitiesVm` per-platform gating over the IPC handshake (AD-27), release =
  Developer-ID-signed + notarized aarch64 bundle (docs/release.md), current
  `minimumSystemVersion: 11.0` (tauri.conf.json).

---

## Executive summary

**Recommended route: a small first-party Swift helper sidecar** (`keeper-rec`) that owns the
entire capture pipeline (ScreenCaptureKit + AVAssetWriter/AVFoundation), spawned launch-on-
demand through the existing sidecar mechanism and controlled over NDJSON-RPC on stdio.
keeper (Rust/TS) owns UI, settings, tray state, folder choice, and the segment manifest.
This keeps the workspace `unsafe_code = "deny"` policy intact (zero new unsafe in Rust),
gives day-one access to every current and future ScreenCaptureKit feature, isolates capture
crashes from the chat client, and follows two strong precedents: keeper's own bbctl sidecar
(Story 6.7) and Kap's `aperture` (a Swift CLI recorder driven from Electron — the same shape,
shipped for years). Cap (cap.so) proves the in-process Rust route is *possible* but only at
the cost of forked ffmpeg/nokhwa/wgpu crates and a large audited-unsafe surface keeper's
policy exists to avoid. ffmpeg/avfoundation is rejected outright: no per-app capture, no
system audio without a virtual driver, GPL-tainted builds vs the cargo-deny firewall.

Capability floor: **macOS 13.0** (system-audio capture), runtime-gated via `CapabilitiesVm`
(app-wide minimum stays 11.0). Container: **fragmented MP4** segments rotated gaplessly by a
dual-AVAssetWriter handover, sized by the user's segment budget, with a JSON manifest and a
startup recovery pass. Webcam MVP = **separate synchronized file**, PiP burn-in deferred.

---

## 1. macOS capture technology, state of 2026

### 1.1 ScreenCaptureKit (SCK) is the only sanctioned API

Apple has funneled all screen capture into ScreenCaptureKit; the legacy paths
(`CGDisplayStream`, `CGWindowListCreateImage`, AVFoundation's screen input) are deprecated
and increasingly hostile under TCC. SCK provides exactly the selection model the owner
asked for:

- **Content selection:** `SCShareableContent` enumerates displays, windows, and running
  applications; `SCContentFilter` captures a whole display, a single window, or a display
  *filtered to one application* (including "display minus these apps"). "Full screen OR a
  selected application" maps 1:1 onto `SCContentFilter`. (macOS 12.3+)
- **System audio:** `SCStreamConfiguration.capturesAudio` delivers system/app audio sample
  buffers scoped to the same filter (so "record only Zoom" also records only Zoom's audio,
  and `excludeCurrentProcessAudio` keeps keeper's own notification sounds out). (macOS 13.0+)
- **Microphone:** `SCStreamConfiguration.captureMicrophone` + `microphoneCaptureDeviceID`
  delivers mic buffers in the same stream, timestamp-aligned with video and system audio.
  (macOS **15.0+** only — below 15 the mic is captured with a parallel `AVCaptureSession`,
  which is routine AVFoundation code in the same helper.)
- **System picker:** `SCContentSharingPicker` (macOS 14.0+) is the system-owned
  window/display/app picker. Capture initiated through it **does not require the Screen
  Recording TCC grant** and avoids the Sequoia re-authorization nags (see 1.3).
- **Presenter overlay:** macOS 14+ composites the webcam over shared content automatically
  when a camera is active during capture — a system feature the app gets "for free" during
  capture, not an API keeper must build. Worth noting in UX copy; not a substitute for a
  recorded webcam track.
- **Direct-to-file:** `SCRecordingOutput` + `SCRecordingOutputConfiguration` (macOS
  **15.0+**) records the stream straight to a file (outputURL, H.264/HEVC codec choice,
  file type). Limits found in Apple docs/forums: **at most one recording output per
  stream**, it should be attached before `startCapture`, and changing the stream config
  stops the recording — i.e., **no built-in segment rotation**. Good for a "one file"
  MVP-of-the-MVP; not sufficient for keeper's segmented requirement (see §4).
- **VideoToolbox:** H.264/HEVC encoding on Apple Silicon is hardware-accelerated
  transparently through AVAssetWriter/`SCRecordingOutput`; no direct VideoToolbox code is
  needed unless we later want fine-grained rate control.
- **macOS 26 (Tahoe, current in 2026):** HDR capture (HEVC) vs SDR (H.264) selection
  surfaced system-wide, per-window recording without stray notifications, "cleaner
  permission system" for third-party recorders; developer-visible quirk: non-bundled plain
  executables that request Screen Recording no longer appear in System Settings on 26.1,
  though the prompt still works. No breaking SCK API changes surfaced in research.

### 1.2 Version matrix (drives the floor decision)

| Feature | Min macOS |
|---|---|
| SCK display/window/app capture | 12.3 |
| System-audio capture (`capturesAudio`) | 13.0 |
| `SCContentSharingPicker`, presenter overlay, screenshot API | 14.0 |
| `captureMicrophone` (mic in-stream), `SCRecordingOutput` | 15.0 |
| HDR capture configs | 15.0 (surfaced further in 26) |

keeper's owner requirement includes system audio → **capability floor macOS 13.0**.
Recommended: gate the *feature* at 13.0 via `CapabilitiesVm` (AD-27) and branch internally
(15+ uses in-stream mic and optionally `SCRecordingOutput`; 13–14 uses AVCaptureSession mic
+ AVAssetWriter — which §4 recommends as the primary writer anyway, making the branch
small). The app-wide `minimumSystemVersion` stays 11.0; the recording surfaces simply never
render below the floor — exactly the AD-27 "no dead buttons" rule.

### 1.3 TCC permissions

Three TCC classes are involved:

- **Screen Recording** (`kTCCServiceScreenCapture`) — prompted on first
  `SCShareableContent` access; user must often relaunch the app after granting. Detect
  with `CGPreflightScreenCaptureAccess()`, request with `CGRequestScreenCaptureAccess()`
  (one real prompt per app lifetime; afterwards only System Settings can change it). Deep
  link for the "fix it" button:
  `x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture`.
- **Microphone** — `AVCaptureDevice.requestAccess(for: .audio)`; requires
  `NSMicrophoneUsageDescription` in the **app bundle's** Info.plist (Tauri: the
  `bundle.macOS.infoPlist`/Info.plist merge in `tauri.conf.json`).
- **Camera** — same pattern with `.video` + `NSCameraUsageDescription`.

Attribution: TCC attributes a child process to its **responsible process** — the parent app
bundle. A sidecar spawned by keeper prompts *as keeper*, uses keeper's usage strings, and
appears as "keeper" in System Settings. This is the behavior we want and another argument
for the sidecar being spawned (not a LaunchAgent).

Hard-won facts for the risk register:

- **Ad-hoc/unsigned builds are broken for SCK on macOS 15+.** TCC ties the Screen Recording
  grant to the code-signing identity; ad-hoc identities change every build, so each build is
  a "new app", and Sequoia goes further — it **silently rejects SCK access for ad-hoc
  binaries without a Team ID**. Cap hit exactly this
  (CapSoftware/Cap issue #1722, "[DevEx] Local dev builds unable to test screen recording on
  macOS Sequoia"). Consequence: keeper **dev builds must be signed with an Apple
  Development certificate** (free account suffices — keeper already runs free-team iOS
  signing per the architecture spine) for anyone to test recording locally. Release builds
  are already Developer-ID signed + notarized (docs/release.md) — no change there.
- **Sequoia/Tahoe re-authorization nag:** macOS 15 re-prompts ("continue to allow…")
  **monthly** (was weekly in early betas) for apps using SCK *outside* the
  `SCContentSharingPicker`. Bypass options: (a) use the picker flow where it fits, (b) the
  `com.apple.developer.persistent-content-capture` entitlement — restricted, requires an
  Apple approval process and a paid developer account; out of reach under decision D-1
  (no paid ADP). MVP accepts the monthly nag and documents it.

---

## 2. Implementation route for a Tauri 2 Rust app

### 2.a In-process Rust bindings

| Crate | State (2026-07) | Verdict |
|---|---|---|
| `screencapturekit` (doom-fish/screencapturekit-rs) | v8.0.0 (2026-06), MIT/Apache-2.0, actively maintained, powers Cap/AFFiNE; macOS 12.3+ capture, 13+ audio, 14+ picker, 15+ mic + `SCRecordingOutput`; leak-tested retain/release wrappers | Best-in-class *capture* binding, but **no encoding/writing** — its own examples shell out to ffmpeg for H.264. Would still need AVAssetWriter bindings. |
| `cidre` | Broad Apple-framework bindings incl. SCK + AVAssetWriter; single-maintainer, API churn, thin/unsafe-heavy by design | Covers everything but imports a very large FFI surface into the shell crate — the exact thing the audited-unsafe policy bounds to *function-level* exceptions. |
| `scap` (CapSoftware) | Explicitly WIP, "not production", effectively unmaintained in favor of Cap's internal crates | No. |
| `xcap` | Screenshot-first; recording/audio immature | No. |
| `objc2-*` framework crates | Complete but raw; every call site is `unsafe` | Same policy problem as cidre. |

Policy fit: `unsafe_code = "deny"` is a per-crate lint, so *dependencies* containing unsafe
don't trip it — technically the `screencapturekit` crate could be used with zero keeper-side
unsafe. But there is **no maintained safe AVAssetWriter binding**, so the writing/muxing
half (the actually hard, segment-rotation half) would land as keeper-authored `unsafe` objc2
code or a cidre dependency — dozens of audited exceptions vs. today's inventory of exactly
one (`IosPlatform::exclude_from_backup`). An in-process capture pipeline also puts real-time
CMSampleBuffer handling inside the chat client's process: a capture bug tears down the
user's messaging app mid-meeting. Cap ships this route — with forked `rust-ffmpeg`,
`nokhwa`, `cpal`, and `wgpu`, and a team maintaining them. That is not keeper's cost
envelope.

### 2.b Swift helper sidecar — **recommended**

A small (~1–2 kLOC) Swift executable, `keeper-rec`, built with SwiftPM in-repo, doing:
SCK stream setup → sample buffers → dual-AVAssetWriter segmented fMP4 writing (§4), mic via
in-stream (15+) or AVCaptureSession (13–14), webcam via AVCaptureSession (§5). keeper spawns
it launch-on-demand through the existing `Platform::sidecar_path` port + Tauri `externalBin`
(the bbctl mechanism, Story 6.7 / AD-16), and controls it with **NDJSON-RPC over stdio**:

```
→ {"id":1,"method":"getCapabilities"}                        ← versions, feature flags, permission states
→ {"id":2,"method":"listSources"}                            ← displays, apps, mics, cameras
→ {"id":3,"method":"start","params":{filter, audio, mic, camera, dir, segmentMB}}
← {"event":"segmentClosed","path":"…seg0001.mp4","bytes":…}  (events interleaved)
← {"event":"state","recording":true,"elapsedSec":754,"warning":null}
→ {"id":4,"method":"stop"}                                   ← final manifest
```

Why this wins:

- **Policy:** zero new `unsafe` anywhere in Rust; the audited-unsafe inventory stays at one
  entry. Swift *is* the safe binding to these frameworks.
- **API access:** first-party Swift gets every SCK feature the day Apple ships it (picker,
  presenter overlay, HDR, whatever macOS 27 adds) — no waiting on binding crates.
- **Crash isolation:** the recorder dying loses at most the tail of the current fragment
  (fMP4, §4); keeper notices EOF on stdout, flips the tray to error state, and offers
  restart. The messaging app never goes down.
- **Precedent, twice over:** keeper's own bbctl sidecar (launch-on-demand `exec` + parsed
  output, Story 6.7) and — the near-exact shape — **Kap's `aperture`**: a Swift CLI
  recorder (AVFoundation/SCK) driven from an Electron app over stdio, shipped publicly for
  years by Wulkano.
- **Licensing:** the helper is keeper-authored (Apache-2.0, first-party); it links only
  Apple system frameworks. cargo-deny is untouched (Swift isn't even in the tree it scans).
- **TCC:** child-process attribution means all prompts and System Settings entries say
  "keeper" (§1.3).

Costs, honestly: a SwiftPM build step in CI (Xcode is already on the macOS runners used for
signing/notarization); a protocol version handshake (`getCapabilities` carries it);
signing care — Tauri signs bundled binaries, but there is a known notarization rough edge
with `externalBin` (tauri-apps/tauri #11992): the reliable pattern is to codesign the
sidecar explicitly (hardened runtime + keeper's entitlements file) in CI before `tauri
build`. keeper ships aarch64-only, so no universal-binary lipo step is needed (Tauri would
not do it for a Swift binary anyway).

### 2.c ffmpeg sidecar with avfoundation — rejected

- The `avfoundation` input device captures *screens* and *devices*; it has **no per-app or
  per-window capture** — a hard owner requirement.
- **No system audio**: avfoundation sees input devices only; system audio requires a virtual
  loopback driver (BlackHole/Soundflower) the user must install — a support disaster and
  exactly the "driver, sudo" experience SCK exists to kill.
- **Licensing:** ffmpeg builds with libx264 are GPL; LGPL-only builds (VideoToolbox encoder)
  are possible but make the binary a bespoke build keeper must produce and maintain, and a
  GPL-adjacent blob sits badly next to the repo's deliberate Apache/MIT-only posture even
  when technically firewalled as a separate process.
- It also solves the wrong problem: encoding was never the hard part — AVAssetWriter does
  hardware H.264/HEVC natively.

---

## 3. Similar products teardown — behaviors to copy (and avoid)

| Product | Tech | Defaults & behaviors worth stealing |
|---|---|---|
| **Cap** (cap.so, open-source Loom alt) | **Tauri v2 + Rust** in-process pipeline (SCK via their crates, forked rust-ffmpeg/nokhwa/cpal, wgpu WGSL color conversion); SolidJS UI | Closest relative. Instant mode (share link) vs Studio mode (local, **screen and camera recorded as separate files**, composed at export). Their repo is the best worked example of every problem keeper will hit — including the ad-hoc-signing TCC failure (issue #1722). Their fork-heavy dependency posture is the cautionary tale that motivates the sidecar route. |
| **Kap** | Electron + **`aperture` Swift CLI recorder over stdio** | The architectural precedent for route (b). Simple selection UI, 30fps default, exports mp4/gif. |
| **Loom** | Electron desktop + cloud | 30 fps, H.264 MP4, resolution auto-fit to device; 4K on paid tiers; menu-bar control with countdown; webcam bubble burned in for instant share. Chunked/segmented capture for upload robustness — same mechanic keeper needs for local batching. |
| **CleanShot X** | Native macOS menu-bar app | The UX gold standard for keeper's scope: record area/window/full screen from one panel; mic + system-audio **toggles with device pickers remembered per mode**; webcam overlay bubble; pause button; 60 fps default with a "30 fps for tutorials" recommendation; H.264 MP4 default, HEVC option; auto-enables Do Not Disturb during recording (nice touch worth copying later). |
| **Screen Studio** | Native macOS | Records screen, camera, mic as **separate synchronized tracks** in a project bundle, effects applied post — validates the separate-webcam-file MVP. Menu-bar item shows elapsed time + stop. |
| **OBS** | Qt/C++ | System audio on macOS via SCK app/desktop audio (13+, no driver since OBS 30). Two crash-safety mechanics worth copying conceptually: record-to-MKV-remux-later (crash-safe container) and **"Automatically split output" by time or size** — the only mainstream implementation of keeper's size-batched requirement. Verbose settings UI is the anti-pattern; keeper wants CleanShot-grade simplicity. |
| **QuickTime** | System | The floor: system picker, .mov, mic-only (no system audio), **moov-at-end so a crash loses the whole file** — the failure mode keeper's fMP4 choice is designed to eliminate. |

Synthesis of defaults for keeper: **1080p-class capture at source resolution, 30 fps
(60 selectable), H.264 + AAC in fMP4 .mp4, mic = system default input, system audio on,
webcam off, segments 500 MB (configurable), files in `~/Movies/keeper` (configurable),
menu-bar state with elapsed time + Stop.**

---

## 4. Segmented / batched writing

**Requirement:** record continuously, saving batches/segments of a given file size, crash-safe.

### Mechanics

- `SCRecordingOutput` cannot rotate (one output per stream, config change stops recording)
  → keeper's helper consumes `SCStreamOutput` sample buffers and writes with
  **AVAssetWriter** directly. This also makes the 13/14 vs 15 mic difference irrelevant to
  the writer and enables the same writer for the webcam file.
- **Gapless rotation — dual-writer handover:** at a rotation boundary, start writer B
  (session start = next keyframe PTS), route buffers to both until B's first keyframe is
  written, then finalize A asynchronously. Rotation cost is memory for a second in-flight
  writer for <1 s; no dropped frames. (Time-based variant of what
  `AVCaptureFileOutput`/OBS auto-split do.)
- **Size-based trigger:** compute a rotation deadline from target bytes ÷ configured
  bitrate, then correct against actual on-disk growth (fragmented writing means the file
  size is observable while recording); rotate at the first keyframe after the threshold.
  Expose the knob as MB per segment (owner's ask), with a duration cap fallback
  (e.g. max 30 min) so low-motion recordings still rotate.

### Crash safety and format

- **Fragmented MP4** (`outputFileTypeProfile = .mpeg4CMAFCompliant`, fragment interval
  ~4 s): every fragment is durable once written; a crash/power-loss loses at most the last
  interval, and the file is playable up to the last complete fragment. On clean finalize,
  AVAssetWriter defragments into a regular MP4 — so *successful* segments are perfectly
  ordinary .mp4 files.
- Rejected alternatives: plain MP4/.mov (moov at end — total loss on crash, the QuickTime
  failure mode); `.mov` + `movieFragmentInterval` (works, but forum/field reports note moof
  handling quirks at finalize and it's the older mechanism); MKV-remux (OBS's trick —
  needs ffmpeg, rejected in §2.c).
- **Recommended container: fMP4 `.mp4`** — crash-safe while writing, universally playable
  after finalize, H.264+AAC (HEVC selectable later for HDR).

### Naming and recovery

```
<chosen folder>/
  keeper-rec 2026-07-16 14.03.28/          ← one folder per recording session
    manifest.json                          ← session id, filter, devices, segment list, status
    screen-0001.mp4  screen-0002.mp4 …
    camera-0001.mp4 …                      ← only when webcam enabled (§5)
```

Local-time, filesystem-safe, lexicographically ordered names; `manifest.json` is written
via atomic rename after every segment close and on state changes (`recording` →
`finalized`). **Recovery pass:** on keeper startup (and before a new recording), scan the
target folder for manifests still in `recording` state → mark the session `recovered` in
the manifest, surface a one-line notice ("A recording was interrupted; N segments were
saved"). The orphaned tail segment is a valid-up-to-last-fragment fMP4 — playable as-is; no
remux step in MVP (an optional "tidy" remux is a later story).

---

## 5. Webcam

- Enumeration/selection: `AVCaptureDevice.DiscoverySession` (built-in, external, Continuity
  Camera, Desk View); `NSCameraUsageDescription` in keeper's Info.plist; camera hot-plug via
  connect/disconnect notifications.
- **MVP recommendation: separate synchronized webcam file** (`camera-####.mp4`), written by
  a second AVAssetWriter in the same helper, timestamps anchored to the same host clock as
  the SCK stream, rotated at the same boundaries as the screen file. This is what Screen
  Studio and Cap's Studio mode do, and it dodges the genuinely expensive part — live GPU
  compositing (Cap needed custom wgpu/WGSL for their PiP; Loom burns in because their
  product is instant-share).
- PiP burn-in = post-MVP; a "camera bubble preview" floating window (see yourself while
  recording, without affecting the files) is a cheap intermediate polish story.
- Note in UX copy: on macOS 14+ the system's **presenter overlay** can composite the camera
  over captured content at the OS level while a camera is in use — free behavior, not a
  keeper feature.

## 6. Audio

- **System audio:** SCK `capturesAudio` (13+), scoped by the same content filter (per-app
  recording captures per-app audio), `excludeCurrentProcessAudio = true` so keeper's own
  notification sounds stay out of meeting recordings. "Speaker source" in the owner's ask
  == this toggle — it is not a device pick; SCK taps whatever the filtered content plays.
- **Microphone:** device picker from `AVCaptureDevice` audio inputs, default = system
  default input. macOS 15+: in-stream `captureMicrophone` + `microphoneCaptureDeviceID`
  (sample-accurate alignment for free); 13–14: parallel `AVCaptureSession` — same helper,
  same writer.
- **Tracks: separate, not mixed.** Write system audio and mic as **two AAC tracks in the
  screen file** (AAC-LC 48 kHz, 128–192 kbps/track). Players (QuickTime, browsers, VLC)
  play both enabled tracks; editors can pull them apart; nothing is lost the way a premix
  loses the ability to duck/remove one side later. Mixing is a later "share-friendly
  export" concern, not a capture concern.
- **Hot-unplug:** on mic disconnect mid-recording, keep video and system audio rolling,
  insert silence on the mic track, attempt fall-back to the system default input, and
  surface a warning state (tray badge + event) rather than aborting — a meeting recording
  that stops because AirPods died is worse than one with a silent gap.

## 7. Tray / menu-bar state

Existing surface: `crates/keeper/src/tray.rs` — opt-in tray (Story 10.3), single
`TrayIcon` slot guarded by a mutex, Show/Quit menu, best-effort policy, uses the app icon.
Extension plan:

- **States:** idle (current icon) → recording (template icon with record-dot variant;
  keeper ships a second tray asset) → warning/error (badge variant). Icon swap via
  `TrayIcon::set_icon`; Tauri 2 supports live menu-item text updates, so a 1 Hz tick sets
  "Recording — 12:34 (segment 3, 412 MB)".
- **Menu items while recording:** elapsed/segment line (disabled item), Stop Recording,
  (later: Pause), Open Recordings Folder, then the existing Show keeper / Quit.
- **Presence rule:** recording *materializes* the tray even when the user's opt-in tray
  toggle is off, and restores the prior state at stop — a recording indicator that isn't
  visible is a bug; this stays consistent with Story 10.3 by treating recording as a
  temporary forced-on presence with its own teardown.
- **Quit honesty (Story 10.3 pattern):** quitting keeper while recording must warn and
  finalize the current segment first — the sidecar receives `stop`, flushes, exits;
  a kill-timeout guards shutdown.
- **System indicator:** macOS itself shows the purple screen-recording pill in the menu bar
  (with the app name from Sequoia on) whenever SCK is active. keeper cannot remove it and
  should not try; keeper's own tray state still carries the value (elapsed, stop, errors).
- **Gating:** new `recording` flag in `CapabilitiesVm` (AD-27): present only on desktop
  macOS ≥ 13.0; absent → the Settings section and tray affordances never render (iOS,
  old macOS, later Windows/Linux until built).

## 8. Recommendations

### Architecture

- **Route (b): first-party Swift sidecar `keeper-rec`** (SCK + AVAssetWriter, NDJSON-RPC
  stdio), spawned launch-on-demand via `Platform::sidecar_path` / Tauri `externalBin`;
  keeper-core owns a platform-free `recording` module (state machine, manifest schema,
  settings VM, folder validation) and the shell owns spawn/stdio plumbing + tray glue —
  same split as bbctl (AD-16) and tray (AD-24).
- **Floor:** recording capability at **macOS 13.0**, runtime-gated (CapabilitiesVm);
  internal 15+ branch for in-stream mic; app minimum stays 11.0.
- **Format:** fMP4 `.mp4`, H.264 + AAC (two audio tracks), 30 fps default, source
  resolution; size-based gapless rotation with duration cap; per-session folder + manifest
  + recovery pass (§4).
- **Selection UX:** own picker UI built from `SCShareableContent` (full screen / app list)
  for a consistent flow on 13+; consider `SCContentSharingPicker` later as the 14+ path
  that also silences the monthly TCC nag.

### MVP vs later

| MVP (walking skeleton first) | Later | Never |
|---|---|---|
| Permission flow (detect/request/recover, Settings deep-links) | Pause/resume | Video editing |
| Full-screen capture + system audio + mic → segmented fMP4 in chosen folder | Per-app audio-only tweaks, HEVC/HDR | Cloud upload/share |
| Tray recording/error state + Stop | Webcam PiP burn-in, camera preview bubble | |
| App-picker capture | `SCContentSharingPicker` path, DND-while-recording | |
| Mic/camera device pickers; webcam as separate file | Orphan-segment remux "tidy" pass | |
| Disk-space guard (min-free threshold → warn, then graceful stop) | Auto-restart supervision | |

### Epic/story sketch (new epic, post-15 numbering)

1. **R.1 Walking skeleton:** `keeper-rec` sidecar (build, sign, bundle, RPC handshake) +
   permission flow + full-screen recording with system audio to a single fMP4 → chosen
   folder. Proves TCC, signing, and the pipeline end-to-end.
2. **R.2 Segmentation + manifest + recovery** (dual-writer rotation, size knob, crash test).
3. **R.3 Tray states + honest quit** (extends Story 10.3 surface).
4. **R.4 App-picker capture** (SCShareableContent UI, per-app audio scoping).
5. **R.5 Device pickers + mic track + hot-unplug resilience.**
6. **R.6 Webcam separate file** (camera capability, synchronized rotation).
7. **R.7 Polish & guards** (disk-space guard, Settings section, CapabilitiesVm gating audit,
   docs: monthly re-auth nag, purple pill, dev-signing requirement).

### Risk register

| Risk | Severity | Mitigation |
|---|---|---|
| **TCC vs ad-hoc dev builds** — macOS 15+ silently rejects SCK for ad-hoc-signed binaries; identity churn resets grants (Cap #1722) | High (DevEx) | Require Apple Development-cert signing for local dev of this feature; document in release.md; CI/e2e capture tests only on a signed runner or skipped. |
| **Sidecar signing/notarization** — Tauri `externalBin` notarization rough edge (#11992); Swift binary not lipo'd by Tauri | Medium | Explicit codesign (hardened runtime + entitlements) of `keeper-rec` in CI before `tauri build`; aarch64-only ships today so no universal step. |
| **Monthly re-auth nag (15+)** for non-picker SCK | Low/Medium | Accept + document in MVP; adopt `SCContentSharingPicker` path later; persistent-content-capture entitlement blocked by D-1 (no paid ADP). |
| **Disk exhaustion** during long recordings | Medium | Min-free-space guard: warn at threshold, graceful stop + finalize below hard floor; segment sizing makes cleanup easy. |
| **Long-run stability** (hours-long meetings): sample-buffer backpressure, writer stalls, thermal | Medium | Bounded buffer queues with drop-oldest video policy (never drop audio); fMP4 bounds data-loss; soak test story in R.2; sidecar restart recovers via manifest. |
| **Gapless rotation correctness** (A/V sync across segments) | Medium | Dual-writer handover cut on keyframes, host-clock anchored PTS; automated test: concatenate segments, assert continuous timestamps. |
| **macOS API drift** (Tahoe+ permission UX changes, macOS 27 betas) | Low | Sidecar isolates all Apple API churn in one small Swift file set; `getCapabilities` handshake lets keeper degrade gracefully. |
| **Webcam/mic device churn** (Continuity Camera appearing/disappearing) | Low | Re-enumerate on connect/disconnect notifications; never hard-fail a running recording on device loss (§6). |

### Sources (key)

- Apple: ScreenCaptureKit docs & "Capturing screen content in macOS"; WWDC22/23/24 SCK
  sessions (picker, presenter overlay, HDR, `SCRecordingOutput`); WWDC20 "Author fragmented
  MPEG-4 content with AVAssetWriter"; `movieFragmentInterval` docs.
- TCC/signing: mjtsai.com on Sequoia prompts + persistent-content-capture entitlement;
  9to5Mac (monthly nag); CapSoftware/Cap issue #1722 (ad-hoc SCK rejection);
  tauri-apps/tauri issue #11992 (externalBin notarization); Tauri v2 macOS signing guide.
- Crates: doom-fish/screencapturekit-rs (v8.0.0, 2026-06; feature/version matrix);
  CapSoftware/scap (WIP status); crates.io/screencapturekit.
- Products: CapSoftware/Cap repo + architecture writeups (Tauri v2, forked
  ffmpeg/nokhwa/cpal/wgpu, Studio-mode separate files); Wulkano Kap/aperture; CleanShot X
  features page; Loom encoding/quality support docs; OBS 30 macOS SCK audio + auto-split;
  Nonstrict "Recording to disk with ScreenCaptureKit"; fatbobman ScreenSage architecture
  post; macOS Tahoe 26 recording guides.
