---
title: 'Story 61.1: a provider is something keeper remembers'
type: 'feature'
created: '2026-09-02'
status: 'review'
baseline_revision: '9e06c2b'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-369, FR-370, FR-371 — AD-146, AD-147, AD-148 (extends AD-53); AD-139 consumed (no JSON blob in a row)
depends on: nothing in this epic. First of the strictly serial pair: 61.2 cannot be tested without a provider record.

<intent-contract>

## Intent

**Problem:** keeper holds the drive, the notes, the sessions and the tasks, and has no record of an AI endpoint it could ask about them.
The one multi-tenant precedent is `SyncProfile` — several endpoints of one kind, each with its own credential. **A provider is a record, not a setting.**

**Approach:** Two tables in `keeper.db` beside the account registry; a closed `ProviderKind { Hermes, Ollama }`; a base-URL grammar that
is a `keeper-core` decision with its own refusals; the credential behind the existing secret port and never in a row; and egress
*derived* — `compute_egress` gains a fourth parameter and emits one host-only row per distinct provider host through `remote_host`.

## Boundaries & Constraints

**Always** (each invariant with the tests that pin it):
- No credential column exists and none can be added: the token is behind `Platform::keychain_*` under
  `bot_provider_token/<provider_id>`, tried after `bot_token/<provider_id>/<bot>`, and the grammar refuses userinfo —
  `a_base_url_carrying_userinfo_is_refused_by_name`, `secret_keys_are_namespaced_by_provider_and_bot`.
- Egress is a **host**, never a URL, computed on every read: adding a provider adds its host, removing it removes it —
  `a_provider_is_disclosed_as_a_host_and_never_as_a_url`, `two_providers_on_one_host_are_one_row_and_a_pathless_url_is_none`,
  `adding_a_provider_adds_its_host_and_removing_it_removes_the_entry`, `every_egress_kind_is_produced_by_the_disclosure_matrix` (extended).
- A Hermes bot is addressed by the `/p/{bot}` prefix `Endpoint::url` inserts; an Ollama bot never changes the URL —
  `a_hermes_bot_is_addressed_by_the_profile_prefix`, `a_hermes_endpoint_without_a_bot_has_no_prefix`,
  `an_ollama_endpoint_ignores_the_bot_for_routing`, `joining_never_doubles_or_drops_a_separator`.
- The kind is closed at two; a row whose `kind` this build cannot read is surfaced as `UnknownProviderRow`, never dropped, and contributes
  **no** egress row (the `UnknownTaskVm` posture) — `a_provider_kind_round_trips_and_an_unknown_kind_is_refused`,
  `a_provider_of_an_unknown_kind_is_surfaced_rather_than_dropped`, `the_wire_tags_are_the_frontend_contract`.
- Health is three scalar columns and identity three more (AD-139); unmeasured health is `Unknown` — `health_round_trips_and_defaults_to_unknown`,
  `a_provider_round_trips_and_starts_with_no_health_it_did_not_measure`, `health_and_the_timeout_override_are_written_and_read_as_scalars`.
- Loopback and private-network hosts are legitimate **and marked** — the SSRF question is answered by disclosure and an explicit user
  act, not a blocklist; `is_private` on the VM is `Option<bool>`, a URL that no longer parses being *could not tell*, never `false` —
  `loopback_and_lan_hosts_are_accepted_and_marked_private`, `a_public_host_is_not_marked_private`, `the_grammar_refuses_each_shape_by_name`.
- The bot-target grammar is a door (`ensure_valid_target` at `insert_bot`/`update_bot`); `UNIQUE(provider_id, target)`; the hand order
  is rewritten as one `BEGIN IMMEDIATE`; `delete_provider` is atomic; the tables share `keeper.db` and reopen idempotently —
  `a_bot_target_that_would_retarget_the_url_is_refused`, `a_target_that_never_passed_the_grammar_cannot_be_stored`,
  `pinning_one_bot_of_one_provider_twice_is_refused`, `the_hand_order_is_rewritten_as_a_unit_and_lists_back_in_it`,
  `deleting_a_provider_takes_its_bots_and_leaves_every_other_row`, `reopening_the_database_is_idempotent_and_keeps_every_row`,
  `the_bots_tables_share_keeper_db_with_the_account_registry`.
- **Foreign keys are enforced.** The hand-off report said "declared, not enforced: rusqlite leaves `PRAGMA foreign_keys` off". Story
  61.10 measured that false on this build: `libsqlite3-sys`' bundled SQLite is compiled with `SQLITE_DEFAULT_FOREIGN_KEYS=1`, so
  `bots.provider_id REFERENCES bot_providers(id)` **is** enforced — pinned by `grant_a_grant_cannot_name_a_provider_that_does_not_exist`
  in `tests/bots_grant.rs`. The module doc at `store.rs:51-58` and the table comment at `:134-141` were corrected; `delete_provider`
  still deletes bots first inside its transaction, and any later table here must either cascade or delete children first.

**Block If:** nothing; every decision has a precedent in `registry.rs`, `egress.rs` or `platform.rs`. **Never:** a JSON blob in a row; a hand-listed host; a third `ProviderKind` (DW-214); a default endpoint (61.13's D-4).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Accept | `http://127.0.0.1:11434`, `https://gw.example.org/v1` | normalized, host, port, `is_private` set; prefix kept, trailing slash dropped | — |
| Refuse | userinfo, non-http scheme, missing host, … | seven refusals, each by name | `BaseUrlError` |
| Dot segments; odd slash | `/a/../b`; `http:///v1` | `/b` (resolved by `url` 2.x, never stored); `v1` read **as the host** and accepted | pinned, not refused |
| Egress | two providers on one host; one with a path | one row, host only, kind `botProvider` | no row for a URL yielding no host |
| Bot target | a Hermes target that would retarget the URL | refused at `parse_bot_target`, and at the store door | `CoreError::Internal` at the door |

</intent-contract>

## Code Map

| file | state | what shipped |
|---|---|---|
| `src-tauri/crates/keeper-core/src/bots/mod.rs` | new, 640 | `ProviderKind`, `Provider`, `Bot`, `BotIdentity`, `BotHealthState` (Unknown/Reachable/Unreachable/Unauthorized/SecretMissing; total read defaulting Unknown), `ProviderHealth`, `Endpoint::new`/`url`, `provider_token_key`, `bot_token_key`, `resolve_token`, save/delete for both; the `pub mod` block shared with siblings |
| `src-tauri/crates/keeper-core/src/bots/store.rs` | new, 1064 | `bot_providers` + `bots` DDL with an idempotent WAL opener, CRUD, deterministic order, `ProviderRow`/`UnknownProviderRow`/`ProviderListing`, `provider_base_urls`, atomic `delete_provider`, `reorder_bots`, `ensure_valid_target`; 15 tests over real temp SQLite files |
| `src-tauri/crates/keeper-core/src/bots/url.rs` | new, 508 | `parse_base_url -> BaseUrl{normalized,host,port,is_private}`, `BaseUrlError` (7 refusals), `parse_bot_target`, `BotTargetError`; 8 tests |
| `src-tauri/crates/keeper-core/src/lib.rs`; `vm.rs` | +1; +289/-3 | `pub mod bots;`; `BotProviderVm`(+compose), `BotVm`(+compose), `EgressKind::BotProvider` and its wire-tag doc |
| `src-tauri/crates/keeper-core/src/egress.rs` | +190/-28 | 4th parameter `provider_base_urls`; one row per distinct host via `remote_host`; `NO_PROVIDERS` on all 15 existing rows; exhaustiveness gate extended; 3 new tests |
| `src-tauri/crates/keeper/src/ipc.rs` | +24/-13 | `egress_list` reads `bots::store::provider_base_urls` and passes it — the signature's only caller. **Written, not compiled here** |
| `src/lib/ipc/gen/{BotProviderVm,BotVm,ProviderKind,BotHealthState}.ts`, `EgressKind.ts` | generated | ts-rs export; `EgressKind` now `homeserver\|beeper\|gitRemote\|botProvider\|update` |

DDL: `bot_providers(id TEXT PK, kind, name, base_url, created_ms, health_state, health_checked_ms, health_detail, read_timeout_ms)`; `bots(id TEXT PK,
provider_id REFERENCES bot_providers(id), target, name, pin_order, identity_shape, identity_colour, identity_mark, created_ms, UNIQUE(provider_id, target))`. No new dependency.

## Tasks & Acceptance

**Execution:** [x] vocabulary and secret routing · [x] the two tables and their guards · [x] the grammar · [x] VMs and the egress kind ·
[x] derived egress · [x] the `ipc.rs` call site · [x] bindings regenerated. **Acceptance Criteria:** a provider row holds kind, name, base
URL, timeout override and health, and its credential is reachable only through the secret port — **met**; the grammar refuses each shape
by name and marks private hosts — **met**; every configured provider is one egress row per host, never a URL — **met**; no JSON blob — **met**.

## Design Notes

**Divergences from the plan, flagged rather than silent.** The epic's "optional bot/profile prefix" on the provider row is **not** a provider
column: the bot lives in `bots` and `Endpoint.bot`, and a default-prefix column would be a second place for one fact. AD-147 proposed dotted
keys (`bots.provider.<id>`); the shipped spelling is the house's `<concern>/<id>` slash form, matching `session/<ulid>` — AD-147 should record it.
A dot-segment refusal was dropped because `url` 2.x never lets `.`/`..` survive parsing; the resolution is pinned instead (`a_traversal_segment_is_resolved_by_the_parser_not_stored`).

## Deferred

`resolve_token`'s fallback order (bot key before provider key) is verified only by reading: no test in this slice touches a keychain, because
the `FakePlatform` used by `auth.rs` lives in another module's test scope. A test over it is owed. DW id: to be minted by the coordinator.

## Verification

- `cargo nextest run -p keeper-core -E 'test(bots::url::) or test(bots::store::) or test(bots::tests::) or test(egress::)'` → 51 passed;
  `export_bindings` → 262 passed, five `.ts` files written; clippy clean; `bunx tsc --noEmit` exit 0. Coordinator's tree-wide gate: 4028 Rust / 5466 frontend tests green.

| Mutation | Test that failed | Observed |
|---|---|---|
| `egress.rs`: `remote_host(base_url)` → `Some(base_url.clone())` (the host reduction removed) | `a_provider_is_disclosed_as_a_host_and_never_as_a_url` (+ the other two new egress tests) | 3 failed / 47; "no disclosed destination may carry a credential: https://oauth2:ghp_live@gw.example.org/v1" |
| `url.rs`: the userinfo check → `if false` | `a_base_url_carrying_userinfo_is_refused_by_name` | 1 failed / 49, exactly the intended test |
| `mod.rs` `Endpoint::url`: `"/p/"` → `"/"` | `a_hermes_bot_is_addressed_by_the_profile_prefix` (+ `joining_never_doubles_or_drops_a_separator`) | 2 failed / 48 |

Each restored by copy and proved byte-identical by `diff`. **Not verified here:** the `keeper` shell crate does not build on this Linux host,
so the `egress_list` hunk has never met a compiler — the single highest-value thing for the macOS gate. No network call was made: `Endpoint::url`
is proven as a string against the research's route table, not a live server (61.3 makes it falsifiable). `docs/egress.md` is 61.13's; the release-diff note will fire on this change, which is the visibility working.

## Coordinator addendum, 2026-09-02

The owed keychain test is filed as **DW-216**, with the symptom a wrong resolution order would
produce: a working provider whose every named bot answers 401.
