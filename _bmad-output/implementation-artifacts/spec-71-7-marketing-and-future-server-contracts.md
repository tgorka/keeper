---
story: "71.7"
title: "Marketing and future-server contracts"
status: review
---

# 71.7 — Marketing and future-server contracts

## Intent and acceptance

Provide maintainer tooling for campaign-source mappings and conversion definitions over explicitly consented events. No guessed ad-account integrations or cross-device attribution. Keeper repository has no marketing website: integration boundaries with tgsite are documented rather than modifying its live deployment silently. Analytical Endpoints are exercised through maintainer tooling only. Future server contract: authenticate caller, derive tenant scope server-side, proxy privileged queries; propagate traceparent only to owned participating hosts, use span links for delayed delivery and separate durable operation IDs. Acceptance: no query key in client; endpoint variables are never described as authorization; future server flow is labelled unimplemented.

The complete Always / Block If / Never contract is in [Epic 71](../planning-artifacts/epic-71-observability-without-private-content.md). The exact cross-slice IPC and ownership contract is [frozen here](../planning-artifacts/epic-71-implementation-contract.md). Both are normative.

## I/O and edge cases

| Input or transition | Required observable result |
|---|---|
| Query a user-scoped endpoint from client | No embedded privileged key or implemented client route |
| Maintainer endpoint query | Explicit project and synthetic scope |
| Marketing account missing | No invented campaign connections |
| Future server contract | Authentication and trace propagation documented as future |

## Code map and ownership

Use the evidence/code-map section in the epic and the exact ownership table in the frozen implementation contract. Backend owns src-tauri; frontend owns src and bundled study entry; operations owns scripts/posthog and its dedicated workflow; Main owns docs, shared fixture integration and final gates. No agent edits another owner's files without coordination.

## Verification record

Personless installation segments and engagement definitions execute through maintainer tooling. No ads or cross-device attribution is claimed; the owner-selected future server contract remains explicitly unimplemented.

Source commits: client `e5ab18b`, maintainer tooling `35e9e3e`, both on `vigorous-chicken`. Exact gate results, independent review dispositions, runtime/fixture distinctions and external blockers are recorded once in [epic-71-verification.md](epic-71-verification.md). This status is not a published-PR or deployment claim.

## Delivery

Commit on the original checkout branch only. Coordinator owns branch topology/pushing. Keep Rust-generated types with every consumer so each future stack rung builds alone. This story cannot be marked done while any acceptance above is unmet.
