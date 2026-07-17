---
status: ready-for-dev
---

# BMad Dev Auto Result

Status: blocked

Blocking condition: human-in-the-loop — deferred to coordinator (not an escalation)

## Story

- Key: `12-6-on-device-walking-skeleton-validation-sm-7-gate`
- Title: Story 12.6 — On-Device Walking-Skeleton Validation (SM-7 Gate)
- Epic: 12 (iOS Walking Skeleton — Build, Sign, Run)

## Why this HALTs (not a spec problem, not an escalation)

Story 12.6 is one of exactly two stories in the iOS phase deliberately marked
**human-in-the-loop**, to be **deferred to the coordinator rather than escalated**.
This is stated verbatim across the frozen planning artifacts:

- `epics.md:26` — "exactly two stories are explicitly human-in-the-loop (12.6
  on-device skeleton validation, 15.6 final device install) so the automation loop
  defers them to the coordinator rather than escalating."
- `epics.md:1902` — "**Human-in-the-loop: yes** — requires the owner's physical
  iPhone and free Personal Team signing (Developer Mode enabled, personal-team
  certificate trusted on device). The automation loop defers this story to the
  coordinator instead of escalating; all other Epic 12 stories are device-free."
- `epics.md:2422` — 12.6 and 15.6 "both explicitly marked so the automation loop
  defers them to the coordinator rather than escalating."
- `epic-12-context.md:44` — "12.6 depends on all of 12.1–12.5 and is the
  human-in-the-loop SM-7 gate — the automation loop defers it to the coordinator
  rather than escalating."

The gate's acceptance criteria are physically un-automatable in this unattended
session: they require deploying to the owner's actual iPhone via `tauri ios dev`
under free Personal Team signing, completing the on-device certificate-trust flow,
driving OIDC `keeper://` deep-link login, exercising E2EE send/receive, relaunch
session-restore, and an overnight background/foreground resume soak. There is no
simulator or CI substitute for the on-device leg (that was the whole point of the
epic being simulator/compile-first for 12.1–12.5 and device-only for 12.6).

This is NOT a `bad_spec` / `intent_gap` / contradiction condition, so it must not be
routed to `bmad-loop-resolve`. It is a clean hand-off: the spec is coherent and its
dependencies are satisfied — only a human with the device can execute it.

## Readiness of the gate

Dependencies are all satisfied (per `sprint-status.yaml`):

- 12.1 iOS Project Init and Repo Integration — done
- 12.2 Desktop/Mobile Compile Seam and Capability Handshake — done
- 12.3 iOS Platform Port — Keychain Spike and Data Directory — done
- 12.4 Media Protocol on WKURLSchemeHandler with Capped Buffers — done
- 12.5 iOS Compile Check in CI — done

The SM-7 gate is therefore **ready for the coordinator to run on-device**.

## What the coordinator (human owner) needs to do

Run the SM-7 checklist on a physical iPhone under free Personal Team signing and
record results back into the story / `docs/ios.md`:

1. Enable Developer Mode on the iPhone; trust the personal-team certificate;
   record the certificate-trust steps for `docs/ios.md` (AC1, FR-55).
2. `tauri ios dev` deploys, installs, and launches the app on the device.
3. Execute the SM-7 gate: OIDC login via `keeper://` deep-link callback → room list
   loads → text send/receive in one E2EE room → relaunch restores session without
   re-login (AC2; FR-55, FR-63, SM-7).
4. Lifecycle soak: background / foreground / overnight-suspend. Record any
   blank-webview occurrence (tauri#14371) as direct input to Story 14.4's guard
   (NFR-18 first exercise, AD-30); spot-check on-device media rendering (Story 12.4
   paths) (AC3).
5. Note the 7-day profile expiry / re-sign behavior confirming the stable bundle
   identifier preserves local data (AC4).

**Epic 12 exit gate:** all Story 12.6 ACs must pass on-device before Epics 13/14
phone-UX work begins.
