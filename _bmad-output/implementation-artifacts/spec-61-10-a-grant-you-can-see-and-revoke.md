---
title: 'Story 61.10: a grant you can see and revoke'
type: 'feature'
created: '2026-09-02'
status: 'review'
baseline_revision: '9e06c2b'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-386, FR-387 → AD-158; FR-388, NFR-47 → AD-158; AD-139 (scalars only, consumed), AD-27 (no affordance that lies, consumed), AD-C7 (one component, two modes, consumed)
depends on: 61.4 (the pane header the bar sits in, the Settings dialog, `CapabilitiesVm.sync` for the affordance gate)

<intent-contract>

## Intent

**Problem:** The owner asked for *"permission (read, write, none, drive, part of the drive)"* for local data access. The epic's finding: *"absent — there is nothing to extend. What exists is routing and disclosure, not permission."* `IncognitoScope`, `MediaPolicy`, `VirtualPolicy` are sync policies; TCC prompts are the OS's. This story mints keeper's first user-granted, revocable capability, and the epic set its standard: *built like a promise* — checked at every tool call and not at conversation start, `write` never meaning auto-approved, an audit row before the effect whose reader is a human. It also carried the coordinator's correction of 2026-09-02: a grant is offered only where keeper is the thing that would execute the tool, an Ollama bot whose model advertises `tools`; a Hermes bot's pane offers none and says why (`/v1/capabilities` declares `tool_execution: "server"`, no route registers a client tool, `unattended_mode` defaults to `deny`).

**Approach:** Shipped in two halves. The **engine** half is `keeper_core::bots::grant` (the decision, pure) plus `bots::audit` (the record) plus the `bot_grants` table in `bots::store`, with four `#[tauri::command]`s. The **UI** half is the pane's `BotGrantBar` and `BotGrantEditor` (AD-C7: create and rewrite in one component), the `BotApprovalDialog` that answers an `Ask`, and a `BotGrantsSection` in Settings that lists every grant and the audit log. Every refusal sentence is a single `const` in `grant.rs`, quoted verbatim by the UI, stored verbatim in `bot_audit.reason`, and asserted by name.

## Boundaries & Constraints

**Always — `decide(grants, target, effect)`, in this order:**
1. Re-validate `target.subpath` — unsafe ⇒ `Deny(DENY_SUBPATH_UNSAFE)`; the grammar (`parse_subpath`) is the door, this is belt and braces.
2. Keep grants whose scope covers the target — none ⇒ `Deny(DENY_NO_GRANT)`. `Drive` covers every profile, including ones added after the grant; `Profile{p}` covers `p` at any depth; `Subtree{p, sub}` covers the target **by segment** — `segments(target)` must start with `segments(sub)`, so `notes` covers `notes` and `notes/2026/a.md` and does not cover `notes-old`, `notesomething` or `notes2`. The profile root (`subpath == ""`, only via `ToolTarget::profile_root`) is covered by `Drive` and `Profile`, never by a `Subtree` — a grant on a subtree is not a grant on its container.
3. Keep those of maximal specificity: scope depth primary (`Drive` 0, `Profile` 1, `Subtree` 2 + segment count), bot-scoped over provider-wide secondary. **Most specific wins.**
4. Among the winners, any `GrantMode::None` ⇒ `Deny(DENY_MODE_NONE)` — deny before ask before allow.
5. Otherwise the most permissive winner (write > read; ties broken by smaller id so the audit log names a deterministic authority). Read + read/write ⇒ `Allow{grant_id}`. Write + read ⇒ `Deny(DENY_READ_ONLY)`. Write + write on a `Subtree` ⇒ `Allow`. Write + write on `Profile` or `Drive` ⇒ `Ask{grant_id, ASK_WRITE_WIDE_SCOPE}`.
- **FR-387, as shipped:** `Allow` for a write is produced only by a grant whose own scope is the subtree the write lands in. Drawing that subtree *is* the durable "always for this subtree" approval — a grant edit, listed in Settings, revocable there — and not a remembered click. A write-mode grant on a whole profile or the drive is a standing permission to be asked, every time.
- `check(data_dir, provider_id, bot_id, target, effect)` re-reads `bot_grants` on every call and holds nothing across calls; the store narrows in SQL (`provider_id = ?1 AND (bot_id IS NULL OR bot_id = ?2)`), so a call is never judged against another bot's permissions. A revoked grant that would have covered the target yields `Deny(DENY_REVOKED)`.
- Audit: `append_intent` writes the row **before** the effect, opening, committing and dropping its own connection; `complete` fills `finished_ms`, `outcome`, `bytes`. `outcome = 'pending'` with `finished_ms IS NULL` is the crash evidence, so neither has a non-null default. A failure to write the intent row is a refusal, never a silent proceed. The reader is a human: `display_path` holds `work/notes/a.md`; `profile_id` + `subpath` are the filter's.
- Revocation sets `revoked_ms`, never deletes — the audit keeps its referent. A revoked grant is still listed and grants nothing; saving the same id again brings it back to life.
- `grant_offer(kind, tools_supported)` is the decision home for the affordance: Hermes ⇒ `Absent{HERMES_RUNS_ITS_OWN_TOOLS}`; Ollama `Some(false)` ⇒ `Absent{MODEL_HAS_NO_TOOLS}`; Ollama `Some(true)` ⇒ `Offered{warning: None}`; Ollama `None` ⇒ `Offered{warning: Some(TOOLS_CAPABILITY_UNKNOWN)}`. The UI's `botGrantOffer` mirrors it and a source-parity test pins the three sentences to `grant.rs` letter for letter.
- The bar and the Settings section are gated on `CapabilitiesVm.sync` — the honest split 61.4 named: chat needs no `sync.db`, the drive-tool half needs `sync` exactly.

**Block If:** nothing; every rule is stated in the epic or in AD-158.

**Never:** cache a grant set across calls; auto-approve a write from a wide grant; delete a grant row; store JSON in a row; retype a `DENY_*`/`ASK_*` sentence in TypeScript; treat a missing approver as consent (`None` declines every ask).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| No grant | any target | `Deny(DENY_NO_GRANT)` — names Settings → Bots as the act | The sentence IS the handling |
| Shared prefix | `Subtree{p, notes}`; target `notes-old/a.md` | not covered — segment match, not string prefix | `DENY_NO_GRANT` |
| Deeper beats wider | `Drive: read` and `Subtree{p, notes}: none`; target `notes/a.md` | `Deny(DENY_MODE_NONE)` | The sentence IS the handling |
| Bot beats provider | provider-wide read and bot-scoped write at the same scope | the bot-scoped one wins | No error expected |
| Wide write | `Drive: write`; effect Write | `Ask{…, ASK_WRITE_WIDE_SCOPE}` — never `Allow` | The dialog answers it |
| Wide write, read | `Profile: write`; effect Read | `Allow` without asking | No error expected |
| Revoked mid-flight | grant revoked between two calls | second call `Deny(DENY_REVOKED)`; `run_tool` proves the file is created on the first call and not on the second | The sentence IS the handling |
| Crash between row and effect | `append_intent` committed, process gone | a fresh connection reads a `pending` row with `finished_ms NULL`; Settings labels it *Pending* and says what that means | Evidence, not error |
| Unknown provider; unreadable row | `save_grant` naming no provider; a `subtree` row missing its subpath | refused — foreign keys are ON on this build (`SQLITE_DEFAULT_FOREIGN_KEYS=1`), so `ON DELETE CASCADE` is what keeps `delete_provider`/`delete_bot` working; listed as `UnknownBotGrantVm`, grants nothing, revoke is its one action | `internal`; listed, never widened |
| Dialog | `Ask` raised | title *This bot is asking to write*; Rust's reason verbatim; `write — p1/journal/2026/monday.md`; Deny (focused, Escape), Just this once, Always for this folder — *journal/2026* | Escape denies and saves no grant |
| Always, top-of-profile path | parent is the profile root | the *Always* control is absent with a sentence — a profile-wide write grant would ask every time by FR-387, so it would not stop the asking | Deny and once remain |

</intent-contract>

## Code Map

| Half | Path | Change |
|---|---|---|
| engine | `src-tauri/crates/keeper-core/src/bots/grant.rs` | new, 891 lines: `GrantMode`, `GrantScope` (serde-tagged, ts-exported), `Grant`, `ToolTarget`, `Effect`, `GrantVerdict`, `GrantOffer`, `SubpathError`, `MAX_SUBPATH_CHARS` (1024), the 9 refusal consts, `parse_subpath`, `subpath_is_safe`, `segments`, `covers_subpath`, `decide`, `check`, `grant_offer`; the `Offered { warning }` arm is the coordinator's mid-flight change |
| engine | `src-tauri/crates/keeper-core/src/bots/audit.rs` | new, 485 lines: `bot_audit` DDL + `bot_audit_session` index in its own idempotent opener, `AuditVerdict`, `AuditOutcome`, `AuditIntent`, `append_intent`, `complete`, `AuditRow`, `list_audit` |
| engine | `src-tauri/crates/keeper-core/src/bots/store.rs` | `bot_grants` DDL + `bot_grants_provider` index :149-170; `GrantRow`, `UnknownGrantRow`, `GrantListing`, `ensure_valid_scope`, `save_grant`, `revoke_grant`, `get_grant`, `list_grants`, `list_grants_for_bot` |
| engine | `src-tauri/crates/keeper-core/tests/bots_grant.rs` | new, 1124 lines, 40 integration tests against a real `keeper.db` and a real file |
| engine | `src-tauri/crates/keeper-core/src/bots/mod.rs`, `vm.rs` | `pub mod audit; pub mod grant;`; `BotGrantVm`, `UnknownBotGrantVm`, `BotGrantListVm`, `BotAuditRowVm` (+ compose), `BotGrantSaveReq` |
| engine | `src/lib/ipc/gen/BotGrantVm.ts`, `UnknownBotGrantVm.ts`, `BotGrantListVm.ts`, `BotAuditRowVm.ts`, `BotGrantSaveReq.ts`, `GrantMode.ts`, `GrantScope.ts`, `Effect.ts`, `AuditVerdict.ts`, `AuditOutcome.ts` | generated |
| engine | `src-tauri/crates/keeper/src/bots_ipc.rs` | `bots_grants_list` → `BotGrantListVm`; `bots_grant_save(req)` → `BotGrantVm`; `bots_grant_revoke(grantId)`; `bots_audit_list(sessionId?, limit?)` → `Vec<BotAuditRowVm>` — in the shared literal, none needs `keeper-sync` — **not compiled on this host** |
| UI | `src/components/bots/bot-grant-bar.tsx` (+ test, 23) | replaced, 466 lines: `botGrantOffer`, `liveGrantsFor`, `grantSentence`, `BotGrantBar`, `BotGrantEditor` (fields: what it may reach — drive/profile/subtree, folder, inside that folder, what it may do — none/read/write; option text is the stored spelling) |
| UI | `src/components/bots/bot-approval-dialog.tsx` (+ test, 15) | new, 253 lines: `BotApprovalRequest`/`BotApprovalAnswer`, `approvalSubtree`, `BotApprovalDialog`, `BotApprovalHost` (store-backed mount so the pane wiring is one line) |
| UI | `src/components/settings/bot-grants-section.tsx` (+ test, 14) | new, 275 lines: grants grouped by endpoint then bot; revoked and unreadable rows kept and labelled; audit log newest-first with honest pending rows |
| UI | `src/components/settings/settings-dialog.tsx` | `{bots && sync && <BotGrantsSection open={open} />}` after `BotsSection`, with the sync-gate comment |
| UI | `src/lib/ipc/client.ts`, `src/lib/stores/bots.ts`, `dev/mock-shell.ts` | 10 type re-exports and 4 wrappers; `BotApprovalAsk`, `pendingApproval`, `askApproval`/`clearApproval`; `BOT_GRANTS` + `BOT_AUDIT` fixtures whose save/revoke mutate the table |
| UI | `src/components/bots/bots-pane.tsx` | coordinator-owned wiring: `BotApprovalHost` import, `botId={selectedBotId}` on `<BotGrantBar>`, `<BotApprovalHost />` before `</section>` |

## Tasks & Acceptance

**Execution:**
- [x] `grant.rs`, `audit.rs`, `store.rs`, `vm.rs`, bindings, the four commands — the engine half.
- [x] `bot-grant-bar.tsx`, `bot-approval-dialog.tsx`, `bot-grants-section.tsx`, `settings-dialog.tsx`, wrappers, store, mock — the UI half.
- [x] The coordinator's correction applied in `grant::grant_offer`, not in the UI (see Design Notes).

**Acceptance Criteria:**
- Given a revocation between two tool calls, when the second runs, then it is denied with `DENY_REVOKED` and touches no byte. **Met** — `grant_a_revocation_between_two_calls_denies_the_second_with_the_revocation_reason`, on a real store and a real file.
- Given a drive-wide write grant, when the model writes, then keeper asks and never auto-approves. **Met** — `grant_a_write_under_a_drive_wide_write_grant_asks_and_is_never_auto_approved`; mutation (c).
- Given a crash between the audit row and the effect, when the log is read, then a `pending` row is there. **Met** — `grant_audit_a_crash_between_the_row_and_the_effect_leaves_a_readable_pending_row`; mutation (d).
- Given a Hermes bot, when the pane renders, then no grant control and the one sentence. **Met** — `the_grant_offer_is_absent_for_hermes_and_says_why`, *BotGrantBar > shows the Hermes sentence and offers no control*.

## Design Notes

**The coordinator's mid-flight correction, and where it was made.** The first UI pass had an `unknown` tools capability *hide* the grant control, following the then-current `grant_offer` (`None ⇒ Absent{TOOLS_CAPABILITY_UNKNOWN}`). On the coordinator's instruction the decision was changed in its home — `grant::grant_offer` in `keeper-core` — rather than in the UI: `None` now **offers** the grant and carries the warning, matching how 61.12 treats unknown vision and keeping the tri-state from collapsing to `false` (AD-27). Refusing on unknown would have stranded every Ollama predating the `capabilities` array; offering on unknown risks at most a tool call the model never makes, because every call still passes `decide` and, for a write, the approval path. `Some(false)` is now the only case that hides the affordance. `TOOLS_CAPABILITY_UNKNOWN` was reworded with the behaviour, and the source-parity test flagged the stale TypeScript copy immediately, which is the point of that test. Its guard: `an_unreadable_tool_capability_offers_the_grant_with_a_warning`; mutation (d, UI).

**`ON DELETE CASCADE`, and the comment it contradicts.** Wave A's `store.rs` opener says rusqlite leaves `PRAGMA foreign_keys` off. That is false on this build — the bundled SQLite is compiled with `SQLITE_DEFAULT_FOREIGN_KEYS=1`, proved by `grant_a_grant_cannot_name_a_provider_that_does_not_exist`. Given that, cascade is the only spelling that keeps `delete_provider`/`delete_bot` working, and it is what the feature wants: a permission must not outlive the endpoint it was about nor block its removal. Wave A's comment is now misleading for anyone adding a table; it was left because it is their line. If grants should survive a provider deletion for the audit's sake, the change is to drop the cascade and clear grants inside those two transactions — `grant_deleting_a_provider_takes_its_grants_with_it_and_is_not_blocked_by_them` pins whichever answer is chosen.

## Verification

- `cargo nextest run -p keeper-core -E 'test(grant) + test(audit)'` → 60 passed (47 of this story: 5 unit in `grant.rs` + 2 in `audit.rs` + 40 integration; 8 ts-rs export tests; 3 siblings whose names contain `grant`). `cargo check` / `cargo clippy -p keeper-core --all-targets -- -D warnings` → clean. `rustfmt --check` clean on `grant.rs`, `audit.rs`, `bots_grant.rs`.
- `bunx vitest run bot-grant-bar.test.tsx bot-approval-dialog.test.tsx bot-grants-section.test.tsx` → 52 passed (23 + 15 + 14). `bunx biome check` on the 12 UI-half files → clean. `bunx tsc --noEmit` → no error in any file touched.

| Mutation | Tests that failed | Observed |
|---|---|---|
| (a) `covers_subpath` → `target.starts_with(scope)` | `grant_a_subtree_matches_by_segment_so_a_shared_prefix_is_not_inside_it`, `grant_a_deeper_subtree_also_matches_by_segment` | 2 failed / 38 of 40 |
| (b) specificity `.max()` → `.min()` | `…most_specific_grant_wins…`, `…deepest_subtree_wins…`, `…bot_scoped_grant_beats_the_provider_wide_one…`, `…mode_none_is_an_explicit_refusal…`, `…revocation_of_one_grant_leaves_a_wider_live_grant_in_force` | 5 failed / 35 |
| (c) (Write, write, Drive\|Profile) `Ask` → `Allow` | `…drive_wide_write_grant_asks…`, `…profile_wide_write_grant_also_asks`, `grant_audit_an_ask_is_recorded_as_an_ask…` | 3 failed / 37 |
| (d) `append_intent` writes `finished_ms = started_ms`, `outcome = 'ok'` | the five `grant_audit_*` tests and `…revocation_between_two_calls…` | 6 failed / 34 |
| (a, UI) `provider.kind === "hermes"` arm removed from `botGrantOffer` | *refuses a Hermes bot with the one sentence*, *shows the Hermes sentence and offers no control* | 2 failed / 20 |
| (b, UI) dialog's `always()` skips `botsGrantSave` | *saves a write grant on the path's own folder…*, *does not approve when the grant write was refused*, *grants read, not write, for a read…* | 3 failed / 12 |
| (c, UI) deny dropped from `onOpenChange` | *denies on Escape, and saves no grant* — the Deny-button test still passed, which is why Escape has its own | 1 failed / 14 |
| (d, UI) `grant_offer`'s `None` arm back to `Absent` | `an_unreadable_tool_capability_offers_the_grant_with_a_warning` | 1 failed / 2 |

Every restore verified by `diff` against a pre-mutation copy (empty / byte-identical). The coordinator's full gate afterwards: 4028 Rust tests, 5466 frontend tests, lint/typecheck/fmt/clippy clean.

## Auto Run Result

Status: implemented and gated; uncommitted; not reviewed.

**Not verified here, and why.** The four commands and their three `use` lines were never compiled — every referenced name was checked against `keeper-core`, which does compile, but a type or arity mistake is what a macOS `cargo check -p keeper` would catch. The dialog has never been raised by a real `Ask`: the Rust approval port (`DriveToolHost.approve`, 61.11's shell) has no IPC transport filling it yet, so `askApproval` is driven directly in tests and FR-387's second half is proven at the decision boundary, not at the button. Revocation mid-flight is proven in `grant::check` and the store, not through the revoke control. No real filesystem containment is exercised here — `ToolTarget` is a profile id plus a profile-relative subpath; the containment rule is 61.11's `keeper-sync` half, and TOCTOU between check and effect is that layer's problem. `rustfmt` was run on four shared files (`vm.rs`, `store.rs`, `mod.rs`, `bots_ipc.rs`) because the `#[serde(tag = …)]` attribute needed rewrapping and rustfmt has no line-range mode; it cannot be proven not to have rewrapped a sibling's block. No pixels.
