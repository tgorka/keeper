---
title: 'Story 61.2: one wire, and the divergences are data'
type: 'feature'
created: '2026-09-02'
status: 'review'
baseline_revision: '9e06c2b'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-372, FR-373, FR-374, NFR-46 — AD-149; AD-40 consumed (`keeper-core` may not import `keeper-sync`)
depends on: 61.1 (a provider record and `Endpoint::url` are the inputs; second of the strictly serial pair)

<intent-contract>

## Intent

**Problem:** Both back ends honour `POST /v1/chat/completions` with `stream: true`, and the temptation is a client per kind. Every
epic that failed review failed because the test sat on a pure function while the risk sat in the impure shell — here, a real socket
that dies mid-frame. **The client is one framer and one state machine, and both are tested against a socket that misbehaves.**

**Approach:** `keeper-core::bots::{http, sse, chat, quirks, error}`: one `reqwest::Client` bounded by silence, an SSE framer over
`&[u8]` decoding UTF-8 per complete frame, a `Reassembler` keying `tool_calls` fragments by `index`, a cancel handle, and a per-kind
quirk table whose every row carries its citation, all driven by 14 tests against an in-process HTTP/1.1 server that counts connections.

## Boundaries & Constraints

**Always** (each invariant with the tests that pin it):
- `ClientBuilder::read_timeout`, never `ClientBuilder::timeout`; the policy is restated in `http.rs` with a comment naming
  `keeper-sync/src/http.rs:24-40` as its source, not imported (AD-40) — `a_server_that_sends_headers_then_nothing_fails_on_silence`,
  `a_slow_but_alive_stream_outlives_the_silence_budget`, `the_read_timeout_default_clears_every_documented_keepalive`.
- `stream_chat` returns `Err` only for a failure **before any stream byte**. After bytes, a failure returns `Ok(outcome)` with the partial
  content intact, `finish_reason` `Failed` or `Cancelled`, and the typed cause as `ChatEvent::Failed` — a partial reply is a record, and
  that line is also the retry boundary — `a_stream_cut_mid_message_keeps_the_partial_and_reports_the_failure`,
  `a_cancelled_stream_keeps_what_had_already_arrived`, `a_partially_streamed_request_is_never_retried`,
  `a_request_that_produced_no_bytes_is_retried_once`, `only_429_and_5xx_are_retryable_statuses`, `backoff_doubles_and_saturates`.
- A credential appears in no error: `AUTHORIZATION` built by `http::authorization()` with `set_sensitive(true)`; `BotsError::transport`
  takes `reqwest::Error` by value and calls `without_url()` first; errors name `POST /v1/chat/completions`, never a URL; excerpts capped
  at `MAX_ERROR_DETAIL_BYTES` = 512 on a char boundary — `unauthorized_is_a_typed_error_carrying_no_credential`,
  `a_401_display_names_neither_the_token_nor_the_header`, `a_credential_with_a_newline_is_refused_without_echoing_it`,
  `the_authorization_value_is_marked_sensitive`, `excerpt_bounds_a_long_body_on_a_char_boundary`.
- Chunk boundaries are nothing; an unterminated frame is capped at `MAX_FRAME_BYTES` (1 MiB) — `a_frame_split_across_two_writes_is_reassembled`,
  the `sse` split/truncated quartet, `a_frame_that_never_ends_is_capped_rather_than_buffered`.
- Tool calls are keyed by `delta.tool_calls[].index`, never array position — `tool_calls_are_keyed_by_index_not_by_array_position`,
  `out_of_order_and_gapped_indices_keep_first_seen_order`, `a_tool_call_without_an_id_gets_a_stable_synthesised_one`,
  `an_unparseable_arguments_string_is_kept_raw_not_dropped`, `two_interleaved_tool_calls_reassemble_with_valid_arguments`.
- A quirk is a row in `quirks(kind)`, never a code path; `Support::Unknown` is **permitted**; the body always streams and always asks
  for usage — `ollama_drops_tool_choice_and_hermes_keeps_it`, `a_remote_image_url_is_refused_for_the_kind_that_will_not_fetch_one`,
  `an_image_takes_the_shape_its_provider_documents`, `unknown_is_permitted_and_no_is_not`, `the_body_always_streams_and_always_asks_for_usage`.
- `usage` absent is absent (every `TokenUsage` field `Option<u32>`); reasoning is its own channel; an in-band error frame fails the turn —
  `done_with_no_usage_frame_is_a_reply_with_no_numbers`, `a_null_usage_never_overwrites_a_real_one`,
  `reasoning_is_its_own_channel_and_never_joins_the_answer`, `an_in_band_error_frame_fails_the_turn_rather_than_completing_it`.

**Block If:** nothing; the source of every timeout number is cited at the constant. **Never:** a native `/api/chat` client (DW-210);
`Response::text` on an unbounded body; a silently dropped image or `tool_choice`; a new dependency (`bytes`, reqwest `stream` avoided).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Split frame / scalar | a frame, a `\r\n\r\n`, or a multi-byte scalar across two writes | reassembled | pending tail held, never cleared |
| Headers then silence | socket held open sending nothing | `Timeout{after_ms}` after `READ_TIMEOUT` | retryable — no bytes streamed |
| Cut mid-message / cancel | closed after content / `CancelHandle` fired | `Ok`, partial kept, `Failed` / `Cancelled` | **never retried** |
| No bytes then failure | connect failure / 5xx before a byte | retried once; backoff doubles, saturates | server sees 2 connections |
| 401 / 404 / 500 | status before a byte | distinct `Status{status, endpoint, detail}` | no token, no `authorization`, no `http://` in Display |

</intent-contract>

## Code Map

| file | lines | what shipped |
|---|---|---|
| `src-tauri/crates/keeper-core/src/bots/error.rs` | 272 | `BotsError`: ClientBuild, Transport{detail,retryable}, Timeout{after_ms}, Status, Parse, Protocol, Provider{code,message}, Unsupported, Credential, BaseUrl(#[from]); `transport()`, `status()`, `is_retryable()` (Transport{retryable}, Timeout, 429, 5xx) |
| `src-tauri/crates/keeper-core/src/bots/http.rs` | 178 | `client(read_timeout)`; `CONNECT_TIMEOUT` 10 s, `READ_TIMEOUT` 120 s, `POOL_IDLE_TIMEOUT` 20 s, `redirect(Policy::none())`; `authorization`, `authorize` |
| `src-tauri/crates/keeper-core/src/bots/sse.rs` | 326 | `SseFramer` — `push`, `next_frame`, `pending_bytes`; three blank-line spellings, optional space after `data:`, multi-line data, `:` keep-alives, `event:`/`id:`/`retry:` ignored, `[DONE]`, `MAX_FRAME_BYTES` |
| `src-tauri/crates/keeper-core/src/bots/quirks.rs` | 265 | `Support{Yes,No,Unknown}` + `permitted()`, `ImagePartShape{Object,BareString}`, `Quirks`, `const fn quirks(kind)` |
| `src-tauri/crates/keeper-core/src/bots/chat.rs` | 1567 | request model, `build_body`, `honours_tool_choice`, `Reassembler`, `cancellation()`, `ChatOptions`, `stream_chat`, `ChatEvent`, `FinishReason` |
| `src-tauri/crates/keeper-core/tests/bots_wire.rs`; `src-tauri/crates/keeper-core/src/bots/mod.rs` | 673; +8/-13 | 14 integration tests over `tokio::net::TcpListener`; the deduplicated `pub mod` block shared with 61.1/61.3 |

`ChatEvent`: `FirstToken{after_ms}` (exactly once) · `ContentDelta` · `ReasoningDelta` · `ToolCallDelta{index,id,name,arguments}` ·
`Usage(TokenUsage)` · `Finished{reason}` · `Failed{error}`. `FinishReason`: Stop | Length | ContentFilter | ToolCalls | Cancelled | Failed
(default) | Other(String). **Quirks** (hermes / ollama): `tool_choice` Unknown / **No** (dropped from the body; `honours_tool_choice` lets the
surface avoid a lying affordance) · `remote_image_url` Yes / **No** · `image_part` Object / BareString · `context_window_over_v1` No / No
(`num_ctx` is DW-210) · `done_sentinel` Yes / Yes · `stream_usage` Unknown / Yes · `reasoning_fields` [reasoning_content, reasoning] / [reasoning] · `keepalive_secs` 30 / none · `named_events` Yes / No.

## Tasks & Acceptance

**Execution:** [x] error taxonomy · [x] client and sensitive header · [x] framer · [x] quirk table · [x] request model, reassembler,
cancel, driver · [x] the misbehaving server and its 14 cases. **Acceptance Criteria:** a local server that sends a valid stream, a
frame in two chunks, dies mid-message, sends `[DONE]` with no usage, returns 401/404/500, and holds the socket open sending nothing —
**every case met by name** in `bots_wire.rs`; cancellation leaves the partial reply as a record — **met**; silence, not deadline — **met**.

## Design Notes

**Two choices a reviewer should see.** `BotsError::Timeout` is retryable, deliberately: without it no post-bytes failure is retryable
and the no-bytes guard would be decorative — mutation (d) would have been unkillable. `redirect(Policy::none())` is chosen on OWASP
grounds (a 302 would carry a validated base URL and its bearer token to a host the user never saw); a proxy that 301s http→https surfaces
as `Status{301}` — honest, untested against a real deployment, a likely support question. **Divergence:** AD-149 names a 15 s connect timeout; shipped `CONNECT_TIMEOUT` is 10 s.

## Deferred

The native `/api/chat` dialect, with `num_ctx` on Ollama and `think` levels — DW-210. The `context_window_over_v1` row is the price.

## Verification

- `cargo nextest run -p keeper-core -E 'binary(bots_wire) or test(bots::chat) or test(bots::sse) or test(bots::http) or test(bots::quirks)
  or test(bots::error)'` → 54 passed. `cargo check --all-targets`, clippy and per-file `rustfmt` clean. Coordinator's tree-wide gate: 4028 Rust tests green.

| Mutation | Test that failed | Observed |
|---|---|---|
| (a) `sse.rs` `next_frame`: `self.buffer.clear()` before `Ok(None)` — the tail discarded | the four `sse` split/truncated tests + `bots_wire::a_frame_split_across_two_writes_is_reassembled` | 5 failed / 20 |
| (b) `chat.rs` `apply_delta`: key by array position instead of `delta.tool_calls[].index` | `tool_calls_are_keyed_by_index_not_by_array_position`, `out_of_order_and_gapped_indices_keep_first_seen_order`, `a_tool_call_without_an_id_gets_a_stable_synthesised_one`, `bots_wire::two_interleaved_tool_calls_reassemble_with_valid_arguments` | 4 failed / 26 |
| (c) `http.rs`: `.timeout(read_timeout)` in place of `.read_timeout(…)` | `bots_wire::a_slow_but_alive_stream_outlives_the_silence_budget` **only** — the silence test still passes, which is why the slow-but-alive fixture exists | 1 failed / 17 |
| (d) `stream_chat`: `!failure.saw_stream_bytes` dropped from the retry guard | `bots_wire::a_partially_streamed_request_is_never_retried` | server saw 3 connections instead of 1 |

Each restored and re-run green. **Not verified here:** no real Hermes or Ollama — every wire assertion is against the in-process server
speaking the shapes the research quotes, so the `Unknown` quirk rows stay Unknown until a live probe; the Ollama bare-string `image_url`
shape is the documented one, not read from `server/routes.go`; HTTP/2 chunking is argued, not measured. Nothing here touches the shell crate.
