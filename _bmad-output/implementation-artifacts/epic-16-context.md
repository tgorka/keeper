# Epic 16 Context: Recording Walking Skeleton — Sidecar, Permissions, Capture to File

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

This epic proves the entire screen-recording vertical slice on the locked recording architecture before any feature breadth is added, deliberately retiring the three existential risks named first in the planning: macOS TCC permissions, sidecar code-signing, and capture-to-file. keeper spawns a first-party Swift capture sidecar (`keeper-rec`), gates all recording surfaces behind a `recording` capability that is true only on desktop macOS ≥ 13.0, negotiates an NDJSON-RPC handshake, runs an honest Screen Recording permission pre-flight, and records a full display plus system audio to a single fragmented MP4 in a chosen folder — all driven from a ⌘5 Recording view with Start/Stop and a live elapsed line. The exit gate is a real recording that plays back on dev-signed hardware; passing it seeds the phase-level SM-9 acceptance and unblocks segmentation (Epic 17), tray/failure surfacing (Epic 18), and source breadth (Epic 19).

## Stories

- Story 16.1: keeper-rec SwiftPM Scaffold, Codesign & externalBin Wiring
- Story 16.2: recording Core Module & Recorder Port
- Story 16.3: recording Capability Flag & Gated Recording Surface
- Story 16.4: NDJSON-RPC Handshake — getCapabilities & listSources
- Story 16.5: Screen Recording Permission Pre-flight
- Story 16.6: Full-Screen + System-Audio Capture to a Single fMP4

## Requirements & Constraints

- Recording is macOS-only and desktop-only. Every recording surface (Recording view entry, Settings section, palette actions) renders only when the `recording` capability is on; that flag is true solely on desktop macOS ≥ 13.0 (the system-audio floor). The app-wide `minimumSystemVersion` stays 11.0; iOS never records. Absent capabilities are removed, never shown as dead buttons.
- The permission pre-flight must be honest: Screen Recording state is live-detected at render (never cached optimistically) and re-detected on focus/return, tracked distinctly as granted / not-yet-requested / denied-with-fix-path. Where the OS allows, request via the system prompt; otherwise deep-link to the exact System Settings pane. Start is disabled until the required grant is green, naming the blocking permission. Disclose the honest quirks: relaunch-may-be-needed after grant, and the macOS 15+ monthly re-confirm.
- End-to-end capture target for the epic: a full display with its system audio, written as a single fragmented MP4 (H.264 + one AAC system-audio track, ~4 s fragments) to a chosen folder; on Stop it finalizes (defragments) to an ordinary `.mp4` that plays back with continuous A/V. keeper's own notification sounds must be excluded from the captured audio. Only one capture target per session.
- Local-only invariant: recording adds zero new network destinations. No upload/share/transcription/cloud affordance anywhere.
- The full skeleton cycle (pre-flight → full-screen + system-audio capture → single playable fMP4 → clean Stop) must run in a real build on macOS 13+ hardware.
- Dev-signing reality (a DevEx constraint, not a product blocker): local builds that exercise real capture require an Apple Development certificate, because macOS 15+ silently rejects ScreenCaptureKit for ad-hoc-signed binaries. Story 16.6 is therefore human-in-the-loop (physical Mac + real grant + dev-signed build); the automation loop defers it to the coordinator rather than escalating.

## Technical Decisions

- **Split of ownership.** `keeper-core::recording` owns the session state machine (`idle → preflight → recording → rotating → stopping → finalized | recovered | failed`), platform-free, with no `tauri` and no Apple API anywhere in its tree (enforced by a dependency/unit-test check). Errors flow `thiserror` → `CoreError`. The core state machine never holds a process handle — the shell port parses sidecar events and feeds them in.
- **Recorder port.** A `Recorder` trait sits beside the existing `Platform` port. The macOS impl (`crates/keeper/src/recorder.rs`, `#[cfg(desktop)]`) spawns `keeper-rec` via `Platform::sidecar_path`; every non-macOS impl and iOS returns `CoreError::Unsupported`.
- **Sidecar package.** `keeper-rec` is a SwiftPM package at top-level `tools/keeper-rec/` (`Package.swift` + `Sources/keeper-rec/`), deliberately outside `src-tauri/crates/` so Cargo and SwiftPM tooling don't collide. Apache-2.0, linking only Apple system frameworks (ScreenCaptureKit / AVFoundation) — no ffmpeg, so the licensing/cargo-deny gate is untouched.
- **Build & sign.** CI on the existing macOS signing runner runs `swift build -c release --arch arm64`, then explicitly codesigns `keeper-rec` (hardened runtime + keeper's entitlements) before `tauri build` (the externalBin notarization rough edge). aarch64-only, no lipo. `bundle.externalBin = binaries/keeper-rec`; Tauri appends the triple so the runtime name resolves to `keeper-rec-aarch64-apple-darwin`, matching what `sidecar_path` expects.
- **Wire protocol (NDJSON-RPC).** One JSON object per line on stdio. Host→sidecar commands include `getCapabilities` (id-correlated; returns macOS version + feature flags + per-TCC permission states, and carries the protocol-version handshake), `listSources`, `start`, `stop`. Sidecar→host events include `state`, `segmentClosed`, `error`. A version mismatch yields a clean unsupported/error surface, never a crash. The contract *shape* is the invariant; exact field lists stay code-owned and are surfaced as generated (ts-rs) VMs.
- **Capability & permission VMs.** `CapabilitiesVm` gains a `recording` flag (serde + ts-rs). A `RecordingPermissionVm` (`keeper-core::vm`, ts-rs) tracks the Screen Recording TCC class using `CGPreflightScreenCaptureAccess` (detect) / `CGRequestScreenCaptureAccess` (request); the deep-link target is `x-apple.systempreferences:…Privacy_ScreenCapture`. Usage strings live in keeper's bundle `Info.plist`. The sidecar is spawned as a child process (never a LaunchAgent) so TCC attributes the grant to keeper. The frontend never sniffs `navigator.userAgent` or build flags — capability comes over the IPC handshake.
- **Capture implementation (16.6).** `keeper-rec` builds an `SCContentFilter` over `SCShareableContent`, captures with `capturesAudio` + `excludeCurrentProcessAudio = true`, and writes the fragmented MP4 via AVAssetWriter.
- **Testability without hardware.** The state machine is unit-tested through a full lifecycle with a fake `Recorder`; stdio framing and event parsing (`state`, `segmentClosed`, `error`) are unit-tested against a recorded fixture stream. This keeps everything except 16.6 implementable behind compile gates, unit tests, and stub sidecars.

## UX & Interaction Patterns

- **Recording view (⌘5).** A single non-chat utility surface (no timeline, no composer, no chat list), living beside Bridges and Settings — not in the inbox. Centered single column at `content-max-width`, not a pane frame. Its sidebar entry appears only when the capability flag is on and carries a `recording-dot` while capture is live. This epic ships the empty shell: a stack of setup cards (Source / Audio / Webcam / Destination / Segmenting / Advanced-fps placeholders), plus the active-recording state reached in 16.6.
- **Active-recording indication.** When recording, the view flips to *active* showing a `recording-red` record dot and a `mono` elapsed line that ticks; macOS posts its own purple capture pill in parallel (left untouched).
- **recording-red token.** The live-capture color (`#E5322D`) is used only as a live indicator (record dot, active-recording banner edge, tray badge, loud-error banner) — deliberately warmer/brighter than the destructive/disconnected colors so a live indicator never reads as a delete button. Never on buttons, text, hovers, or decoration, and it never shares a surface with the destructive color.
- **Permission pre-flight rows.** One permission row per required permission (Screen Recording always in this epic), live-detected at render and re-detected on focus/return. Honest `note-line`s state the macOS quirks plainly, including the subtle dev-facing "ad-hoc dev builds may be blocked on macOS 15+ — sign with an Apple Development certificate."
- **Recording voice.** Sentence case, no exclamation marks, Glossary-capitalized "Recording Session" / "segment"; honest local-only framing (e.g. "Recorded locally. Nothing uploads.").

## Cross-Story Dependencies

- 16.1 has no dependencies and unblocks the rest: 16.2 depends on 16.1; 16.4 depends on 16.1 + 16.2; 16.3 depends on 16.2; 16.5 depends on 16.3 + 16.4; 16.6 depends on 16.5.
- 16.6 is human-in-the-loop and is the epic exit gate (R.1 / SM-9 seed) — it needs a physical Mac, a real Screen Recording grant, and an Apple Development-signed build.
- The `Recorder` port reuses the existing `Platform::sidecar_path` mechanism established by the bbctl sidecar precedent, and the tray recording states (deferred to Epic 18) extend the existing opt-in tray. This epic is the foundation that Epics 17 (segmentation/recovery), 18 (tray/loud failures), 19 (sources/devices), and 20 (webcam/polish/phase acceptance) build upon.
