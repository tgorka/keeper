---
story: "71.8"
title: "CI, security and platform evidence"
status: in-progress
---

# 71.8 — CI, security and platform evidence

## Intent and acceptance

Privileged provisioning runs only on an explicitly trusted/manual path, never pull_request_target or untrusted PR code. Build/test jobs receive no personal key. Inspect generated output for secrets using synthetic sentinels and secret-pattern guards without dumping matching content. Run frontend, Linux-buildable Rust, macOS shell and iOS gates; inspect actual application/browser requests and live PostHog ingestion. Acceptance: evidence names exactly which platforms/surfaces ran; no story is marked done on fixture-only evidence.

The complete Always / Block If / Never contract is in [Epic 71](../planning-artifacts/epic-71-observability-without-private-content.md). The exact cross-slice IPC and ownership contract is [frozen here](../planning-artifacts/epic-71-implementation-contract.md). Both are normative.

## I/O and edge cases

| Input or transition | Required observable result |
|---|---|
| Untrusted PR | No privileged provisioning secret |
| Client build | Only public host/project token inputs |
| Synthetic privileged-key sentinel | Absent from distributable outputs |
| macOS/iOS unexecuted gate | Reported unverified, never inferred from Linux |

## Code map and ownership

Use the evidence/code-map section in the epic and the exact ownership table in the frozen implementation contract. Backend owns src-tauri; frontend owns src and bundled study entry; operations owns scripts/posthog and its dedicated workflow; Main owns docs, shared fixture integration and final gates. No agent edits another owner's files without coordination.

## Verification record

Frontend, Rust, browser, live-service and artifact evidence is centralized below. Protected GitHub environment creation requires administrator access (HTTP 403); no published workflow run or signed release is claimed.

Source commits: client `e5ab18b`, maintainer tooling `35e9e3e`, both on `vigorous-chicken`. Exact gate results, independent review dispositions, runtime/fixture distinctions and external blockers are recorded once in [epic-71-verification.md](epic-71-verification.md). This status is not a published-PR or deployment claim.

## Delivery

Commit on the original checkout branch only. Coordinator owns branch topology/pushing. Keep Rust-generated types with every consumer so each future stack rung builds alone. This story cannot be marked done while any acceptance above is unmet.
