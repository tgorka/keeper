---
title: 'Account Switcher and Per-Account State'
type: 'feature'
created: '2026-07-04'
status: 'done'
baseline_revision: 'fd27c2ddbaa478677b6322b78290674fe2542f7c'
final_revision: '645ded7458f95bceec7682a1c5bc12fad9535221'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** With ≥2 accounts a keeper user has no legible per-account control: the sidebar footer (Story 2.1's throwaway) lists only user ids with a sign-out button, the shell offline pill reflects only `accounts[0]` (2.1 deferred item), there is no way to focus the merged inbox on one account, and Beeper identity is inferred from the resolved homeserver host (2.4 deferred item) which silently breaks if Beeper's `.well-known` moves. FR-6/FR-4 and UX-DR18/UX-DR20 require a real account switcher with per-account sync state, click-to-filter, and a per-account menu.

**Approach:** (1) Rust: add a durable `provider` discriminant (`password | oidc | beeper`) to the `keeper.db` account registry + `AccountVm`, stamped by each `AuthProvider` at add time, with a one-time host/session-shape migration for existing rows (mirroring the existing `hue_index` column migration). (2) Frontend: rebuild the footer into the designed switcher (initials avatar, hue dot, homeserver, 3-state sync glyph, per-row DropdownMenu with Settings / Beeper coverage / Sign out…), add click-to-filter of the merged inbox, key `isBeeperAccount` off `provider`, and consolidate connection-status into a single per-account subscriber feeding both the switcher glyphs and the shell offline pill.

## Boundaries & Constraints

**Always:**
- The `provider` tag is set once at add time by the authenticating provider (`password`/`oidc`/`beeper`), persisted in the non-secret `keeper.db` registry row (never in the Keychain session blob, never a secret), and surfaced on `AccountVm.provider`.
- Registry migration is non-destructive and idempotent, exactly like `ensure_hue_index_column`/`backfill_hue_index`: add a nullable `provider` column; a legacy row (NULL) is backfilled by inference — `Oauth` stored-session shape → `oidc`; `Password` shape with homeserver host `matrix.beeper.com` → `beeper`; otherwise → `password` — then persisted so the inference runs once.
- `isBeeperAccount` keys off `account.provider === "beeper"` (no longer the homeserver host).
- The switcher lists every account with an avatar (initials fallback tinted by the account hue — no network avatar fetch), a hue dot, the homeserver, and a sync-state glyph, plus an always-present, never-count-gated "Add Account" entry (FR-4/FR-6, UX-DR18).
- The sync-state glyph is a passive projection of the per-account connection-status stream: no status batch yet for that account → syncing spinner; `online` → synced; `offline` → offline (gray). No toasts anywhere in this path (AC3 "no toast spam").
- Clicking an account row filters the merged inbox to that account's chats; clicking the same account again clears the filter. The filter is ephemeral frontend display state over the already-merged `roomsStore` (rooms carry `accountId`); no new inbox IPC, backend subscription stays merged.
- Each row's DropdownMenu offers Settings (opens the existing `SettingsDialog`), Beeper coverage (Beeper accounts only, opens the existing `BeeperCoverageDisclosure`), and "Sign out…" opening an AlertDialog whose default framing is "Sign out, keep local archive" and which performs Story 1.8/2.1 local sign-out semantics (destructive archive deletion stays out — Story 5.7).
- `use-account-statuses` (new) is the single connection-status subscriber, subscribing every account and populating a per-account status store; the shell offline pill and the conversation "Queued" caption are derived from that map. `use-connection-status`/`connectionStore` are retired.
- All existing quality gates pass; ts-rs regenerates `AccountVm.ts` + a new `Provider.ts`; `keeper-core` stays tauri-free.

**Block If:**
- The `AccountVm`/registry cannot carry a `provider` tag without a schema change that would drop or rewrite existing account rows destructively (none anticipated — the `hue_index` migration proves the pattern).

**Never:**
- Do not add a Matrix/`thirdparty/protocols`/hungryserv coverage probe or any network call for provider detection (provider is known locally at add time; migration infers from stored shape + host).
- Do not store the provider tag (or any new field) in the Keychain session blob or expose tokens/secrets over IPC.
- Do not expand `ConnectionStatus` beyond `online|offline`, add per-account inbox subscription IPC, add a real avatar/media fetch (Epic 3), or build a per-account settings screen (Settings reuses the existing global `SettingsDialog`).
- Do not re-derive inbox ordering/filtering in a way that sorts or mutates Rust-owned room data — the account filter only hides non-matching rows for display.
- No softening copy, no exclamation marks, no `any`, no `.unwrap()` in Rust production paths.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Add password account | `PasswordAuthProvider` completes | registry row + `AccountVm` carry `provider = "password"` | login errors unchanged |
| Add OIDC account | `OidcAuthProvider` completes | `provider = "oidc"` | unchanged |
| Add Beeper account | `BeeperAuthProvider` completes | `provider = "beeper"` | unchanged |
| Restore legacy row (NULL provider), Beeper | stored `Password`, host `matrix.beeper.com` | backfilled + persisted as `beeper`; VM carries `beeper` | absent session → row skipped (unchanged) |
| Restore legacy row (NULL provider), OIDC | stored `Oauth` shape | backfilled + persisted as `oidc` | — |
| Restore legacy row (NULL provider), other | stored `Password`, non-Beeper host | backfilled + persisted as `password` | — |
| `isBeeperAccount` | `provider === "beeper"` | `true`; any other provider → `false` | never throws |
| Sync glyph, pending | no status batch yet for account id | syncing spinner | — |
| Sync glyph, online / offline | account status `online` / `offline` | synced glyph / offline (gray) glyph | — |
| Click account row | account A row clicked | inbox shows only account A's rooms; row marked active | — |
| Click active account again | same row clicked | filter cleared; inbox shows all accounts | — |
| Sign out filtered account | filter = A, A signed out | filter clears; its status-map entry + subscription removed; other accounts keep syncing | cleanup failure keeps account signed in (unchanged) |
| Shell offline pill | ≥1 account and every account `offline` | pill shown | pending/absent status ≠ offline (no false flash) |
| Shell pill, mixed | one account offline, another online | pill hidden (per-account glyph shows the offline one) | — |

</intent-contract>

## Code Map

**Rust (src-tauri/crates/keeper-core/src/):**
- `vm.rs` — `AccountVm` (lines ~373–397): add `pub provider: Provider`. NEW `Provider` enum (`Password|Oidc|Beeper`, `#[serde(rename_all = "lowercase")]`, `#[ts(export)]`) → generates `src/lib/ipc/gen/Provider.ts` union `"password"|"oidc"|"beeper"`.
- `registry.rs` — add nullable `provider TEXT` column via a new `ensure_provider_column` (mirror `ensure_hue_index_column`, called from `open`); `AccountRow.provider: Option<String>`; `insert_account` gains a `provider: &str` param; `list_accounts`/`get_account` select it; NEW `backfill_provider(data_dir, account_id, provider)` (mirror `backfill_hue_index`, idempotent UPDATE … WHERE provider IS NULL).
- `auth.rs` — `AuthProvider` trait (line 63): add `fn provider(&self) -> Provider;`; impl on `PasswordAuthProvider`/`OidcAuthProvider` (this file). `add_account` (466–595): read `provider.provider()`, pass to `insert_account`, set `AccountVm.provider`. `find_restorable_accounts` (609–635): for a NULL-provider row, infer (read Keychain `StoredSession::from_json` for `Oauth`→`oidc`; else host `matrix.beeper.com`→`beeper`; else `password`), `backfill_provider`, and set the VM.
- `auth/beeper.rs` — `BeeperAuthProvider::provider()` returns `Provider::Beeper`; reuse `BEEPER_HOMESERVER` host for the migration match.

**Frontend (src/):**
- `lib/ipc/gen/AccountVm.ts`, `lib/ipc/gen/Provider.ts` — regenerated by ts-rs (do not hand-edit).
- `lib/beeper.ts` — `isBeeperAccount(a)` → `a.provider === "beeper"`; keep the host constant only if still referenced, update the doc note; update `beeper.test.ts`.
- `lib/stores/account-status.ts` — NEW vanilla store: `Record<accountId, ConnectionStatus>`; `setStatus`/`removeAccount`/`reset`; `useAccountStatus(accountId)` selector (returns `status | undefined`); `useShellOffline()` derived selector (true iff ≥1 account and all `offline`).
- `hooks/use-account-statuses.ts` — NEW: single subscriber; on the account-id set, subscribe `connection_status` for every account, mirror batches into `account-status` store, tear down per account on removal/unmount (subsumes `use-connection-status`).
- `lib/stores/accounts.ts` — add ephemeral `filterAccountId: string | null`, `toggleFilter(id)`; clear filter in `removeAccount` (if it matches) and `hydrateAll`.
- `components/layout/account-footer.tsx` — rebuild `AccountRow` into the switcher row: avatar (initials, hue-tinted) + hue dot + homeserver + sync glyph; clickable to toggle inbox filter (active-state styling); DropdownMenu (Settings → SettingsDialog; Beeper coverage for Beeper rows; Sign out… → AlertDialog). Keep the never-gated Add Account entry; keep collapsed rail behavior.
- `components/layout/chat-list-pane.tsx` — read `filterAccountId`; when set, render only rooms whose `accountId` matches (display filter only; subscription unchanged).
- `components/layout/app-shell.tsx` — mount `useAccountStatuses()` instead of `useConnectionStatus()`.
- `components/layout/sidebar-pane.tsx` — offline pill from `useShellOffline()`.
- `components/layout/conversation-pane.tsx` — `offline` from the open conversation's account status (`useAccountStatus(selected?.accountId)` === `"offline"`).
- `hooks/use-sign-out.ts` — remove the signed-out account's `account-status` entry instead of `connectionStore.reset()`.
- DELETE `lib/stores/connection.ts`, `hooks/use-connection-status.ts` (+ their tests); update all references.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- Add `Provider` enum (ts-rs export) and `AccountVm.provider`.
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- Add `provider` column (idempotent migration), `AccountRow.provider`, `insert_account` param, reads, and `backfill_provider`; unit-test the migration (legacy row survives NULL then backfills) mirroring the hue tests.
- [x] `src-tauri/crates/keeper-core/src/auth.rs` + `auth/beeper.rs` -- Add `AuthProvider::provider`, stamp at `add_account`, and the NULL-provider inference+backfill in `find_restorable_accounts`; unit-test each provider's tag and the three migration inferences.
- [x] `src/lib/beeper.ts` (+ test) -- Key `isBeeperAccount` off `provider`.
- [x] `src/lib/stores/account-status.ts` (+ test) -- Per-account status map with `useAccountStatus`/`useShellOffline`.
- [x] `src/hooks/use-account-statuses.ts` (+ test) -- Single all-account connection subscriber with per-account teardown; delete `use-connection-status.ts`.
- [x] `src/lib/stores/accounts.ts` (+ test) -- `filterAccountId` + `toggleFilter`, cleared on sign-out/hydrate.
- [x] `src/components/layout/account-footer.tsx` (+ test) -- Designed switcher row (avatar/hue/homeserver/glyph/menu) + click-to-filter; Add Account never gated.
- [x] `src/components/layout/chat-list-pane.tsx` (+ test) -- Apply the account display filter.
- [x] `src/components/layout/{app-shell,sidebar-pane,conversation-pane}.tsx` + `src/hooks/use-sign-out.ts` -- Migrate off `connectionStore` to the per-account store; delete `connection.ts`; update tests.

**Acceptance Criteria:**
- Given ≥2 connected accounts, when the sidebar footer renders, then the switcher lists every account with avatar, hue dot, homeserver, and sync-state glyph, plus an always-present, never-count-gated "Add Account" entry (FR-4/FR-6, UX-DR18).
- Given an account row, when the user clicks it, then the inbox filters to that account and clicking it again clears the filter; its DropdownMenu offers Settings and "Sign out…" opening an AlertDialog defaulting to keep-local-archive sign-out (FR-6, UX-DR20).
- Given a per-account sync-state change, when the status stream emits, then that account's glyph updates within one sync cycle with no toast spam (AC3).
- Given a Beeper account (added or restored), when identity is checked, then `provider === "beeper"` drives Beeper-specific UI regardless of the resolved homeserver host, and existing sessions are migrated once without losing any account row.
- Given `bun run check` and `bun run check:rust` and `bun run test:rust`, then biome + tsc strict + vitest and rustfmt + clippy (`-D warnings`) + nextest all pass, including new/updated tests and the regenerated ts-rs bindings.

## Design Notes

**Provider home = registry, not Keychain.** `find_restorable_accounts` builds `AccountVm` from non-secret registry rows and only checks Keychain *presence*; putting `provider` in the registry keeps restore a row read (no blob parse on the hot path) and matches AD-10 ("keeper.db holds the account registry"). The migration is the *only* place that parses a legacy blob (`StoredSession::from_json`), and only to distinguish `oidc` from `password` for rows created before the column — beeper vs password uses the same homeserver-host signal Story 2.4 used, now demoted to a one-time inference.

**Migration mirrors hue.** Follow `ensure_hue_index_column` (idempotent `PRAGMA table_info` guard + `ALTER TABLE ADD COLUMN`) and `backfill_hue_index` (idempotent `UPDATE … WHERE provider IS NULL`) exactly; the existing `migration_adds_hue_column_to_legacy_table_without_dropping_rows` test is the template for the provider migration test.

**3-state glyph from a 2-state stream.** Backend `ConnectionStatus` is `online|offline`; the third UI state is "no batch yet for this account id" (`undefined` in the map) → spinner. Shell offline = ≥1 account and *all* offline (retires the `accounts[0]` positional pill without flashing on a single re-auth); pending never counts as offline (preserves the current default-online no-flash behavior).

**Filter is display-only.** The merged inbox stays a single Rust-computed stream; the switcher toggles `filterAccountId` and `chat-list-pane` hides non-matching rows. This keeps the story frontend-scoped for the inbox while honoring "ordering/filtering never re-derived in TypeScript" (no sorting, no mutation — only visibility).

## Verification

**Commands:**
- `bun run check:all` -- expected: biome + tsc strict + vitest, rustfmt + clippy (`-D warnings`) + nextest, and `tauri build --no-bundle` all green, including regenerated `AccountVm.ts`/`Provider.ts` and all new/updated tests.
- `bun run test:rust` -- expected: registry provider-migration test and auth provider-tag/inference tests pass.

**Manual checks (if no CLI):**
- With ≥2 accounts, confirm each switcher row shows avatar/hue/homeserver/glyph, clicking filters the inbox and clicking again clears it, and the row menu opens Settings and the keep-archive sign-out dialog.
- Restore a profile that has a pre-migration Beeper account and confirm Beeper UI still appears (provider migrated), and that only-all-offline shows the shell pill.

## Spec Change Log

_No `bad_spec` loopback occurred; the spec was implemented as written._

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 1, low 1)
- defer: 1: (high 0, medium 0, low 1)
- reject: 12
- addressed_findings:
  - `[medium]` `[patch]` `useShellOffline` computed the shell offline pill over `Object.values(statuses)` (only accounts that had delivered a status batch), so a pending (absent) account was invisible to `.every()` and the pill showed offline whenever the one delivered status was offline — contradicting the spec ("pending/undefined does NOT count as offline") and the store's own doc/test. Fixed: `useShellOffline` now ranges over the signed-in account set (accounts store), so a pending account (`undefined`) makes `every(... === "offline")` false and keeps the pill hidden; corrected the contradictory test and added a signed-out-stale-status test.
  - `[low]` `[patch]` `addAccount` left `filterAccountId` untouched, so adding a second account while the inbox was filtered to the first left the new account's Chats hidden with no signposting. Fixed: `addAccount` clears the filter (like `hydrateAll`/`removeAccount`); added a test.
- notes: Blind Hunter + Edge Case Hunter reviewed the full baseline→working-tree diff. Deferred 1 (low): `use-account-statuses` tears down + rebuilds all per-account subscriptions on any account-set change, transiently spinning surviving accounts' glyphs — the same tear-all/rebuild pattern already deferred for the merged inbox in Story 2.1; delta subscription management is the shared fix. Rejected 12: `infer_legacy_provider` collapsing "parse failed" and "parsed non-Oauth" (by-design — a corrupt blob fails restore regardless, and the inference matches the spec); the legacy-Beeper host coupling in migration (the same `matrix.beeper.com` signal Story 2.4 relied on for 100% of real Beeper accounts — a one-time bootstrap, durable thereafter); non-NULL-unknown provider re-inference (no live trigger — only `password/oidc/beeper` are ever written; a pure forward-compat edge); `#[allow(clippy::too_many_arguments)]` on `insert_account` (the spec directed the param add; documented, round-trip-tested); silent sign-out-failure feedback (pre-existing behavior preserved verbatim); the map+sort+join status selector (works; identical pattern to `chat-list-pane`); the `useAccountStatus(accountId ?? "")` empty-string sentinel (safe; `statuses[""]` is undefined); the vendored shadcn `alert-dialog.tsx` unused exports (relaxed-lint `ui/`, coverage-excluded); collapsed-rail glyph contrast and initials ambiguity (cosmetic); and the global jsdom ResizeObserver/pointer-capture stubs (necessary Radix test infra).

## Auto Run Result

Status: **done**

**Summary.** Implemented Story 2.5 as a cohesive cross-layer change. Rust: a durable `Provider` discriminant (`password | oidc | beeper`) now lives on the non-secret `keeper.db` account registry and on `AccountVm`, stamped at add time by each `AuthProvider` (new `AuthProvider::provider()` on the password/OIDC/Beeper impls) and migrated once for legacy rows (`find_restorable_accounts` infers `Oauth`→`oidc`, else `matrix.beeper.com` host→`beeper`, else `password`, then `backfill_provider`), all via an idempotent, non-destructive `provider TEXT` column migration mirroring the existing `hue_index` pattern. Frontend: the sidebar footer became the designed account switcher — hue-tinted initials Avatar, hue dot, homeserver, a 3-state sync glyph (pending spinner / synced / offline gray) driven by a new single all-account connection-status subscriber (`use-account-statuses` → `account-status` store), click-to-filter of the merged inbox, and a per-row DropdownMenu (Settings / Beeper coverage / Sign out… → keep-local-archive AlertDialog). `isBeeperAccount` now keys off `provider`. The retired `connectionStore`/`use-connection-status` were replaced everywhere (shell pill via `useShellOffline`, conversation "Queued" caption via the open room's account status, sign-out via per-account `removeAccount`).

**Files changed (one-line each).**
- `src-tauri/crates/keeper-core/src/vm.rs` — new `Provider` enum (ts-rs export, lowercase serde, registry-string mapping) + `AccountVm.provider`; tests.
- `src-tauri/crates/keeper-core/src/registry.rs` — `ensure_provider_column` migration, `AccountRow.provider`, `insert_account` provider param, reads, `backfill_provider`; migration test.
- `src-tauri/crates/keeper-core/src/auth.rs` — `AuthProvider::provider`, add-time stamping, legacy-row `infer_legacy_provider`/`is_beeper_homeserver` + backfill in `find_restorable_accounts`; provider + 3-inference tests.
- `src-tauri/crates/keeper-core/src/auth/beeper.rs` — `BeeperAuthProvider::provider() → Beeper`.
- `src/lib/ipc/gen/Provider.ts` (new), `src/lib/ipc/gen/AccountVm.ts`, `src/lib/ipc/client.ts` — regenerated/exported bindings.
- `src/lib/beeper.ts` (+ test) — `isBeeperAccount` keys off `provider`.
- `src/lib/stores/account-status.ts` (+ test) — per-account status map; `useAccountStatus`; `useShellOffline` (over the signed-in set — patched).
- `src/hooks/use-account-statuses.ts` (+ test) — single all-account connection subscriber.
- `src/lib/stores/accounts.ts` (+ test) — `filterAccountId`/`toggleFilter`, cleared on add/sign-out/hydrate (add-clear patched).
- `src/components/layout/account-footer.tsx` (+ test) — designed switcher (avatar/hue/homeserver/glyph/menu) + click-to-filter.
- `src/components/layout/chat-list-pane.tsx` (+ test) — display-only account filter.
- `src/components/layout/app-shell.tsx`, `sidebar-pane.tsx` (+ test), `conversation-pane.tsx` (+ test), `src/hooks/use-sign-out.ts` (+ test) — migrated off `connectionStore`.
- `src/components/ui/alert-dialog.tsx` (new, shadcn), `src/test/setup.ts` (Radix jsdom stubs), and `AccountVm` fixtures in `App.test.tsx` / `login-screen.test.tsx` / `use-session-restore.test.ts`.
- Deleted `src/hooks/use-connection-status.ts` (+ test), `src/lib/stores/connection.ts` (+ test).
- `_bmad-output/implementation-artifacts/deferred-work.md` — one new deferred entry (subscription churn on account-set change).

**Review findings breakdown.** intent_gap 0, bad_spec 0, patch 2 (1 medium + 1 low, both fixed), defer 1 (low), reject 12. Patches: `useShellOffline` pending-account semantics (medium) and `addAccount` filter-clear (low). Deferred: tear-all/rebuild per-account subscription churn on account-set change (mirrors the deferred 2.1 inbox re-subscribe).

**Verification.** After patches: `bun run check:rust` PASS (rustfmt + clippy `-D warnings` clean); `bun run test:rust` PASS (171 tests); `bun run check` PASS (biome 99 files clean, `tsc --noEmit` strict clean, vitest 25 files / 237 tests, core-tauri-free guard PASS). `check:all`'s `bindings:check` only flags the newly generated `Provider.ts`/`AccountVm.ts` as uncommitted — resolved by this commit; regeneration is stable (re-running `test:rust` yields no further diff).

**Residual risks.** The legacy-provider migration reuses the `matrix.beeper.com` host signal (Story 2.4's approach) as a one-time bootstrap; it is correct for every real Beeper account today (their resolved homeserver is `matrix.beeper.com`) and becomes durable thereafter, but a legacy Beeper row with a non-standard stored host would migrate as `password`. Adding/removing an account briefly re-subscribes all per-account status streams (deferred), flashing surviving glyphs to the syncing spinner for one sync cycle (the shell pill no longer flashes after the `useShellOffline` fix). The OQ-3 real-Beeper manual check remains non-automatable here.
