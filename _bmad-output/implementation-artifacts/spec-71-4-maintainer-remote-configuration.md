---
story: "71.4"
title: "Maintainer remote configuration"
status: review
---

# 71.4 — Maintainer remote configuration

## Intent and acceptance

Read public non-secret configuration through PostHog flags only after separate consent. Validate a small fixed schema with safe shipped defaults; demonstrate a reachable configuration effect. Never synchronize user preferences through PostHog person properties. Acceptance: malformed/offline values fall back; payload cannot change security/collection/destinations; user preferences win.

The complete Always / Block If / Never contract is in [Epic 71](../planning-artifacts/epic-71-observability-without-private-content.md). The exact cross-slice IPC and ownership contract is [frozen here](../planning-artifacts/epic-71-implementation-contract.md). Both are normative.

## I/O and edge cases

| Input or transition | Required observable result |
|---|---|
| Remote config consent off | No flags request |
| Valid bounded support message | Plain text rendered in diagnostics settings |
| Unknown/security key or invalid response | Reject or ignore; keep shipped default |
| Offline or oversize body | Bounded failure, normal app unaffected |

## Code map and ownership

Use the evidence/code-map section in the epic and the exact ownership table in the frozen implementation contract. Backend owns src-tauri; frontend owns src and bundled study entry; operations owns scripts/posthog and its dedicated workflow; Main owns docs, shared fixture integration and final gates. No agent edits another owner's files without coordination.

## Verification record

Actual Rust public flag retrieval and the reachable plain-text Settings consumer passed; remote values cannot control consent, security or destinations.

Source commits: client `e5ab18b`, maintainer tooling `35e9e3e`, both on `vigorous-chicken`. Exact gate results, independent review dispositions, runtime/fixture distinctions and external blockers are recorded once in [epic-71-verification.md](epic-71-verification.md). This status is not a published-PR or deployment claim.

## Delivery

Commit on the original checkout branch only. Coordinator owns branch topology/pushing. Keep Rust-generated types with every consumer so each future stack rung builds alone. This story cannot be marked done while any acceptance above is unmet.
