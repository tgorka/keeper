# Epic 21 context — Recording Ergonomics

Owner-requested increment (2026-07-21) after v0.2.0 shipped. Five stories:
codec (21.1), scale (21.2), audio-only (21.3), template tray (21.4), session
meta (21.5). All wire changes are ADDITIVE on protocol v1 (precedent: 16.5's
requestScreenRecording, 16.6's startRecording/stop, 17.4's segmentClosed pts
bounds, 19.x/20.x device params).

## Load-bearing platform facts (hard-won in 16-20 — do not re-litigate)

- Segments are fragmented QuickTime **.mov**, NOT .mp4: the macOS 26
  fragmented-.mp4 muxer is permanently poisoned by wall-clock-slow sample
  delivery (idle screens) — finishWriting fails -11800/-16341. Keep .mov for
  every new path, including 21.3's audio (`audio-####.m4a` is fine — audio
  flows continuously from the mic; system-audio-only idle silence was healthy
  on .mov in the 20.5 bisect, but TEST an idle rotation before review).
- The 30 fps idle **heartbeat** re-appends the last video frame when SCK goes
  quiet; 21.2 must re-append the SCALED frame; 21.3 audio-only sessions have
  no video heartbeat — verify idle rotation empirically (1-min cap, hands off).
- A PTS **monotonic guard** drops samples racing a heartbeat stamp — keep it.
- TCC/hardened runtime: mic needs `com.apple.security.device.audio-input`,
  camera `...device.camera` (already in keeper-rec.entitlements + tauri.conf
  bundle.macOS.entitlements). 21.3's audio-only-with-system-audio still needs
  the Screen Recording grant (SCK requirement) — the pre-flight must say so
  honestly; mic-only needs no Screen Recording.
- Live capture tests need the DEV-SIGNED build on this Mac; automated stories
  ship compile/unit gates + the 17.4 concat gate (extend it to HEVC in 21.1).
- The stop-during-rotation false "no frames" outcome and display-sleep
  teardown are OPEN deferred-work items — do not regress them; fixing them is
  in-scope opportunistically for whichever story touches that code.

## Tauri/tray notes (21.4)

- Tray icons live in the Epic 18 tray module; Tauri 2 `TrayIcon` supports
  `set_icon_as_template(true)` — template images must be monochrome+alpha
  PNGs. Recording-state distinction must come from glyph SHAPE, not color.

## Session meta (21.5)

- Manifest is owned by keeper-core (`SessionManifest`); adding `meta` +
  `startedAt`/`endedAt` (ISO-8601 with offset, via chrono already in the
  shell) is additive — recovery/reconcile must tolerate their absence in old
  manifests. Folder naming: host-side (ipc.rs builds the session dir); title
  sanitization strips `/:\0` and trims; keep Unicode.
