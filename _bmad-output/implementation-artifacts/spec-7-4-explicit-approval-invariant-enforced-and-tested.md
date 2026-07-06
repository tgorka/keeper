---
title: 'Story 7.4: Explicit-Approval Invariant — Enforced and Tested'
type: 'feature'
created: '2026-07-05'
status: 'done'
baseline_revision: 'e9d6d20257cbbc2f16f8690507c3a993b276565a'
final_revision: 'e89a9aa'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** The airlock invariant (AD-13) — keeper may dispatch a message only through exactly two user-initiated triggers (`ComposerSend`, `ApprovalPaneApprove`) and by no other path — is today only *partially* self-guarding. A source-scan test locks each SDK send *verb* (`.send(content)`, `.send_reply(`, …) to one call site, but nothing locks the `SendTrigger` set to exactly two variants, nothing locks that `submit`'s only callers are the two user-initiated methods (a future third trigger, or a background/scheduled/bulk caller, would pass silently), and the invariant is not written down as a binding contract for the future agent-proposal features it exists to protect.

**Approach:** Harden the invariant into an explicit, self-enforcing contract without changing any runtime behavior: (1) document it in `keeper-core::send` rustdoc as binding on future agent-proposal features — "agents may propose; only the user approves"; (2) add a compile-time exhaustiveness gate that fails the build if a third `SendTrigger` variant is added, and a source-scan that fails if `send::submit` gains a third caller or a non-user-initiated trigger; (3) assert both triggers reach the gate and no other public API can dispatch.

## Boundaries & Constraints

**Always:**
- The invariant governs new **plain-text message dispatch** via `send::submit` (composer + approval). Media, edit, reply, reaction, redaction each keep their own single-site gate (already guarded); receipts/typing are AD-14 signals; draft mirroring writes account data, not messages — all out of scope here.
- Enforcement is compile-time + deterministic source-scan; no live homeserver required. New tests pass under `cargo-nextest`.
- Preserve and extend the existing `submit_is_the_sole_send_dispatch_gate` guard — never weaken it.
- Rust rules hold: `-D warnings`, no `unsafe`, no `.unwrap()`/bare `.expect()` in prod paths.

**Block If:**
- The audit finds an **actual bypass**: any path that reaches a room's send queue with a plain-text message without going through `send::submit` + a trigger, OR a third `send::submit` caller / third `SendTrigger` variant already wired, OR any existing background/scheduled/automated/bulk dispatch path. That is a real invariant violation needing a planning decision — HALT, do not paper over it with a test.

**Never:**
- No new dispatch path, trigger, public send API, or `approve-all`/bulk affordance. No behavior change to `send_text`, `send_approval`, or `submit`.
- Never add a `_ =>` wildcard arm to any `SendTrigger` match — it would silently absorb a new variant and defeat the exhaustiveness gate.
- No live-homeserver or fantasy "message actually delivered" test.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Trigger set intact | current source | exhaustiveness guard compiles; `ALL_TRIGGERS.len() == 2` | No error expected |
| Third trigger added (future) | dev adds a `SendTrigger` variant | wildcard-free match fails to compile; count assert fails | Forces a planning decision |
| Two gate callers | production `account.rs` | scan finds exactly two `send::submit(` sites: one `ComposerSend`, one `ApprovalPaneApprove` | No error expected |
| Third / background caller (future) | dev adds any `send::submit(` call | scan count ≠ 2 → guard fails | Blocks the bypass |
| ComposerSend reachable | `send_text`, unparsable room / non-live account | typed `RoomNotFound` / `NoOpenTimeline` (never `Ok`, never panic) | Proves path reaches the gate boundary |
| ApprovalPaneApprove reachable | `send_approval` (existing tests) | routes through the gate; whitespace body → `EmptyBody` | Existing coverage retained |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/send.rs` -- module rustdoc (:1-12) states the FR-41/AD-13 single-gate; `SendTrigger` enum (:31-38, two variants) + `as_label` wildcard-free match (:43-48); `submit` = sole `.send(content)` gate (:60-83); existing guard `submit_is_the_sole_send_dispatch_gate` (:350) scans `include_str!("send.rs")` and splits off its own `#[cfg(test)]\nmod tests` slice before scanning. ADD the binding-contract rustdoc + two new guard tests here.
- `src-tauri/crates/keeper-core/src/account.rs` -- the two and only `send::submit` callers: `send_text` → `SendTrigger::ComposerSend` (:2107), `send_approval` → `SendTrigger::ApprovalPaneApprove` (:2220). `open_timeline_for` returns `SendError::NoOpenTimeline` for a non-live account (:3246). Exactly one `#[cfg(test)]` boundary (:4547) → clean production slice for `include_str!("account.rs")`. Existing approval behavioral tests (:4746, :4793). ADD the symmetric composer-trigger reachability test.
- `src-tauri/crates/keeper-core/src/error.rs` -- `SendError` variants incl. `RoomNotFound`, `NoOpenTimeline`, `EmptyBody` (:142-188). No change expected.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/send.rs` (module + `SendTrigger` rustdoc) -- expand the header into the **binding contract** for future agent-proposal features: exactly two user-initiated dispatch triggers (`ComposerSend`, `ApprovalPaneApprove`), both flowing through `submit`; **no** background, scheduled, automated, or bulk dispatch path exists or may be added; agents may *propose* (write drafts) but only the *user* approves — a proposal is never a dispatch; adding a trigger or any unattended send path requires a new planning-level decision, not merely a code change; name the guard tests that enforce this. Enrich the `SendTrigger` doc to declare it the closed set. Doc-only — no code/behavior change.
- [x] `src-tauri/crates/keeper-core/src/send.rs` (test module) -- add `exactly_two_legal_dispatch_triggers`: a `const ALL_TRIGGERS: &[SendTrigger] = &[SendTrigger::ComposerSend, SendTrigger::ApprovalPaneApprove];`, a wildcard-free exhaustive `match` over it (no `_ =>` arm — so adding a variant fails to compile here), and `assert_eq!(ALL_TRIGGERS.len(), 2, …)`. A comment marks it the planning gate and forbids a wildcard arm.
- [x] `src-tauri/crates/keeper-core/src/send.rs` (test module) -- add `submit_has_exactly_the_two_user_initiated_callers`: take the production slice of `include_str!("account.rs")` (everything before the sole `#[cfg(test)]` marker) and assert `send::submit(` appears **exactly twice**; assert the call-form `SendTrigger::ComposerSend).await` appears exactly once and `SendTrigger::ApprovalPaneApprove).await` exactly once (call forms dodge the `[`SendTrigger::…`]` rustdoc reference at account.rs:2172). Failure messages report the counts found. This locks "exactly two user-initiated callers; no third/background/bulk caller; no other public API dispatches."
- [x] `src-tauri/crates/keeper-core/src/account.rs` (test module) -- add `send_text_composer_trigger_routes_through_the_gate`, mirroring `send_approval_routes_through_the_single_gate`: on a fresh `AccountManager`, `send_text("acctA", "not-a-room-id", "hi")` → `Err(CoreError::Send(SendError::RoomNotFound))`; `send_text("acctA", "!room:example.org", "hi")` → `Err(CoreError::Send(SendError::NoOpenTimeline))` (composer legitimately requires an open conversation — there is no transient-build fallback). Proves the `ComposerSend` trigger path is reachable and reaches the gate-acquisition boundary with a typed error, never `Ok`/panic — no live homeserver.

**Acceptance Criteria:**
- Given the current source, when the invariant guards run under `cargo-nextest`, then the two-trigger exhaustiveness guard, the two-caller scan, and both triggers' reachability tests pass, and the pre-existing `submit_is_the_sole_send_dispatch_gate` guard still passes.
- Given a future change that adds a third `SendTrigger` variant, then the crate fails to compile at the wildcard-free match (and the count assert fails); given a future change that adds a third `send::submit` call site, then `submit_has_exactly_the_two_user_initiated_callers` fails — each surfacing the change as an invariant breach requiring a planning decision.
- Given `keeper-core::send` rustdoc, then it documents the explicit-approval invariant as binding on future agent-proposal features ("agents may propose; only the user approves"; no background/scheduled/automated/bulk dispatch; changes require a planning decision) and names the enforcing guards.
- Given the audit, then no actual bypass exists in MVP: every plain-text dispatch flows through `send::submit` with one of exactly two user-initiated triggers, and no programmatic or background API can send.

## Spec Change Log

_No bad_spec loopback — the spec's source-scan + exhaustiveness + rustdoc approach held through review; all review-driven changes were auto-applied patches (see Review Triage Log)._

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 0, low 3)
- defer: 0
- reject: 12: (high 0, medium 0, low 12)
- addressed_findings:
  - `[low]` `[patch]` P1 — the caller scan trusted `.split("#[cfg(test)]").next()`; a future *second* `#[cfg(test)]` block in `account.rs` would silently truncate the scanned production slice and could hide a `send::submit` caller after it (false PASS). Now asserts `account.rs` contains exactly one `#[cfg(test)]` marker before splitting, so the load-bearing assumption fails loudly instead. `send.rs`.
  - `[low]` `[patch]` P3 — the caller scan matched exact substrings, so a rustfmt-reformatted / multi-line call site (`send::submit(\n …`, `SendTrigger::ComposerSend)\n .await`) could evade the count (false NEGATIVE — the realistic bypass a hardening story must close). Now whitespace-normalizes the production slice before counting; a `crate::send::submit(` prefix already matched as a substring. `send.rs`.
  - `[low]` `[patch]` P2 — the module rustdoc said "no programmatic send API" while `submit` (and siblings) are `pub`, and it did not reconcile the airlock's scope with the other send verbs. Tightened to "no *unattended*, scheduled, or bulk send API" (the guard is a caller-supplied trigger, not privacy) and added a **Scope** paragraph placing reply/edit/reaction/redaction/attachment (their own single-site gates), receipts/typing (AD-14 signals), and draft mirroring (account data) outside the `SendTrigger` accounting by design. `send.rs`.
  - reject (12, all low actual-consequence; inherent-limitation of the established source-scan pattern / by-design / factually-moot / speculative / cosmetic): source-scan substring-exactness against aliased or macro-generated callers (same limitation as the already-accepted `submit_is_the_sole_send_dispatch_gate`; the hard guarantees are the type-level single gate + the compile-time exhaustiveness match, which hold regardless); scan reads only `account.rs` so a `submit` caller in another file evades it (any such caller still cannot dispatch except through the audited single gate with one of exactly two typed triggers — the rustdoc contract + code review are the backstop; whole-crate scanning is disproportionate and self-fragile); `assert_eq!(len,2)` "adds nothing" beside the exhaustiveness match (the match is the real compile-time gate; the assert is a secondary human signal — a `strum` derive would add a firewalled dependency for marginal gain); `ALL_TRIGGERS` can drift from the enum (a new variant still cannot compile without a human editing this exact test's match — the intended planning gate); `send_text_…_routes_through_the_gate` stops at timeline-acquisition before `submit` (identical to the accepted sibling `send_approval_routes_through_the_single_gate`; the docstring already says "gate-acquisition boundary"; a live-homeserver dispatch test is explicitly out of scope); "no analogous approval routing test" (factually wrong — `send_approval_routes_through_the_single_gate` exists); `submit`'s internal match could use `_ =>` (submit does not branch on the trigger to decide whether to send — it always dispatches; `as_label` is already wildcard-free); a `SendTrigger` variant carrying data / a non-dispatch variant (speculative future semantics); temp-dir leak on assertion-failure + `unwrap_or(0)` clock-skew suffix collision (pre-existing pattern shared with accepted tests; unique PID+nanos+distinct label — no real collision); a brand-new non-`submit` SDK send verb evading both guards (unbounded/speculative — future SDK verbs cannot be enumerated; the architecture invariant + review cover it).

## Design Notes

- **Why a source-scan and not only types.** Exhaustiveness locks the trigger *set* at compile time, but "exactly two callers / no background dispatch" is a property of *call sites* — only a scan over production `account.rs` asserts it deterministically without a live homeserver. Both `include_str!` scans exclude their file's `#[cfg(test)]` slice so the guards' own string literals never self-match, exactly as the existing gate guard already does.
- **Dodging doc-comment false positives.** Match the call forms `SendTrigger::ComposerSend).await` / `SendTrigger::ApprovalPaneApprove).await` — the rustdoc reference at account.rs:2172 is the bracketed `[`SendTrigger::ApprovalPaneApprove`]` and won't match; `send::submit(` occurs only at the two real call sites (prose says "the single `send::submit` gate", without the `(`).
- **No wildcard arms.** A `_ =>` arm in any `SendTrigger` match would absorb a new variant and defeat the gate; the guard and the rustdoc both forbid it. (`as_label` is already wildcard-free — keep it so.)
- **Scope is plain-text dispatch only.** Media/edit/reply/reaction/redaction each already carry their own single-site guard in the existing send.rs test; receipts/typing are AD-14 signals; draft mirroring writes `dev.keeper.draft` account data, not a room message. Story 7.4 governs the composer + approval message path per AD-13.

## Verification

**Commands:**
- `bun run test:rust` -- expected: `exactly_two_legal_dispatch_triggers`, `submit_has_exactly_the_two_user_initiated_callers`, `send_text_composer_trigger_routes_through_the_gate`, plus the existing `submit_is_the_sole_send_dispatch_gate` and approval tests all pass.
- `bun run check:rust` -- expected: `cargo fmt --check` + clippy `-D warnings`, no `unsafe`, no `.unwrap()` in prod paths.

**Manual checks (if no CLI):**
- Confirm `keeper-core::send` module rustdoc renders the binding contract (agents propose / only the user approves; no background/bulk dispatch; changes need a planning decision) and cites the guard tests (`cargo doc -p keeper-core` optional).

## Auto Run Result

Status: done

**Summary.** Hardened the explicit-approval airlock invariant (AD-13) into a self-enforcing, documented contract with no runtime-behavior change. The audit found no actual bypass: production `account.rs` has exactly the two user-initiated `send::submit` callers (`send_text`→`ComposerSend`, `send_approval`→`ApprovalPaneApprove`), `SendTrigger` is a closed two-variant set, and no background/scheduled/automated/bulk dispatch path exists. Added a binding-contract rustdoc, a compile-time exhaustiveness gate, a whitespace-normalized production source-scan locking exactly two gate callers, and a composer-trigger reachability test.

**Files changed.**
- `src-tauri/crates/keeper-core/src/send.rs` — module + `SendTrigger` rustdoc expanded into the AD-13 binding contract (exactly two user-initiated triggers; no unattended/scheduled/bulk send API; agents propose / only the user approves; changes require a planning decision; a **Scope** paragraph placing the other send verbs and signals outside the `SendTrigger` accounting); added guard tests `exactly_two_legal_dispatch_triggers` (wildcard-free exhaustiveness) and `submit_has_exactly_the_two_user_initiated_callers` (single-`#[cfg(test)]`-marker assertion + whitespace-normalized scan of `account.rs`); pre-existing `submit_is_the_sole_send_dispatch_gate` unchanged.
- `src-tauri/crates/keeper-core/src/account.rs` — added test `send_text_composer_trigger_routes_through_the_gate` (unparsable room → `RoomNotFound`; non-live account → `NoOpenTimeline`). No production code touched.

**Review findings breakdown.** 3 patches applied (all low): P1 single-`#[cfg(test)]`-marker assertion (closes a silent-truncation false-pass), P3 whitespace-normalized scan (closes a rustfmt-reformatting false-negative), P2 rustdoc precision + scope paragraph. 0 deferred. 12 rejected (inherent source-scan limitations shared with the accepted prior guard, by-design, factually-moot, speculative, or cosmetic) — see Review Triage Log.

**Follow-up review recommendation:** `false` — the three review-driven changes are localized, low-consequence, and test/doc-only (no behavior, API, security, or data impact).

**Verification.**
- `bun run test:rust` — 621 tests run, 621 passed, 0 skipped. The four story tests plus the pre-existing gate guard and both approval tests pass.
- `bun run check:rust` — `cargo fmt --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean (no `unsafe`, no `.unwrap()`/bare `.expect()` in prod paths). rustfmt was run on the edited test to satisfy the format gate.

**Residual risks.** The caller source-scan is a defense-in-depth heuristic: it reads `account.rs` only and matches on normalized substrings, so a `send::submit` caller added in a *different* file, via an unusual alias, or macro-generated, would not be counted. This is an accepted, documented limitation — the hard guarantees remain the type-level single gate (the sole `Timeline::send(content)` locked inside `submit` by `submit_is_the_sole_send_dispatch_gate`) and the compile-time exhaustiveness of `SendTrigger`; the rustdoc contract additionally binds any new trigger/caller/unattended path to a planning-level decision, with code review as the backstop.
