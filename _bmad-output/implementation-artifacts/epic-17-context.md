# Epic 17 Context: Segmentation & Recovery — Hours-Long, Crash-Safe Capture

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Epic 17 turns the single-file recording skeleton (Epic 16) into hours-long, crash-safe capture. It adds gapless, size-based file rotation inside the Swift capture sidecar; a self-describing per-session folder with an atomically-written `manifest.json` and a segment ledger owned by the Rust core; a startup recovery pass that finds and marks orphaned segments left by a crash; an automated concatenate-and-assert CI gate that proves rotation stays gapless; and user-facing segment-size and duration-cap settings. The payoff for the user: a crash costs at most the last few seconds, files stay a manageable size, and every recording produces ordinary playable files an external tool can always read consistently.

## Stories

- Story 17.1: Dual-Writer Gapless Size-Based Rotation in keeper-rec
- Story 17.2: Session Folder, manifest.json & Segment Ledger
- Story 17.3: Startup Recovery of Orphaned Segments
- Story 17.4: Automated Gapless-Concat Test (NFR-22)
- Story 17.5: Segment-Size & Duration-Cap Settings

## Requirements & Constraints

- **Continuous segmented recording with size-based rotation.** Record continuously, rotating to a new segment when the current file reaches the configured segment size (default 500 MB), with a duration-cap fallback (default 30 min) so low-motion recordings still rotate. Rotation must be gapless: no pause, no dropped audio, no user-visible hiccup. The configured size must be respected within one keyframe interval of file growth. Defaults are authored assumptions — adjustable on dogfooding evidence without a spec change.
- **Session output — folder, manifest, ledger.** Each recording creates one timestamped session folder holding the segment files and a `manifest.json` describing capture target, devices, segment list, and status. Segment names are local-time-stamped, filesystem-safe, and lexicographically ordered. The manifest updates atomically at every segment close and every status change, so an external reader never sees a torn or inconsistent file. Cleanly finalized segments are ordinary `.mp4` (H.264 + AAC) playable everywhere with no keeper-specific tooling.
- **Crash safety and startup recovery.** Any interruption — recorder crash, keeper crash, power loss — must lose at most the last ~4 s fragment; every earlier segment stays untouched and playable. On startup and before each new recording, scan for interrupted sessions (stale `recording` manifests), mark them recovered, and record that a once-per-session notice should be shown (the notice UI itself is Epic 20; the live loud-failure notification is Epic 18). Recovered files play as-is with no remux step.
- **Gapless-handover release gate (NFR-22).** Rotation cuts on keyframes with continuous, host-clock-anchored timestamps. Concatenating a session's segments must yield monotonic timestamps with no gap or overlap exceeding one frame duration. An automated concatenate-and-assert test gates release, wired into the CI perf/concat harness.
- **Authored bars pending sign-off.** The one-frame alignment bound and default thresholds are authored numbers awaiting owner confirmation at phase release — not blockers for building the mechanism.

## Technical Decisions

- **Ownership split — sidecar vs. core.** The dual-`AVAssetWriter` gapless rotation lives *entirely* in the `keeper-rec` Swift sidecar. `keeper-core::recording` owns *only* the platform-free session state machine (`idle → preflight → recording → rotating → stopping → finalized | recovered | failed`), the manifest schema, the segment ledger, folder validation, and recovery reconciliation — with no Tauri and no Apple APIs. The sidecar spawn and stdio framing sit in the `keeper` shell behind a `Recorder` port; the core never holds a process handle.
- **Rotation mechanics.** Start writer B at the next keyframe PTS, dual-route to both writers until B's first keyframe lands, then finalize writer A asynchronously. The size trigger is a bytes-budget deadline corrected against observable on-disk growth, with the duration-cap as a fallback. All PTS are host-clock anchored so timestamps stay continuous across the cut.
- **Container format.** Fragmented MP4 (`.mpeg4CMAFCompliant`, ~4 s fragments) throughout recording so size is observable live and a mid-segment kill loses at most the last fragment; H.264 video plus up to two unmixed AAC tracks (system audio + microphone). A clean finalize defragments to an ordinary `.mp4`.
- **Session folder shape.** `<folder>/keeper-rec <local timestamp>/` holds `manifest.json`, `screen-####.mp4` segments (and `camera-####.mp4` when webcam is on — an Epic 20 concern). The manifest is fed from the sidecar's `segmentClosed{path, bytes, track}` and `state` events.
- **Atomic manifest writes.** Every manifest update is written by atomic rename so an external reader never observes a partial file. Status transitions `recording → finalized` on clean Stop, and stale `recording` → `recovered` on recovery.
- **Sidecar wire contract.** NDJSON (one JSON object per line) over stdio. The `start` command carries `{filter, audio, mic, camera, dir, segmentMB, fps, …}`; the sidecar emits `segmentClosed`, `state`, and `error` events. Contract shape is the invariant; exact field lists are code-owned.
- **Recovery is remux-free.** The orphaned tail fMP4 plays as-is; no remux in this phase. Recovery is the safety net, distinct from the live failure notification.
- **CI signing constraint.** The concat-assert test runs against committed fixture segments (or output produced on the signed runner) so gaplessness is gated without depending on a physical capture. The induced-kill / force-kill test also runs against committed fixture output. The concat harness extends existing CI measurement hooks and leaves a screen↔camera one-frame-alignment assertion hook for Epic 20's webcam files.
- **Settings live in Rust.** Segment size and duration-cap persist in `keeper.db` behind `keeper-core::settings` — no `tauri-plugin-store` or `tauri-plugin-sql`. Values are exposed via commands + a settings stream and passed to the sidecar on `start`. Changing a setting affects future sessions only.

## UX & Interaction Patterns

- **Two setting surfaces mirror each other.** Settings → Recording and the pre-record setup card (the "Destination + Segmenting" section) show the same segment-size stepper (default 500 MB) and duration-cap fallback field (default 30 min). Editing either surface writes the same underlying value; changes affect the next session only, never the running one.
- **Live segment feedback.** A segment meter fills toward the configured segment size ("segment N · 412 / 500 MB") and resets at each gapless rotation — the reset is the only user-visible signal that rotation happened.
- **Copy conventions.** Sentence case; "Recording Session" and "segment" are capitalized per the glossary.

## Cross-Story Dependencies

- **Within the epic:** 17.1 (rotation) is the base. 17.2 (folder/manifest/ledger) depends on 17.1's `segmentClosed`/`state` events. 17.3 (recovery) depends on 17.2's manifest. 17.4 (concat gate) depends on 17.1 + 17.2. 17.5 (settings) depends on 17.1 + 17.2.
- **Upstream:** the whole epic builds on Epic 16 (single-file capture, sidecar, `Recorder` port, session state machine seed).
- **Downstream:** Epic 18 consumes segment info for the tray/banner live line and owns the live loud-failure notification. Epic 20 owns the once-per-session recovery notice UI and populates the webcam (`camera-####`) alignment hook the concat harness leaves open. Epic 19 owns the destination folder chooser UI.

## Coordinator guidance for the 17-3 retry (2026-07-18)

The first 17-3 attempt was deferred by review-budget non-convergence. The
reviewer's four concrete findings are recorded in `deferred-work.md` (entries
sourced from spec-17-1/17-2/17-3) — the retry spec MUST fold them in up front:

1. Salvage runs `SessionManifest::reconcile_from_dir` (authoritative rebuild
   from on-disk `.mp4`s), never a mere status flip to `recovered`.
2. The 17.1 stop-during-rotation `segmentClosed` suppression means an event-fed
   ledger can miss a fully-written segment — reconcile-from-disk covers it.
3. `recording_base_dir`: lazy fallback (`unwrap_or_else`), not eager
   `unwrap_or(platform.data_dir()?)`.
4. Keep the pre-record recovery scan bounded / off the hot path where cheap.

Story 20.3 (completion + recovery notice) is blocked on this story — its FR-73
leg consumes the `recovered` manifests this story produces.
