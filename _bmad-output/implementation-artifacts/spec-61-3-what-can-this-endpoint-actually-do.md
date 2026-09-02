---
title: 'Story 61.3: what can this endpoint actually do'
type: 'feature'
created: '2026-09-02'
status: 'review'
baseline_revision: '9e06c2b'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-375, FR-376, FR-377 — AD-150, AD-151; AD-27 consumed (no affordance that lies)
depends on: 61.1 (`Endpoint::url`, `BotHealthState`) and 61.2 (`bots::http::client`, `BotsError`, the sensitive header)

<intent-contract>

## Intent

**Problem:** Two of the owner's asks — which models, which bots — are the same question, *what can this endpoint do?* Both back ends
answer it differently, and one half has no bearer-authenticated answer at all: the Hermes api-server has no profile roster.
**Discovery answers three questions and refuses a fourth with its reason.**

**Approach:** `keeper-core::bots::discover` — health, models with capabilities, and a named-bot probe, per kind, each a function whose
verdict the shell never re-derives. Ollama reads `/api/tags` alone, because it carries `capabilities`; Hermes merges `/v1/models` with
`/api/model/options`. A capability keeper could not read is `None`, never `Some(false)`. The roster refusal is one constant,
`NO_BOT_ROSTER`, sent with zero requests, so the pane, the empty state and the docs quote the same words.

## Boundaries & Constraints

**Always** (each invariant with the tests that pin it):
- **Unknown is never false.** `capability_flags(Option<&[String]>)` is the only Ollama-side decision: `None` → all three `None`;
  `Some([])` → also `None` (both `/api/tags` and `/api/show` emit `capabilities` with `omitempty`, so an empty array is the same
  silence); a non-empty list makes all three `Some` — a stated absence **is** `false`, and that arm matters as much as the unknown one.
  Hermes is per key: a row stating only `supports_tools` leaves vision and reasoning unknown; a 404 or unparseable
  `/api/model/options` returns the `/v1/models` roster with every flag `None` — `an_absent_capability_list_is_unknown_not_false`,
  `an_empty_capability_list_is_also_unknown`, `a_present_capability_list_answers_all_three`,
  `discover_ollama_without_capabilities_is_unknown_never_false`, `discover_hermes_merges_models_with_model_options`,
  `discover_hermes_models_survive_options_being_unavailable`.
- Ollama discovery is **exactly one round trip**; `ollama_model_capabilities()` (POST `/api/show`) exists for a build old enough to
  serve no capabilities, and `models()` never calls it — `discover_ollama_tags_carry_capabilities_in_one_round_trip` asserts the
  recorded request log is `["GET /api/tags"]` while an `/api/show` route was available to be wrongly called.
- A Hermes bot is probed at `GET /p/{bot}/v1/models`, the URL built by 61.1's `Endpoint::url`, never string arithmetic: `404` → `Absent`;
  `401`/`403` → `Unknown` (a profile can be served under its own key); other status → `Unknown`; nothing on the wire → `Offline` +
  `Unknown`. An Ollama bot is a tag looked up in `/api/tags` — `a_404_is_absence_and_a_401_is_not`,
  `discover_bot_probe_404_means_the_bot_does_not_exist`, `discover_bot_probe_401_is_a_different_answer_from_404`,
  `discover_bot_probe_cannot_tell_when_nothing_answers`, `discover_ollama_bot_is_a_model_tag`.
- `BotRoster` for Hermes is always `Unavailable { reason: NO_BOT_ROSTER }` and sends **no** request —
  `discover_enumerate_bots_lists_ollama_tags_and_refuses_hermes_with_a_reason` (quotes the constant by identity).
- `BotProbeVm` is never an error type; a 401 or 404 is `online` — an answer keeper did not like is still an answer. `health_state`:
  Offline → Unreachable, 401/403 → Unauthorized, 2xx → Reachable, else Unknown (a 502 from a proxy says nothing about the endpoint
  behind it); `SecretMissing` is never produced here — `discover_health_reads_the_version_each_kind_reports`,
  `discover_health_of_a_dead_endpoint_is_offline_and_unreachable`, `discover_health_of_a_refusing_endpoint_is_online_and_unauthorized`.
- No token, host or port in any sentence; no `Response::text`/`json` — `bounded_body()` reads to `MAX_DISCOVERY_BODY` (1 MiB), the
  failing body too, before `BotsError::status` excerpts it — `discover_malformed_json_names_the_endpoint_and_not_the_token`.
- The endpoint's capability strings are kept verbatim in `capabilities`; Hermes' `supports_tools` is **not** rewritten to `false`
  because Hermes executes tools server-side — FR-377 reads the flag from the endpoint, and the client-tools distinction belongs to the
  chat/tool loop (a doc comment on `hermes_models` says so).

**Block If:** nothing. **Never:** enumerate Hermes profiles by any door but the bearer route table; hand-list a host; an N+1 over `/api/show`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Ollama tags | `/api/tags` with `capabilities` | family, parameterSize, quantization, sizeBytes, three definite flags | one request only |
| Hermes merge | `/v1/models` + `/api/model/options` | `/v1/models` order kept; options-only models appended; per-key enrichment | options 404 → roster with unknown flags |
| Nothing answers | bound-then-released port | `reach: offline`, `presence: unknown`; reason names neither token nor host:port | — |
| Malformed JSON | `/api/tags` returns non-JSON | error names `GET /api/tags`, not the credential, not `127.0.0.1` | `BotsError::Parse` |
| Health | Ollama `/api/version`, Hermes `/health` | `version`, measured `roundTripMs` | dead → offline; refusing → online + Unauthorized |
| Roster | Hermes / Ollama | `Unavailable { reason: NO_BOT_ROSTER }`, zero requests / the tag list | — |

</intent-contract>

## Code Map

| file | state | what shipped |
|---|---|---|
| `src-tauri/crates/keeper-core/src/bots/discover.rs` | new, 977 | `health`, `health_state`, `models`, `probe_bot`, `enumerate_bots`, `ollama_model_capabilities`, `discovery_client`, `NO_BOT_ROSTER`, `DISCOVERY_READ_TIMEOUT` (30 s, a ceiling on silence, `keeper-sync/src/http.rs:24-40` named as source), `MAX_DISCOVERY_BODY`, the wire shapes, `BotRoster`; 4 unit tests |
| `src-tauri/crates/keeper-core/tests/bots_discover.rs` | new, 565 | 13 integration tests over a `std::net::TcpListener` on a thread that records every request line it answers |
| `src-tauri/crates/keeper-core/src/vm.rs` | +123 | `BotModelVm` (`vision`/`tools`/`reasoning: Option<bool>`, `contextWindow`, `maxOutputTokens`, `capabilities: Vec<String>`), `BotProbeVm` (`reach`, `status`, `version`, `roundTripMs`, `bot`, `presence`, `reason`), `BotReach` (`online`\|`offline`, `ConnectionStatus`'s vocabulary), `BotPresence` (`exists`\|`absent`\|`unknown`) |
| `src-tauri/crates/keeper-core/src/bots/mod.rs`; `src/lib/ipc/gen/{BotModelVm,BotProbeVm,BotReach,BotPresence}.ts` | +3; generated | `pub mod discover;` plus the `error`/`http` lines 61.2's files needed, claimed over hub; ts-rs export, numbers as `number \| null` |

`NO_BOT_ROSTER`: *"keeper cannot list the bots on a Hermes endpoint. The key you gave it opens the chat API, and that API has no route
that names the profiles behind it — a roster needs a door keeper is not handed. Type a bot's name and keeper will check it exists before
you rely on it."*

## Tasks & Acceptance

**Execution:** [x] health per kind · [x] models per kind with the tri-state · [x] the bot probe over `Endpoint::url` · [x] the roster
refusal · [x] bounded bodies · [x] the four VMs and bindings · [x] the recording server and 13 cases. **Acceptance Criteria:** models,
capabilities and health per kind — **met**; a named profile verified at `/p/<name>/v1/models` — **met**; the roster refusal in the house
voice — **met** (`NO_BOT_ROSTER`); a capability keeper could not read is `unknown`, never `false` — **met**, and mutation (a) proves it load-bearing.

## Design Notes

**Divergences from AD-151, flagged.** The architecture names a `Capability { True, False, Unknown }` enum and a `thinking` flag; the
shipped shape is `Option<bool>` (`boolean | null` on the wire) and the flag is `reasoning`, read from Ollama's `"thinking"` string and
Hermes' `supports_reasoning`. Same tri-state, different spelling — AD-151 should record it. The assignment asked for
`tokio::net::TcpListener`; the test server is `std::net` on a thread because the workspace's tokio has no `net` feature and keeper-core
has no `[dev-dependencies]` — the pattern `keeper-sync/src/http.rs` and `keeper-syncd/tests/update_install.rs` already use. The socket is just as real.

## Verification

- `cargo nextest run -p keeper-core -E 'test(discover)'` → 32 passed (17 this story's, 15 pre-existing bridge-discovery tests the filter
  also matches). `export_bindings_bot…` → 4/4. `cargo check -p keeper-core` clean; clippy and `rustfmt --check` clean on both files.
  Coordinator's tree-wide gate: 4028 Rust tests green, clippy/fmt clean.

| Mutation | Test that failed | Observed |
|---|---|---|
| (a) `capability_flags` early return `(None,None,None,vec![])` → `(Some(false),Some(false),Some(false),vec![])` | `an_absent_capability_list_is_unknown_not_false`, `an_empty_capability_list_is_also_unknown`, `discover_ollama_without_capabilities_is_unknown_never_false` | 29 passed, 3 failed |
| (b) `hermes_presence`: `404 =>` widened to `404 \| 401 \| 403 =>` — a refused key reported as Absent | `a_404_is_absence_and_a_401_is_not`, `discover_bot_probe_401_is_a_different_answer_from_404` | 30 passed, 2 failed |
| (c) `ollama_models` rewritten to call `ollama_model_capabilities` per row — the N+1 forced | `discover_ollama_tags_carry_capabilities_in_one_round_trip` (log gained `POST /api/show`), `discover_ollama_without_capabilities_is_unknown_never_false`, `discover_enumerate_bots_lists_ollama_tags_and_refuses_hermes_with_a_reason` | 29 passed, 3 failed |

Restored from a pre-mutation copy, proved byte-identical by `diff`, 32 passed. **Not verified here:** no real Ollama or Hermes was
contacted; every payload is a fixture transcribed from the research digests. Two shapes are inferred, not observed: the nesting of
`/api/model/options`' per-model capability keys (read both nested under `capabilities` and flat, preferring nested) and whether Hermes'
`/health` carries `version` at the top level. Either being different degrades to unknown / `null` rather than lying, but one capture
against a live gateway would settle both. No `#[tauri::command]` here — `bots_models_list`, `bots_bot_probe` and `bots_provider_probe` are 61.4's.
