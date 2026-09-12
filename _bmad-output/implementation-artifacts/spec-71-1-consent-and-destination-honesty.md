---
story: "71.1"
title: "Consent and destination honesty"
status: review
---

# 71.1 — Consent and destination honesty

## Intent and acceptance

Rust-owned versioned consent and generated view models; reachable settings controls; fail-closed persistence/validation; live destination disclosure; policy documentation updated only with working enforcement. Acceptance: a fresh install makes zero PostHog requests; enabling one category enables only that category; disabling clears unsent records and stops capture across windows; corrupt state fails closed.

The complete Always / Block If / Never contract is in [Epic 71](../planning-artifacts/epic-71-observability-without-private-content.md). The exact cross-slice IPC and ownership contract is [frozen here](../planning-artifacts/epic-71-implementation-contract.md). Both are normative.

## I/O and edge cases

| Input or transition | Required observable result |
|---|---|
| Absent/corrupt consent | All categories off; zero network |
| One category enabled | Only its closed records eligible |
| Consent revoked during queue wait | Unsent records dropped |
| Synced config contains consent keys | Cannot grant local consent |

## Code map and ownership

Use the evidence/code-map section in the epic and the exact ownership table in the frozen implementation contract. Backend owns src-tauri; frontend owns src and bundled study entry; operations owns scripts/posthog and its dedicated workflow; Main owns docs, shared fixture integration and final gates. No agent edits another owner's files without coordination.

## Verification record

Consent, local-only persistence, category enforcement, pure preview and reachable independent Settings controls are implemented and exercised.

Source commits: client `e5ab18b`, maintainer tooling `35e9e3e`, both on `vigorous-chicken`. Exact gate results, independent review dispositions, runtime/fixture distinctions and external blockers are recorded once in [epic-71-verification.md](epic-71-verification.md). This status is not a published-PR or deployment claim.

## Delivery

Commit on the original checkout branch only. Coordinator owns branch topology/pushing. Keep Rust-generated types with every consumer so each future stack rung builds alone. This story cannot be marked done while any acceptance above is unmet.
