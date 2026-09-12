---
story: "71.6"
title: "PostHog project tooling"
status: review
---

# 71.6 — PostHog project tooling

## Intent and acceptance

Safe idempotent maintainer provisioning for a Keeper-specific project: explicit events, cohorts/personas, governed metric catalog, remote-config defaults and analytical Endpoints. Dry run is default; apply requires explicit invocation; responses and errors never print secrets. Reuse tgsite's credential *reference conventions*, not its analytics project. Acceptance: dry run is non-mutating; repeated apply creates no duplicates; live API readback confirms managed resources without exposing keys. Human metric approvals and AI-processing approvals are not forged by automation.

The complete Always / Block If / Never contract is in [Epic 71](../planning-artifacts/epic-71-observability-without-private-content.md). The exact cross-slice IPC and ownership contract is [frozen here](../planning-artifacts/epic-71-implementation-contract.md). Both are normative.

## I/O and edge cases

| Input or transition | Required observable result |
|---|---|
| Dry run | No remote writes |
| Repeated apply | No duplicate managed resources |
| Conflicting unmanaged resource | Fail closed |
| Missing key/HTTP error | Safe status-only output |
| Metric proposal | No automatic human approval |

## Code map and ownership

Use the evidence/code-map section in the epic and the exact ownership table in the frozen implementation contract. Backend owns src-tauri; frontend owns src and bundled study entry; operations owns scripts/posthog and its dedicated workflow; Main owns docs, shared fixture integration and final gates. No agent edits another owner's files without coordination.

## Verification record

Reviewed live apply/readback and idempotent verification passed for the dedicated Keeper project; all four Endpoints execute, metrics remain proposed and confidence is null.

Source commits: client `e5ab18b`, maintainer tooling `35e9e3e`, both on `vigorous-chicken`. Exact gate results, independent review dispositions, runtime/fixture distinctions and external blockers are recorded once in [epic-71-verification.md](epic-71-verification.md). This status is not a published-PR or deployment claim.

## Delivery

Commit on the original checkout branch only. Coordinator owns branch topology/pushing. Keep Rust-generated types with every consumer so each future stack rung builds alone. This story cannot be marked done while any acceptance above is unmet.
