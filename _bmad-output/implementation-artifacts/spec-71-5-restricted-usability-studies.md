---
story: "71.5"
title: "Restricted usability studies"
status: review
---

# 71.5 — Restricted usability studies

## Intent and acceptance

Implement a content-isolated synthetic study surface supporting replay and heatmaps after explicit per-session start. Real account/drive/bot/recording components cannot mount in that surface. Stop/unmount ends capture. AI analysis is a separate maintainer-side approval and scope, not an automatic project setting. Acceptance: browser study produces replay/heatmap evidence, private content sentinel remains absent from captured requests, normal app never starts DOM capture.

The complete Always / Block If / Never contract is in [Epic 71](../planning-artifacts/epic-71-observability-without-private-content.md). The exact cross-slice IPC and ownership contract is [frozen here](../planning-artifacts/epic-71-implementation-contract.md). Both are normative.

## I/O and edge cases

| Input or transition | Required observable result |
|---|---|
| Study opened but not started | No SDK/network |
| Explicit start | Synthetic content only; memory-only identity |
| Stop/unmount/window close | Stop capture before clearing egress disclosure |
| Private content sentinel elsewhere | Absent from all captured study requests |
| Remote project enables AI | No automatic local consent to AI processing |

## Code map and ownership

Use the evidence/code-map section in the epic and the exact ownership table in the frozen implementation contract. Backend owns src-tauri; frontend owns src and bundled study entry; operations owns scripts/posthog and its dedicated workflow; Main owns docs, shared fixture integration and final gates. No agent edits another owner's files without coordination.

## Verification record

The actual production study bundle passed privacy-marker, remote-setting veto, Stop/hide/failure/native-close wrapper and narrow-width checks; real replay storage and heatmap coordinates were read back.

Source commits: client `e5ab18b`, maintainer tooling `35e9e3e`, both on `vigorous-chicken`. Exact gate results, independent review dispositions, runtime/fixture distinctions and external blockers are recorded once in [epic-71-verification.md](epic-71-verification.md). This status is not a published-PR or deployment claim.

## Delivery

Commit on the original checkout branch only. Coordinator owns branch topology/pushing. Keep Rust-generated types with every consumer so each future stack rung builds alone. This story cannot be marked done while any acceptance above is unmet.
