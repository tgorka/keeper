---
title: 'Skipping setup can stick'
type: 'feature'
created: '2026-09-13'
status: 'done'
review_loop_iteration: 1
followup_review_recommended: true
context: []
baseline_revision: '53e859eedcfbe51b112c5ddf575a2c49503c2f18'
final_revision: '3b6a0d6'
warnings: []
---

<intent-contract>

## Intent

**Problem:** A keeper install with no Matrix Account meets the full-frame First-Run Wizard on every launch: `wizardStore.dismissed` is session-scoped and never persisted (`src/lib/stores/wizard.ts:10-14`), so the skip is forgotten the moment the app restarts — which is exactly what an update does. A field report (Marta, v0.8.28) is that updating keeper re-opens onboarding she had already skipped. Story 6.8 wrote that re-offer down as intended ("a relaunch with zero accounts legitimately re-offers the wizard", `spec-6-8-first-run-wizard.md:93`, and `Never: no persistence of the "seen/dismissed" flag`, `:28`). This spec renegotiates that clause deliberately: the wizard may be re-offered, but only until a person says not to.

**Approach:** Give the skip-confirm one checkbox that persists the answer in the Rust `settings` k/v table (`ui.first_run_setup_skipped`, Story 14.2's `ui.ios_sync_disclosure_shown` pattern), and gate the boot auto-start on it. Two-way, not a one-way latch: the same checkbox — pre-filled from the stored answer — brings the wizard back when it is cleared, and Settings → "Run setup again" keeps setup reachable either way.

## Boundaries & Constraints

**Always:** The persisted answer is device-global, set only from the skip-confirm the person is looking at, and is a `Flag01` settings key no config file may set. A boot that has not yet read it renders the splash, never a flash of wizard or login. Every read and write is best-effort: a failed read means *not skipped* (a first run still gets its wizard — one click skips it again), a failed write still closes the wizard for this session. A suppressed boot with zero accounts lands where a skip lands today — the shell's empty Inbox with its add-account footer — never the bare login screen. Copy is sentence case, no exclamation marks (UX-DR10).

**Block If:** the skip would have to suppress anything besides the wizard auto-start (login gate, encryption choice, iOS disclosure), or the answer would have to be per-Account rather than device-global.

**Never:** No new Settings row (the existing "Run setup again" entry is the re-entry path, and the checkbox is the toggle). No change to what the wizard's own steps do, to `start()`'s welcome-vs-discovery routing, or to the Done step. No writing the flag from anywhere except the skip-confirm. No `localStorage`/cookie (a device fact Rust owns). No auto-start after a sign-out-of-last-account (unchanged). No suppression of the at-rest-encryption choice.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh install | key absent, zero Accounts, posture chosen | wizard auto-starts at `welcome` | No error expected |
| Skip with box ticked | confirm dialog, checkbox checked | `first_run_setup_skipped_set(true)` then `finish()`; lands in empty Inbox | Failed write: wizard still closes for this session |
| Next launch after that skip | key `"1"`, zero Accounts | no auto-start; `finish()` stands in for the skip, so the shell (empty Inbox) renders | No error expected |
| Skip with box unticked | key `"1"`, checkbox cleared by the person | `first_run_setup_skipped_set(false)`; next launch auto-starts again | Failed write: previous answer stands |
| Confirm opened | key `"1"` | checkbox renders already checked | Failed read: renders unchecked |
| Boot, read in flight | key unread, zero Accounts | accessible splash; no wizard, no login screen | Read rejects ⇒ treat as not skipped |
| Accounts present | any key value | no auto-start (unchanged); checkbox still offered in a Settings re-run | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/registry.rs:611-627` -- `ui.ios_sync_disclosure_shown` get/set over `get_setting`/`set_setting` (:208, :230); the shape to copy. Round-trip test at :3894.
- `src-tauri/crates/keeper-core/src/config/keys.rs:744-753` -- the `KeySpec` entry to model (`Scope::SessionState`, `Settable::Never(why)`, `Shape::Flag01`, `default: "0"`, `example: ""`). Guards: `:1281` classification, `:1614` doc table, `:1428-1434` refusal list.
- `src-tauri/crates/keeper/src/ipc.rs:10836-10853` + `lib.rs:1071-1072` -- command pair and its registration inside the **shared** `keeper_with_commands!` literal (both targets, no `Unsupported` twin).
- `src/lib/ipc/client.ts:2746-2762` -- wrapper style/placement for ui-latch commands.
- `src/App.tsx:96-148` -- posture tri-state and the one-shot boot decision the new read joins; `:195-208` the splash/login/shell branch.
- `src/components/wizard/first-run-wizard.tsx:49-121` -- wizard frame and the "Skip setup?" `AlertDialog`.
- `src/components/settings/key-backup-dialog.tsx:199-203` -- `Checkbox` + `Label` composition precedent.
- `dev/mock-shell.ts:3146-3156` -- `bots_message_details_get/set`: module-level `let` so a bool round-trips in `bun run dev`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- add `UI_FIRST_RUN_SETUP_SKIPPED_KEY = "ui.first_run_setup_skipped"` with `get_first_run_setup_skipped(&Path) -> Result<bool, CoreError>` (absent ⇒ `false`) and `set_first_run_setup_skipped(&Path, bool)` writing `"1"`/`"0"` -- two-way, because clearing the box must be expressible.
- [x] `src-tauri/crates/keeper-core/src/registry.rs` (tests) -- round-trip test: absent ⇒ false, `true` ⇒ true, `false` ⇒ false again, plus the stored `1`/`0` bytes and a stray value reading as not-skipped.
- [x] `src-tauri/crates/keeper-core/src/config/keys.rs` -- add the `KeySpec` and the key to the refusal list at `:1428-1434`; keeps `every_settings_key_in_the_sources_is_classified` green.
- [x] `docs/settings-keys.md` -- regenerate (never hand-edit) so `docs::the_checked_in_table_matches_the_registry` passes.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- `first_run_setup_skipped_get` / `first_run_setup_skipped_set(skipped: bool)` commands over the registry helpers, errors through `to_ipc_error`.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register both in the shared `keeper_with_commands!` literal (never a second `invoke_handler`).
- [x] `src/lib/ipc/client.ts` -- `firstRunSetupSkippedGet()` / `firstRunSetupSkippedSet(skipped)` wrappers beside the ios-latch pair.
- [x] `src/components/wizard/first-run-wizard.tsx` -- exported label const + a `Checkbox` in the skip-confirm, pre-filled from the stored answer (read once on mount, best-effort); confirming writes the checkbox value when it is known, then `finish()`; closing without confirming restores the stored answer; a failed write is named in a toast.
- [x] `src/App.tsx` -- tri-state `setupSkipped` read on mount under `SETUP_ANSWER_DEADLINE_MS`; the boot decision latches *what the boot was* and acts when the answer lands; `renderContent` reads the answer directly so no frame paints the login screen; the telemetry readiness gate includes it.
- [x] `dev/mock-shell.ts` -- module-level `let` + get/set handlers so the checkbox round-trips in `bun run dev`.
- [x] `src/components/wizard/first-run-wizard.test.tsx` -- the box reflects the stored answer; skipping persists ticked and cleared answers; a late read does not move it; an unreadable answer writes nothing; a failed write still leaves and says so; cancelling restores the stored answer.
- [x] `src/App.test.tsx` -- stored `true` ⇒ no auto-start, shell renders (even with `finish` neutered); unreadable ⇒ auto-start without waiting; never-answering ⇒ auto-start at the deadline; a sign-out mid-read ⇒ login screen, not onboarding.

**Acceptance Criteria:**
- Given a device whose stored answer is `true` and no Account, when keeper boots, then the wizard never mounts and the shell's empty Inbox renders.
- Given the wizard is open from Settings → "Run setup again", when the skip-confirm's checkbox is cleared and skip is confirmed, then the next boot with zero Accounts auto-starts the wizard again.
- Given `first_run_setup_skipped_get` rejects, when keeper boots with zero Accounts, then the wizard auto-starts (no trap, no silent suppression).

## Spec Change Log

## Review Triage Log

### 2026-09-13 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 11: (high 4, medium 4, low 3)
- defer: 0
- reject: 6: (medium 2, low 4)
- addressed_findings:
  - `[high]` `[patch]` A sign-out while the stored answer was still in flight could read as a first run and auto-start the wizard — the boot decision is now two halves: `App.tsx` latches *what the boot was* when the boot state resolves, and acts on that frozen fact when the answer lands. Test: "a sign-out while the stored answer is still in flight does not open setup".
  - `[high]` `[patch]` A suppressed boot could paint one frame of the login screen, because only the effect that calls `finish()` decided the landing — `renderContent` now reads the stored answer directly. Test: "renders the shell on a suppressed boot even if nothing records the dismissal".
  - `[high]` `[patch]` A read that hung (rather than rejected) held the whole app on the splash forever — bounded by `SETUP_ANSWER_DEADLINE_MS` (3 s ⇒ not skipped). Tests: "gives up on an answer that never arrives…", "treats an unreadable answer as an answer, without waiting out the deadline".
  - `[high]` `[patch]` A confirm after a failed read wrote `false` and destroyed a stored `true` — re-introducing the field-report bug. The wizard now writes only when the answer is known (read succeeded) or the box was touched. Test: "writes nothing when the stored answer could not be read and the box was not touched".
  - `[medium]` `[patch]` A late-arriving read overwrote a box the person had already ticked (and a comment claimed otherwise) — guarded by a touched ref; comment corrected. Test: "a late-arriving stored answer does not move the box under the person".
  - `[medium]` `[patch]` "Keep setting up" discarded the tick silently while the box kept showing it — closing without confirming now restores the stored answer. Test: "keeping setup re-opens the box on the stored answer, not on the abandoned tick".
  - `[medium]` `[patch]` A failed write was swallowed, so onboarding returning next launch was inexplicable — it now says so (`toast.error`). Test: "still leaves when the answer cannot be written, and says so".
  - `[medium]` `[patch]` `useTelemetryReadiness` did not include the new gate, so `appReady` could fire while the splash was up — condition extended.
  - `[low]` `[patch]` The Rust round-trip test did not pin the on-disk `1`/`0` representation the docs table publishes, nor the stray-value branch — both asserted now.
  - `[low]` `[patch]` The App suite's mock for the new command was untyped — typed `vi.fn<() => Promise<boolean>>()`.
  - `[low]` `[patch]` The stale `App.tsx` comment claiming `dismissed` can only come from the wizard's own `finish()`, and the dev-harness comment claiming a reload keeps the answer — both rewritten; `wizard.ts`'s module doc now names the persisted answer.
- rejected (with reason):
  - `[medium]` Completing setup from Settings does not clear a stored `true`, and a stale answer governs a later zero-account boot after a sign-out. Out of contract by design (**Never:** no writing the flag from anywhere except the skip-confirm) and non-trapping: that boot lands in the shell whose footer is "Add an account", and Settings → "Run setup again" is reachable on both tiers.
  - `[medium]` "Un-silencing requires abandoning setup again" — the box is deliberately the skip-confirm's, not a Settings row (**Never**); clearing it and confirming is one click from the wizard the Settings entry opens.
  - `[low]` Two reads of the same key per first-run boot (App's boot decision and the wizard's box). One row each, on two surfaces with different lifetimes; the wizard is also opened later from Settings and must read fresh.
  - `[low]` The checkbox label does not name the landing surface. It names exactly what is suppressed; the dialog's own description names the way back.
  - `[low]` React `act` warnings in pre-existing wizard tests — none observed (`grep -c "not wrapped in act"` = 0).
  - `[low]` A first-run boot can still paint a frame of the login screen before `start()` commits — pre-existing shape of the posture-driven auto-start, unchanged by this story.

## Design Notes

`finish()` is reused for the suppressed boot instead of a new store action: it already sets `active: false` and `dismissed` iff zero Accounts, which is precisely "a skip stands here". That keeps `dismissed` set by exactly one function (`wizard.ts`) and keeps the sign-out-of-last-account path identical to a person who skipped this session. The *landing* does not depend on that effect having run — `renderContent` reads the stored answer itself — because an effect commits after a paint, and that frame would be the login screen.

Two-way rather than Story 14.2's one-way latch: a one-way flag would need a second control somewhere to undo it, and the honest place for "stop opening this" is the same box that said it.

The box is an input for the pending skip, not a mirror of disk: only confirming writes, and closing the dialog any other way restores the stored answer. Two refs decide whether a confirm may write at all — a read that failed plus an untouched box is *no answer*, and writing the default `false` over a stored `true` would be this very bug, re-introduced.

## Verification

**Commands:**
- `bun run lint && bun run typecheck && bun run test` -- expected: green (347 files / 5 833 tests before this change; the new App and wizard cases on top).
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync` -- expected: green, incl. the registry round-trip and the `config::keys` classification/doc guards.
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --lib config::keys::tests::docs::regenerate -- --ignored` -- expected: rewrites `docs/settings-keys.md` with the new row.
- Mutation check (each guard removed in turn, suites re-run, source restored): the render-side stored-answer guard, the read deadline, the frozen boot fact, the rejection path, the write-only-when-known rule, the touched-box rule and the cancel reset each have a test that fails without them.

**Manual checks:**
- Real engine, not jsdom: the dev harness (`bun run dev`) driven through Chrome on the macOS host over tunnelled CDP. The wizard auto-starts on a zero-Account boot; the confirm renders the box unchecked; ticking it and confirming closes the wizard, lands on the shell, and `first_run_setup_skipped_get` answers `true` over the real IPC wire. With the harness seeded `true`, boot renders the shell (empty chat list, "Add account" footer) with no wizard and no login screen.
- Note what the harness cannot show: its answer is a module-level `let`, so a page reload starts at `false`. Persistence across launches is the `settings` table's, covered by the registry test.
- `src-tauri/crates/keeper/src/{ipc,lib}.rs` cannot be compiled on the Linux dev host (AGENTS.md); the command pair is written by inspection here and gated by CI's macOS `Rust (fmt, clippy, test)` job.

## Auto Run Result

Status: done. Blocking condition: none.

**Implemented.** A skipped first-run setup now stays skipped across launches. One checkbox in
the wizard's skip-confirm ("Don't open setup when keeper starts") writes a device-global
two-way answer into the Rust `settings` table (`ui.first_run_setup_skipped`); the boot honours
it and lands where a skip lands — the shell's empty Inbox — and Settings → "Run setup again"
still opens the wizard, where clearing the same box brings startup setup back.

**Files changed**
- `src-tauri/crates/keeper-core/src/registry.rs` — the key plus `get_/set_first_run_setup_skipped`, and a round-trip test that also pins the stored `1`/`0` and a stray value.
- `src-tauri/crates/keeper-core/src/config/keys.rs` — the `KeySpec` (`SessionState`, `Settable::Never`, `Flag01`) and the never-from-a-file refusal list.
- `docs/settings-keys.md` — regenerated row.
- `src-tauri/crates/keeper/src/ipc.rs`, `lib.rs` — the command pair, registered in the shared handler literal (both targets).
- `src/lib/ipc/client.ts` — the two wrappers.
- `src/components/wizard/first-run-wizard.tsx` — the label const, the checkbox, the read/write rules, the cancel reset, the named write failure.
- `src/App.tsx` — the bounded read, the two-half boot decision, the render-side landing, the telemetry gate.
- `src/lib/stores/wizard.ts` — module doc names the persisted answer.
- `dev/mock-shell.ts` — a round-tripping harness answer.
- `src/App.test.tsx`, `src/components/wizard/first-run-wizard.test.tsx` — 12 new cases.

**Review** — 2 passes in parallel (adversarial, edge-case): 11 findings patched (4 high), 6 rejected with reasons, 0 deferred, 0 spec loopbacks. See the triage log above.

**Verification** — every command in Verification above ran green; each of the seven guards was mutation-checked (removed, suite red, restored); the behaviour was driven in Chrome on the macOS host against the dev harness over tunnelled CDP.

**Residual risk** — `crates/keeper/src/{ipc,lib}.rs` were written by inspection (the shell crate does not compile on this Linux host) and are gated by CI's macOS `Rust (fmt, clippy, test)` job. A follow-up review is recommended: the review pass changed boot ordering and the render-time landing decision, which is the kind of change worth a second pair of eyes.
