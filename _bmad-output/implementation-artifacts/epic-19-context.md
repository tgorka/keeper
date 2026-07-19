# Epic 19 Context: Sources & Devices — Choose What and Whom to Capture

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Give the user real control over what and whom they capture. Building on the walking-skeleton recorder (Epic 16), segmentation/recovery (Epic 17), and the tray/loud-failure surface (Epic 18), this epic adds the pre-record setup surface: a live application/window/display picker, an app-scoped system-audio toggle, a microphone written as its own track with hot-unplug resilience, a destination-folder chooser with validate-on-Start, and a collapsed Advanced fps control. It turns "record the whole screen" into "record exactly this source, this audio, this mic, to this folder" — with honest inline disclosure of what is and isn't captured, and never a hung or silently-lost recording. This is the sources/devices leg of the macOS Screen Recording phase (research R.4 + R.5).

## Stories

- Story 19.1: Application/Window Picker — SCShareableContent Live List
- Story 19.2: System-Audio Toggle & Per-App Audio Scoping
- Story 19.3: Microphone Picker & Separate Track
- Story 19.4: Microphone Hot-Unplug Resilience
- Story 19.5: Destination-Folder Chooser & fps Advanced Control

## Requirements & Constraints

- **Source selection is live and single-select.** The picker lists Displays first, then Applications, each with name and icon, re-enumerating as apps launch and quit. Exactly one capture target per Recording Session; on multi-display setups each display is individually selectable.
- **App-scoped capture is exclusionary and disclosed.** When one application is chosen, only that app's windows and audio land in the file — keeper itself, other apps, and incoming notification banners never appear. This exclusion must be disclosed inline ("only {App}'s windows and audio are recorded — keeper, other apps, and notification banners stay out of the file").
- **A vanished source fails cleanly at Start.** A display/app that disappeared before the user presses Start must produce a clear inline error, never a hung or hanging recording.
- **System audio is content-audio, not a device pick.** Default on; labelled as "the audio the recorded content plays." Scoped to the same capture target with keeper's own process audio excluded, so notification sounds never leak in.
- **Each enabled audio source is its own AAC track, never premixed.** System audio and microphone are written as separate tracks so stock players (QuickTime, browsers, VLC) play them together while editors can separate them.
- **Microphone defaults to system default input; permission is lazy.** Microphone permission is requested only when the mic source is enabled — never preemptively.
- **Mic hot-unplug never aborts.** If the mic is unplugged mid-recording, video and system audio keep rolling, the mic track continues silence-filled, keeper attempts fallback to the system default input, and a persistent (non-dismissible) warning state is raised on the tray and in-app banner. Device churn triggers re-enumeration on device notifications.
- **Destination is validated on Start.** A folder chooser defaults to the remembered `~/Movies/keeper`; a validate-on-Start check (exists, writable, adequate free space per the disk-guard policy) blocks start with actionable errors.
- **fps is an advanced control.** 30 default, 60 selectable, in a collapsed Advanced group; passed to the sidecar on start.
- **Settings persistence and session scope.** Folder and fps persist in the DB behind the settings module, mirror Settings → Recording, and changing either affects the next session only.
- **Local-only, zero new egress.** Nothing in this epic adds a network destination; no upload/share/transcription/cloud affordance may appear anywhere in the recording UI.

## Technical Decisions

- **Platform split (AD-33).** The platform-free `keeper-core::recording` module owns the session state machine, manifest, segment ledger, folder validation, and recovery — no `tauri`, no Apple APIs. The actual sidecar spawn and stdio framing live in the shell behind a `Recorder` port (a trait beside `Platform`); non-macOS impls return `Unsupported`. Folder validation (the destination check for 19.5) belongs in core, not in Swift.
- **Sidecar contract (AD-34).** The first-party Swift sidecar `keeper-rec` (ScreenCaptureKit + AVAssetWriter) is spawned launch-on-demand and speaks NDJSON-RPC over stdio (one JSON object per line). Relevant commands: `listSources` (displays, apps, mics, cameras) and `start{filter, audio, mic, camera, dir, segmentMB, fps}`. Events include `state{recording, elapsedSec, segmentIndex, bytes, warning}` and `error{code, message, fatal}`. The contract shape is the invariant; exact field lists are code-owned.
- **Source capture via SCContentFilter (AD-34/AD-37).** App-scoping and display selection are expressed as an `SCContentFilter` inside `keeper-rec`. The picker is fed by `SCShareableContent` enumeration, re-run on app launch/quit.
- **System audio scoping (FR-69).** Captured via `capturesAudio` scoped by the same `SCContentFilter` with `excludeCurrentProcessAudio = true`, so keeper's own audio is excluded; written as its own AAC track (48 kHz), never premixed.
- **Microphone across OS versions (AD-36/FR-69).** In-stream `captureMicrophone` on macOS 15+, a parallel `AVCaptureSession` on macOS 13–14 — same writer either way, invisible to the user and to the capability flag. Mic is a second, unmixed AAC track. Mic permission (`NSMicrophoneUsageDescription`) is probed and requested only when the source is enabled.
- **Settings ownership (AD-25).** Folder and fps live in `keeper.db` behind `keeper-core::settings`, exposed via commands + a settings stream (no JS-writable store, no tauri-plugin-store/sql).
- **Recorder never holds a process handle in core.** The `Recorder` port parses sidecar events and feeds them into the core state machine.
- **Warning states are persistent, never toast (AD-39).** The mic-unplug warning must reach the tray error/warning state and the in-app banner; every fault surfaces loudly. No recording fault is silent — every session reaches finalized / recovered / failed.
- **Simulated-signal testability.** The mic never-abort behavior is validated via a simulated device-removal signal; real Continuity-Camera/mic hardware churn is deferred to phase acceptance (Story 20.6). Destination free-space checks are testable via a simulated low-free-space signal without physically filling a disk.

## UX & Interaction Patterns

- **The Recording view is a utility, not a conversation** — no chat list, timeline, or composer. It flips in place between setup and active states; setup is a centered single column of shadcn `Card` sections: Source / Audio / Webcam / Destination / Segmenting / collapsed Advanced (fps). Every surface renders only behind the `recording` capability flag.
- **Source picker** — a scrollable list of `source-picker-row` (44px, radio semantics), grouped under "Displays" then "Applications" section headers; each row shows a leading glyph (monitor for displays, app icon for apps) + name. A subtle "refreshing…" affordance shows during re-enumeration. Single-select.
- **Audio card** — a `Switch` for System audio (default on, labelled as content-audio not a device); a `device-picker` (shadcn `Select`) for the microphone with "System default input" always first. A toggle-off greys its picker with a helper caption. Copy states plainly that system audio and mic are separate tracks, not a mix, and that keeper's own notification sounds are excluded.
- **Destination + Segmenting** — folder chooser showing the remembered `~/Movies/keeper`; fps lives in a collapsed Advanced group (30 default, 60 selectable). Changing values here mirrors Settings → Recording and affects the next session only.
- **Warning surface for mic unplug** — the `active-recording-banner` warning variant (amber left edge, persistent line, never auto-clears) plus the tray warning badge. This is the loud, non-dismissible warning surface reused from Epic 18.
- **Copy voice** — sentence case, no exclamation marks, no "please" in errors; "Recording Session" and "segment" capitalized. Inline app-scope disclosure and "Recorded locally. Nothing uploads." belong in the setup surface. `Esc` never stops a recording; stopping is always explicit (destructive-by-omission guard).

## Cross-Story Dependencies

- **Prerequisites:** Epic 16 (recorder walking skeleton, `Recorder` port, `getCapabilities`/`listSources` probe, permission pre-flight mechanism) and Epic 17 (segmentation + settings module) underpin the whole epic. Epic 18 provides the tray/banner warning surface that Story 19.4 raises into.
- **Intra-epic ordering:** 19.1 (picker) → 19.2 (system audio scoping, builds on the same `SCContentFilter`) → 19.3 (mic separate track) → 19.4 (mic hot-unplug resilience, depends on 19.3 and Epic 18's warning surface). 19.5 (destination + fps) depends on Epic 17's settings and 19.1.
- **Downstream consumers:** Epic 20 reuses this epic's `device-picker` pattern for the webcam (20.1) and its per-source permission model for the Mic/Camera pre-flight rows (20.2). The mic hot-unplug behavior and app-scoped source selection feed the SM-9/SM-10 phase-acceptance and induced-failure matrix (Story 20.6).
