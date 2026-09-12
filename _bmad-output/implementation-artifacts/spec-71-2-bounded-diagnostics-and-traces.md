---
story: "71.2"
title: "Bounded diagnostics and traces"
status: review
---

# 71.2 — Bounded diagnostics and traces

## Intent and acceptance

Content-free structured diagnostic records, operation timings and OpenTelemetry-compatible trace export. Instrument real client operations with closed operation names and numeric outcomes; retain local logs unchanged. Acceptance: real operation produces connected spans and correlated diagnostic records; invalid attributes cannot serialize; offline/export failure does not block the operation; queue/time limits hold; no recording path is exported.

The complete Always / Block If / Never contract is in [Epic 71](../planning-artifacts/epic-71-observability-without-private-content.md). The exact cross-slice IPC and ownership contract is [frozen here](../planning-artifacts/epic-71-implementation-contract.md). Both are normative.

## I/O and edge cases

| Input or transition | Required observable result |
|---|---|
| Valid diagnostic operation | OTLP span and correlated log with closed attributes |
| Queue full or collector offline | Bounded drop; operation completes |
| Revocation during send | Cancellation; no new sends |
| Raw private log/error | Never accepted by export schema |

## Code map and ownership

Use the evidence/code-map section in the epic and the exact ownership table in the frozen implementation contract. Backend owns src-tauri; frontend owns src and bundled study entry; operations owns scripts/posthog and its dedicated workflow; Main owns docs, shared fixture integration and final gates. No agent edits another owner's files without coordination.

## Verification record

Bounded Rust export and cancellation are exercised; independent canonical protobuf decoding and live correlated OTLP log/span readback passed.

Source commits: client `e5ab18b`, maintainer tooling `35e9e3e`, both on `vigorous-chicken`. Exact gate results, independent review dispositions, runtime/fixture distinctions and external blockers are recorded once in [epic-71-verification.md](epic-71-verification.md). This status is not a published-PR or deployment claim.

## Delivery

Commit on the original checkout branch only. Coordinator owns branch topology/pushing. Keep Rust-generated types with every consumer so each future stack rung builds alone. This story cannot be marked done while any acceptance above is unmet.
