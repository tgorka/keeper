---
status: done
story: '20.5'
slug: '20-5-reliability-envelope-4-h-soak-and-cpu-memory-verification'
resolution: coordinator (human-in-the-loop hand-off, as planned)
---

# Story 20.5 — Reliability Envelope: 4 h Soak & CPU/Memory Verification

Coordinator-executed on reference hardware (Apple Silicon, macOS 26.5.2,
Apple-Development-signed build), 2026-07-20.

## Soak result (NFR-19): PASS

Configuration per AC: full main display (1440×900 @ 30 fps), system audio ON,
microphone ON (separate track), segment budget 500 MB / 30 min cap, display
held awake (`caffeinate -d`), real desktop workload (active use + long idle
stretches).

- **4 h 05 m continuous** → session `keeper-rec 2026-07-20 11.36.52`,
  **status `finalized`**, 13 segments (12 byte-budget rotations at 501 MB + a
  309 MB final), 6.32 GB total, every segment independently playable
  (H.264 + AAC + mic AAC, 13–23 min each).
- **Zero recorder crashes, zero writer stalls, zero failed rotations.**
- **Gapless across every boundary**: manifest host-clock bounds show
  `ptsStart(k+1) − ptsEnd(k) = 0.033 s` (exactly one frame period) on all 12
  cuts — NFR-22's contract observed in production, not just in the CI gate.

## Performance envelope (NFR-21): PASS with large headroom

489 samples at 30 s cadence over 4.08 h (`soak3-telemetry.csv`):

| Metric | Measured | Authored bar |
|---|---|---|
| CPU combined (keeper + keeper-rec), avg | **14.2 %** of one core | < 100 % |
| CPU combined, max | 19.6 % | — |
| RSS combined, avg / max | **65 MB / 100 MB** | < 400 MB |
| RSS trend (quartile means) | 80 → 64 → 60 → 57 MB (shrinking) | no unbounded growth |

keeper's messaging loops stayed live throughout (keeper avg 2.4 % CPU — sync,
UI and tray all responsive; no NFR-1..4 regression observed in use).

## Findings fixed en route (the soak earned its keep)

Two soak attempts failed before the pass, exposing a real platform defect:
on macOS 26 the fragmented **.mp4** muxer is permanently poisoned by
wall-clock-slow sample delivery (static screen → SCK stops delivering; paced
1–5 fps append is fatal to a later `finishWriting` (-11800/-16341), while the
same samples delivered fast are fine). Bisected with a standalone
AVAssetWriter repro, no SCK involved. Fixes shipped as `ad0e5a3`:

1. Segments moved to **fragmented QuickTime `.mov`** (same H.264/AAC; the
   .mov fragment path is healthy under sparse traffic).
2. A **30 fps idle heartbeat** re-appends the last frame when SCK goes quiet,
   keeping the writer wall-clock-dense and fragments flushing through idle
   (crash now loses ≤ 1 fragment even on a static screen — AD-37 restored),
   with a PTS monotonic guard and an explicit `endSession` at each cut.
3. Hardened-runtime entitlements fix (`39ac532`): `audio-input` + `camera`
   are required for the mic/camera prompts under a Development-signed build.

Costs measured: the heartbeat adds ~8 pp CPU on keeper-rec during idle
(11.8 % avg vs ~3.7 % pre-fix) — well inside the envelope.

## Owner confirmation (PRD §14.7 open #1)

Authored bars confirmed as release gates on this evidence: NFR-19 = 4 h
zero-crash soak; NFR-21 = < 100 % core avg CPU, < 400 MB combined RSS.

Open (deferred-work, for sweep): display-sleep session teardown semantics;
stop-during-rotation false "no frames" outcome.
