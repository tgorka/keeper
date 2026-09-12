---
story: "71.3"
title: "Product statistics, errors and performance"
status: review
---

# 71.3 — Product statistics, errors and performance

## Intent and acceptance

Explicit closed-vocabulary product events and sanitized error categories; application readiness and interaction duration measurements rather than claiming web vitals measure Rust. Acceptance: reachable controls produce the declared event only with product consent; errors never include exception payloads; sampling and missing clients are documented in metric definitions.

The complete Always / Block If / Never contract is in [Epic 71](../planning-artifacts/epic-71-observability-without-private-content.md). The exact cross-slice IPC and ownership contract is [frozen here](../planning-artifacts/epic-71-implementation-contract.md). Both are normative.

## I/O and edge cases

| Input or transition | Required observable result |
|---|---|
| App readiness after opt-in | Bounded numeric diagnostic duration |
| Settings/palette interaction | Product event only with product consent |
| Error includes private sentinel | Only static category exported |
| Consent absent | No identifier or event emitted |

## Code map and ownership

Use the evidence/code-map section in the epic and the exact ownership table in the frozen implementation contract. Backend owns src-tauri; frontend owns src and bundled study entry; operations owns scripts/posthog and its dedicated workflow; Main owns docs, shared fixture integration and final gates. No agent edits another owner's files without coordination.

## Verification record

Closed product/error events, native grouped error ingestion and navigation-relative readiness measurements are implemented and exercised.

Source commits: client `e5ab18b`, maintainer tooling `35e9e3e`, both on `vigorous-chicken`. Exact gate results, independent review dispositions, runtime/fixture distinctions and external blockers are recorded once in [epic-71-verification.md](epic-71-verification.md). This status is not a published-PR or deployment claim.

## Delivery

Commit on the original checkout branch only. Coordinator owns branch topology/pushing. Keep Rust-generated types with every consumer so each future stack rung builds alone. This story cannot be marked done while any acceptance above is unmet.
