# Screen recording

keeper records your screen to a folder on this Mac. **Nothing uploads** — the
recording feature adds zero network destinations (verified by an automated
egress-diff gate in CI), has no telemetry, and writes only where you point it.

## What it records

- **Screen** — the whole main display, a chosen display, or a single
  application (app-scoped capture records only that app's windows, and its
  audio scoping follows).
- **System audio** — the audio the recorded content plays, as its own track.
  keeper's own notification sounds are excluded from the recording.
- **Microphone** — your voice, recorded as its **own separate track** (never
  premixed with system audio; editors can separate them, stock players play
  them together). Pick a device or use the system default input.
- **Webcam** (optional) — your camera, recorded to a **separate file**
  (`camera-####.mov`), synced to the screen segments by a shared clock.

## Where recordings go

Each Recording Session creates one folder inside your chosen destination
(default `~/Movies/keeper`):

```
keeper-rec 2026-07-20 11.36.52/
  manifest.json      # capture target, devices, segment ledger, status
  screen-0000.mov    # H.264 + AAC (+ mic AAC) — plays anywhere
  screen-0001.mov
  camera-0000.mov    # only when the webcam is on
```

Long recordings rotate into new segments at the configured **segment size**
(default 500 MB) or **duration cap** (default 30 minutes) — the handover is
gapless (the boundary is exactly one frame period, asserted by an automated
CI gate against the manifest's capture-clock bounds). Segments are fragmented
QuickTime files: a crash or power loss costs at most the last ~4 seconds, and
an interrupted session is salvaged on the next launch ("A recording was
interrupted" — with **Reveal in Finder**).

Recordings that end cleanly show "Saved N segments" with the session path.

## Permissions (macOS)

- **Screen & System Audio Recording** — required before Start. On modern
  macOS the system does **not** show a prompt for this permission: grant it
  manually under System Settings → Privacy & Security → Screen & System
  Audio Recording. macOS may require relaunching keeper after granting. On
  macOS 15 and later the system may ask you to **re-confirm this permission
  monthly** (keeper uses the non-picker ScreenCaptureKit path).
- **Microphone / Camera** — standard system prompts appear on first use, and
  each is needed only while that source is enabled.
- macOS shows its own **capture indicator** in the menu bar while recording —
  keeper never suppresses it.

## While recording

- The in-app banner and the **menu-bar (tray) icon** show the live state:
  elapsed time, current segment and its size, Stop, and Open folder. The tray
  stays present for the whole session, and quitting keeper finalizes the
  recording honestly first.
- Failures are **loud, never silent**: a tray error state, a native
  notification, and an in-app banner with the honest reason — already-written
  segments always survive.
- **Disk guard**: below 10 GB free you get a warning; below 2 GB the
  recording stops gracefully and finalizes so everything written stays
  playable.

## For developers (dev builds)

- Real capture needs a **signed build**: sign keeper and the `keeper-rec`
  sidecar with an Apple Development certificate (a free Personal Team works).
  macOS 15+ is documented to reject ad-hoc-signed ScreenCaptureKit
  (Cap #1722); empirically on macOS 26 an ad-hoc build can capture once the
  grant is given manually, but **every ad-hoc rebuild invalidates the TCC
  grant** (the grant keys on the code signature) — a stable certificate makes
  the grant survive rebuilds.
- The hardened runtime needs the `com.apple.security.device.audio-input` and
  `com.apple.security.device.camera` entitlements
  (`src-tauri/crates/keeper/keeper-rec.entitlements`) or TCC will refuse to
  even show the microphone/camera prompts.
- Segments are fragmented **QuickTime `.mov`**, not `.mp4`, on purpose: the
  macOS 26 fragmented-MP4 muxer is permanently poisoned by wall-clock-slow
  sample delivery (an idle, static screen), failing the segment finalize with
  `-11800/-16341`. The `.mov` fragment path is healthy under the same
  traffic, and a frame-rate idle heartbeat keeps the writer dense so
  fragments keep flushing through idle stretches.
