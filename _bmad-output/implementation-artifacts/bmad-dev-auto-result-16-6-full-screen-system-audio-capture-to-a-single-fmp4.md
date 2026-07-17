---
status: blocked
---

# BMad Dev Auto Result

Status: blocked

Blocking condition: human-in-the-loop — deferred to coordinator (not an escalation)

## Story

- Key: `16-6-full-screen-system-audio-capture-to-a-single-fmp4`
- Title: Story 16.6 — Full-Screen + System-Audio Capture to a Single fMP4
- Epic: 16 (Recording Walking Skeleton — Sidecar, Permissions, Capture to File)

## Why this HALTs (not a spec problem, not an escalation)

Story 16.6 is one of exactly three stories in the recording phase deliberately
marked **human-in-the-loop**, to be **deferred to the coordinator rather than
escalated**. This is stated verbatim across the frozen planning artifacts:

- `epics.md:29` — "exactly three stories are explicitly human-in-the-loop —
  **16.6** (first real capture on dev-signed hardware), 20.5 …, 20.6 … — because
  macOS 15+ silently rejects ScreenCaptureKit for ad-hoc-signed binaries
  (Cap #1722), so real capture requires an Apple Development certificate and a
  physical Mac; the automation loop defers those three to the coordinator rather
  than escalating."
- `epics.md:2588` — Story 16.6 "**Human-in-the-loop: yes** — requires a physical
  Mac, a real Screen Recording grant, and an **Apple Development-signed build**
  (macOS 15+ silently rejects ad-hoc ScreenCaptureKit, Cap #1722). This is the
  recording phase's first device step; the automation loop defers it to the
  coordinator instead of escalating."
- `epics.md:3125` — 16.6, 20.5, 20.6 "all explicitly marked so the automation
  loop defers them to the coordinator; every other recording story is
  implementable with compile gates, unit tests, stub sidecars …".
- `epic-16-context.md:25` — "Story 16.6 is therefore human-in-the-loop (physical
  Mac + real grant + dev-signed build); the automation loop defers it to the
  coordinator rather than escalating."
- `epic-16-context.md:49` — "16.6 is human-in-the-loop and is the epic exit gate
  (R.1 / SM-9 seed) — it needs a physical Mac, a real Screen Recording grant, and
  an Apple Development-signed build."

Every acceptance criterion in `epics.md:2590–2603` is physically un-automatable in
this unattended session. They require, on **real hardware with a real grant and a
dev-signed build**: `keeper-rec` building an `SCContentFilter` over
`SCShareableContent` and capturing with `capturesAudio` + `excludeCurrentProcessAudio`
to a single fragmented MP4 (H.264 + one AAC system-audio track, ~4 s fragments); the
Recording view flipping to *active* with a `recording-red` dot and a ticking `mono`
elapsed line while macOS posts its own purple pill; a clean Stop that finalizes
(defragments) to an ordinary `.mp4` that **plays back in QuickTime with continuous
A/V and keeper's own notification sounds absent**; and the full walking-skeleton
cycle running end to end. There is no CI, simulator, or ad-hoc-signed substitute:
macOS 15+ silently rejects ScreenCaptureKit for ad-hoc-signed binaries (Cap #1722),
which is precisely why 16.1–16.5 were built compile-first / stub-sidecar and 16.6 is
the single device leg.

This is NOT a `bad_spec` / `intent_gap` / contradiction condition, so it must not be
routed to `bmad-loop-resolve`. It is a clean hand-off: the spec is coherent and its
dependency is satisfied — only a human with a physical Mac, an Apple Development
certificate, and a real Screen Recording grant can execute it. This mirrors the
prior `bmad-dev-auto-result-12-6-…` hand-off for the analogous iOS on-device gate.

## Readiness of the gate

The single dependency is satisfied (per `sprint-status.yaml`):

- 16.5 Screen Recording Permission Pre-flight — done

And the full skeleton stack it rides on is in place:

- 16.1 keeper-rec SwiftPM Scaffold, Codesign & externalBin Wiring — done
- 16.2 recording Core Module & Recorder Port — done
- 16.3 recording Capability Flag & Gated Recording Surface — done
- 16.4 NDJSON-RPC Handshake — getCapabilities & listSources — done

The Epic 16 exit gate (R.1 / SM-9 seed) is therefore **ready for the coordinator to
run on dev-signed macOS 13+ hardware**.

## What the coordinator (human owner) needs to do

On a physical Mac (macOS ≥ 13.0) with an **Apple Development-signed** build and a
real Screen Recording grant, implement the capture leg in `keeper-rec` and run the
walking-skeleton cycle, recording results back into the story / `docs/recording.md`:

1. Implement capture in `keeper-rec`: build an `SCContentFilter` over
   `SCShareableContent` for a full display; capture with `capturesAudio` +
   `excludeCurrentProcessAudio = true`; write a single fragmented MP4 (H.264 + one
   AAC system-audio track, ~4 s fragments) via `AVAssetWriter` to the chosen folder
   (FR-68/FR-69/FR-71, AD-37, AD-34).
2. Drive the cycle from the ⌘5 Recording view: Source = a full display, system audio
   on, press Start → view flips to *active* with the `recording-red` record dot and a
   ticking `mono` elapsed line; confirm macOS posts its own purple capture pill in
   parallel (UX-DR29/UX-DR30).
3. Press Stop → the file finalizes (defragments) to an ordinary `.mp4`; confirm it
   plays back in QuickTime with **continuous audio and video** and that keeper's own
   notification sounds are **absent** from the captured audio (FR-69, AD-37).
4. Confirm the whole cycle (pre-flight → full-screen + system-audio capture → single
   playable fMP4 in the chosen folder → clean Stop) on the dev-signed build — this is
   the Epic 16 exit gate (R.1 / SM-9 seed): a real recording plays back on dev-signed
   hardware.

### Fold in the real-capture hardening deferred from 16.1–16.5

Twelve deferred-work entries are explicitly tagged "(16.6)" and are meant to be
implemented and validated together with the real capture leg (they harden the
now-persistent, long-lived capture session that stub sidecars could not exercise):

- **spec-16-1 (1):** validate that keeper-rec's hardened-runtime signature +
  entitlements survive tauri-action's bundle re-sign inside the notarized `.app`,
  and that the empty entitlements file suffices — or add audio/capture entitlements —
  once real ScreenCaptureKit + system-audio capture lands; extend the `codesign -dv`
  verify to the copy inside the `.app`.
- **spec-16-2 (6):** surface `SidecarFailed` on a mid-stream stdout I/O error (don't
  treat it as clean EOF); add a bounded channel + line-length cap
  (backpressure/anti-flood); add a `child.wait()` timeout + kill fallback; cap/scrub
  the sidecar-provided `Failed` message; reconcile a non-zero exit that follows an
  already-reported terminal so it doesn't mask the honest outcome; add a
  fake-executable NDJSON harness covering spawn→stream→reap.
- **spec-16-4 (4):** move id-correlation to monotonically-increasing request ids with
  response/method validation for the persistent start/stop/events session; add a
  request/response timeout; cap the read loop; cap+scrub embedded sidecar error
  messages once real capture paths/device names can appear.
- **spec-16-5 (1):** finalize the honest awaiting-vs-denied Screen Recording labeling
  against a real grant (the sync `CGRequestScreenCaptureAccess()` return can't
  distinguish "prompt live/awaiting" from "denied").

**Epic 16 exit gate:** all Story 16.6 ACs must pass on dev-signed hardware — a real
recording plays back — before Epic 17 (segmentation & recovery) and Epic 18 (tray &
loud failures) build on it.
