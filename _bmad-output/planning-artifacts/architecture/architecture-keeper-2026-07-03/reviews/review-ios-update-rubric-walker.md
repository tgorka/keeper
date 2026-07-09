# Review — Rubric Walker (good-spine checklist), iOS-phase update 2026-07-09

**Scope reviewed:** AD-26..AD-32 additions + touched sections (frontmatter, conventions, stack, seed, capability map, consistency check, deferred). AD-1..25 frozen, not relitigated.

**Verdict: PASS** (2 fixes applied during the gate, see adversarial review).

- **Fixes real divergence points, misses none found:** platform seam (AD-26), capability surface (AD-27), media transport (AD-28), secrets/file protection (AD-29), lifecycle/sync ownership (AD-30), navigation/layout (AD-31), build+CI (AD-32) — each names the divergence it prevents and an enforceable rule.
- **Every AD enforceable:** cfg-gates and target-gated deps are compile-checkable (AD-26/32 via the CI iOS check); "no platform sniffing in TS" and "no router" are review-checkable conventions; "sole lifecycle entry point" and "badge = inbox aggregate" pin single owners.
- **Deferred cannot cause divergence:** push/NSE, Android remap helper, Swift lifecycle plugin, disk-backed streaming all have named seams (data-dir root, Platform port, single lifecycle command, capped buffer) that hold whichever way they resolve.
- **Verified-current tech:** all iOS claims trace to research-ios-2026-07-09.md (live web research dated today, cited sources); no new tech asserted from memory. Desktop stack rows untouched.
- **Ratifies brownfield reality:** shell crate-type already includes staticlib; useShellLayout/use-mobile seams exist; media handler already async + Range-capable — the ADs extend, not contradict.
- **Inherited invariants respected:** no new AD weakens AD-1..25. AD-28 preserves AD-4; AD-29 extends AD-10's keychain posture through the same port; AD-30 reuses AD-18's engine and AD-8's snapshot-then-diff; AD-31 preserves AD-9 (selection state stays the truth). AD-24's Plan A is confirmed, not amended.
- **All dimensions owned by the altitude decided/deferred/open:** build & toolchain, distribution/signing envelope (deployment prose updated: Personal Team, 7-day re-arm, re-sign flows), CI, security posture, lifecycle, UX shell, out-of-scope list. Operational envelope explicitly covered (docs/ios.md seeded; no push infra promised).
- **Spec coverage:** FR-55..65 and NFR-15..18 each appear in the capability map with governing ADs; SM-7 walking-skeleton gate recorded as a phase gate in the consistency check; PRD §13.8 pre-answers (iOS 16.0, shared bundle ID, no router, Plan B shelved) all encoded.
