---
status: done
story: '20.6'
slug: '20-6-sm-9-sm-10-phase-acceptance-docs-recording-md-and-retrospective-inputs'
resolution: coordinator (human-in-the-loop hand-off, as planned)
---

# Story 20.6 — SM-9 / SM-10 Phase Acceptance, docs/recording.md & Retrospective

## SM-9 end-to-end gate (device, dev-signed, macOS 26.5.2)

- [x] Permission pre-flight (granted state, live re-detection) — verified across
      the whole phase; grant survives rebuilds under the Apple Development cert.
- [x] Full-screen + system audio + microphone recording — 4 h soak (20.5).
- [x] Segments rotate at the configured size into the chosen folder with a valid
      manifest — 12 × 501 MB byte-budget rotations, gapless (0.033 s boundaries).
- [x] Induced crash recovers per FR-73 — `kill -9` of keeper-rec mid-segment:
      loud failure triad with the honest reason ("signal: 9 (SIGKILL)"),
      orphaned 52 MB segment playable, next launch reconciled the manifest
      `recording → recovered` and showed the recovery notice with
      Reveal in Finder.
- [x] App-scoped recording + webcam separate file — live run 2026-07-20
      19:18: captureTarget `com.apple.finder` (honest scoping note shown),
      separate `camera-0000.mov` (35 MB) beside `screen-0000.mov`, both in the
      ledger, session finalized, camera file opens in QuickTime.

## SM-10 reliability

- [x] NFR-19 soak green (20.5): 4 h 05 m, zero crashes/stalls, RSS shrinking.
- [x] Induced-failure matrix, loud in 100% of runs, prior segments intact:
      - recorder kill → loud triad + recovery ✓ (live)
      - disk floor → warn at 10 GB, graceful stop-and-finalize at 2 GB ✓ (live,
        real disk fill; completion card "Saved 1 segment · 184 MB")
      - mic unplug → covered at unit/simulation level (19.4 suite; no physical
        external mic available on the reference machine) — honest partial
      - permission revoke mid-record → live run 2026-07-20 19:26: macOS 26
        enforces the revoke at the NEXT stream start, not against the live
        session — the in-flight recording continued and finalized cleanly
        (zero loss, 176 MB intact), and the pre-flight re-detected the revoke
        immediately after (Start disabled, "needs the Screen Recording
        permission"). Loud + honest; no silent-loss path exists.
- [x] Zero silent recording-loss incidents during phase dogfooding (all losses
      were loud; the two pre-fix soak failures surfaced via the triad).
- [x] NFR-11 egress diff for the phase is empty — automated zero-egress audit
      shipped in 20.4 runs in CI.

## docs/recording.md

- [x] Written (`docs/recording.md`): dev-signing requirement + Cap #1722
      (with the empirical macOS 26 nuance), monthly re-auth nag, untouched
      purple pill, disk-guard/segment/folder defaults — 1:1 with the in-app
      disclosure strings; English; no credentials.

## Retrospective inputs (vs PRD §14.6 risk register)

- **TCC / ad-hoc signing**: worse than planned on macOS 26 — no OS prompt at
  all (authReason=5 service policy); manual System Settings grant only; ad-hoc
  rebuilds invalidate the grant (cdhash-keyed). Mitigated by Apple Development
  signing (grant survives rebuilds). Docs updated.
- **Sidecar notarization**: not needed for local dev-signed builds; CI release
  path signs with entitlements (audio-input + camera — REQUIRED under the
  hardened runtime for the prompts to even appear; found live in 20.5).
- **Monthly nag**: honest note shipped in-app + docs; not automatable.
- **Disk exhaustion**: guard verified live (warn 10 GB, graceful stop 2 GB).
- **Long-run stability**: 4 h soak green after TWO real defects were found and
  fixed by the soak itself — (1) macOS 26 fragmented-.mp4 muxer poisoned by
  wall-clock-slow delivery → segments moved to fragmented QuickTime .mov;
  (2) idle screens starved the writer → 30 fps idle heartbeat (also restores
  the ≤1-fragment crash-loss bound on static screens).
- **Gapless rotation correctness**: manifest host-clock bounds (17.4 amendment)
  proved 0.033 s boundaries across all 12 production rotations.
- **API drift**: SCK `.microphone` output (macOS 15+) vs 13–14 AVCapture mic
  path both exercised; `excludesCurrentProcessAudio` naming drift caught at
  compile time.
- **Device churn**: mic hot-unplug at sim level; camera loss path shipped
  (20.1) with early-finalize semantics.

Deferred-work ledger opened this phase (for the retrospective): pause/resume,
webcam PiP burn-in + self-view, SCContentSharingPicker path, HEVC/HDR,
DND-while-recording, orphan-segment remux, in-app recordings browsing,
Windows/Linux recording, display-sleep teardown semantics,
stop-during-rotation false "no frames", recovery-notice copy plural ("1
segment were saved"), review budget non-convergence pattern (17-3 first
attempt), CLI session expiry stalling unattended dev sessions.
