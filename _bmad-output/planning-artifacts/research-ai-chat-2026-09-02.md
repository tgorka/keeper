---
name: 'keeper'
type: research
topic: 'talk to models — Hermes Agent and Ollama over the OpenAI-compatible API, inside keeper'
decision: 'what a chat-with-a-model surface in keeper (bots, drive tool-calls with per-scope permissions, voice, images, sessions, slash commands) can and cannot be built from, given the two back ends as they ship today and the app as it is on main'
status: final
created: '2026-09-02'
run_folder: _bmad-output/planning-artifacts/research/ai-chat-2026-09-02
run_folder_note: 'not created by this pass — the twelve digests below were read from /tmp/aiplan/ and are not yet filed under research/; filing them is the caller’s step'
digests:
  - /tmp/aiplan/R1.md — Hermes Agent integration surface (route table read from source, v0.21.0 / v2026.8.31)
  - /tmp/aiplan/R2.md — Ollama native + OpenAI-compat API, discovery, remote, errors (main @ e5e4377, v0.33.2)
  - /tmp/aiplan/R3.md — OpenAI-compatible streaming + tool calling in Rust, reassembly, cancellation, retries
  - /tmp/aiplan/R4.md — wake word / STT / TTS engines, code and model licences, Rust bindings, sidecar cost
  - /tmp/aiplan/R5.md — chat UX prior art: slash commands, metadata, pins, session archives, image paste, a11y
  - /tmp/aiplan/R6.md — filesystem tool-calls with permissions, agent context files, prompt injection, TCC
context:
  - /tmp/aiplan/G1.md — BMAD conventions, numbering ceilings, artifact anatomy
  - /tmp/aiplan/G2.md — how a pane / IPC command / stream / table is added to keeper (path:line)
  - /tmp/aiplan/G3.md — the drive as a filesystem, every containment guard, egress, crate graph (path:line)
  - /tmp/aiplan/G4.md — multi-profile config, credentials, HTTP clients, deps, deny.toml (path:line)
  - /tmp/aiplan/G5.md — audio, mic permission, clipboard, images, windows, the zero-egress gates (path:line)
  - /tmp/aiplan/G6.md — existing chat / timeline / sessions / palette / pin prior art (path:line)
---

# Research — Talk to models: Hermes Agent and Ollama, inside keeper

**Evidence grades.** `[SOURCE]` = external primary source, read on 2026-09-02, cited with publisher and URL
(and the digest section it came from). `[REPO]` = read out of this worktree at `origin/main` tip `9e06c2b`,
cited `path:line` as the scout recorded it. `[INFERENCE]` = reasoning over cited facts, no source of its own.
`[UNVERIFIED]` = looked for, not found — never to be repeated as fact. §16 is the complete inventory of the
latter and says what was tried for each.

**How to cite this document.** Sections are numbered `§N.M` and are stable. Cite as
`research-ai-chat-2026-09-02.md §4.3`. Rust doc-comments in this tree already cite research this way
(`keeper-sync/src/lfs/basic.rs:50` cites the sync research), and the virtual-files research
(`research-virtual-files-2026-08-22.md`) set the convention this document follows.

**What this document is not.** It does not design the feature. Where the evidence forces a conclusion it
is stated as a constraint with its evidence; every "must", "cannot" and "will trip" below points at the line
or the URL that makes it so.

---

## 1. Scope, method, what each digest covered, and the confidence grading

### 1.1 The question

The owner's ask, as handed to the scouts: a surface in keeper for talking to models — bots that can be
pinned and given an identity, a chat that streams, tool-calls onto the synced drive with per-scope
permissions, a voice/talk mode with a wake word, an image mode with clipboard paste, sessions that can be
archived and resumed, slash commands, per-message metadata — against two back ends: **Hermes Agent**
(NousResearch) and **Ollama**, both reached over HTTP, ideally through the OpenAI-compatible wire.

### 1.2 Method

Twelve read-only digests, six over the repository and six over the outside world, all produced on
2026-09-02. Repository facts were read at `origin/main` tip `9e06c2b` `[REPO]` G2 §0, G3 header, G4
header, G5 header. External facts were read from primary sources at named refs: Hermes Agent
`hermes-agent` 0.21.0, tag `v2026.8.31`, main @ `1802911` `[SOURCE]` R1 header
(<https://api.github.com/repos/NousResearch/hermes-agent/releases/latest>); Ollama v0.33.2, main @
`e5e4377` `[SOURCE]` R2 §1 (<https://api.github.com/repos/ollama/ollama/releases/latest>); crates.io
metadata via `https://crates.io/api/v1/crates/<name>` `[SOURCE]` R2 §13, R3 §5.1, R4 passim.

| Digest | Slice | Grade |
| --- | --- | --- |
| G1 | `_bmad/config.toml`, numbering ceilings (next free: Epic 60, FR-94, NFR-27, AD-54, DW-201), epic/spec/sprint-status anatomy, `docs/decisions.md`, the architecture spine | `[REPO]` |
| G2 | frontend registry, keyboard chords, pane conventions, IPC commands and `Channel<T>` streams, ts-rs codegen, `dev/mock-shell.ts`, `CapabilitiesVm`, settings, SQLite migrations, palette, adjacent absences | `[REPO]` |
| G3 | vocabulary (profile / folder / vault / space / note / session), every containment guard, file IPC, virtual files, permission prior art, `AGENTS.md`, concurrency, `docs/egress.md`, crate graph | `[REPO]` |
| G4 | `SyncProfile` as the multi-tenant precedent, keychain, HTTP clients and the timeout doctrine, streaming and cancellation idioms, backoff, error taxonomy, `Cargo.toml`, `deny.toml`, `package.json`, state vocabulary | `[REPO]` |
| G5 | `keeper-rec` sidecar, mic TCC, absence of STT/TTS, the recording zero-egress gates, playback, clipboard, images and the four custom protocols, secondary windows, tray, notifications | `[REPO]` |
| G6 | the Matrix chat surface component by component, per-message metadata, sessions (Phase 7), recordings archive, palette, note-editor slash menu, pins, identity, search, copy voice | `[REPO]` |
| R1 | Hermes Agent: licence, deployment shapes, the complete `_http_route_table()`, auth schemes, bots, sessions, slash commands, deliverable mode, voice, wake word, tools/MCP, limits, error envelope, versioning | `[SOURCE]` |
| R2 | Ollama: native and OpenAI-compat surfaces, `capabilities`, context length, thinking, vision, remote/auth/CORS, cloud, errors, first-shipped versions, licences, Rust crates | `[SOURCE]` |
| R3 | the Chat Completions SSE contract, the reassembly state machine and its traps, tool-call round trip, vision shapes across servers, budget strategies, Rust crates, cancellation, retries, persistence schemas, testing, SSRF | `[SOURCE]` |
| R4 | wake word / VAD / STT / TTS engines with code **and** model licences, `ort`, `cpal`, sidecar and codesign cost, barge-in | `[SOURCE]` |
| R5 | slash-command mechanisms (Telegram, Claude Code, opencode/Gemini/Codex, Open WebUI), metadata, pins, session schemas, streaming UI, provider config UX, image paste, a11y | `[SOURCE]` |
| R6 | MCP `roots` and the reference filesystem server, Claude Code / Codex / Gemini permission models, context files, path-safety CVEs, prompt injection, macOS TCC, audit and rollback | `[SOURCE]` |

### 1.3 Two orientation facts that shape everything below

1. **No LLM code exists in keeper.** A grep for `ollama|hermes|nousresearch|openai|anthropic|/v1/chat/completions|text/event-stream` over the worktree hits only prose (`DESIGN.md:301`, `:464`, the design research, the virtual-files research) `[REPO]` G2 §11.1; over `src-tauri; src` it matches only CodeMirror `CompletionSource` names `[REPO]` G4 §11. Provider, transport, discovery and tool-call plumbing are all new.
2. **The epic number 60 is already promised to something else.** `_bmad-output/planning-artifacts/epic-59-a-task-you-can-find-and-a-run-you-can-read.md:6` says *"Epic 60 (`epic-60-*`, not yet written) owns the general exec kind this epic deliberately does not build"*, and D-3 in `docs/decisions.md` records *"the general exec kind is Epic 60, threat model first"* `[REPO]` G2 §0, G1 §6. No `epic-60-*.md` exists `[REPO]` G2 §0. `[INFERENCE]` an epic numbered 60 about talking to models either absorbs that threat-model obligation or the promise in epic-59 must be re-pointed; it cannot be left as two epics with one number.

---

## 2. Hermes Agent

All of §2 is `[SOURCE]` R1 unless marked. Version pin: `hermes-agent` 0.21.0, git tag `v2026.8.31`, published
2026-08-31; source read from `https://raw.githubusercontent.com/NousResearch/hermes-agent/main/<path>`;
docs at <https://hermes-agent.nousresearch.com/docs/> (machine-readable twins at `/docs/llms.txt` and
`/docs/llms-full.txt`, *"Generated fresh on every deploy"*).

### 2.1 What it is, and its licence

- `pyproject.toml`: `name = "hermes-agent"`, `version = "0.21.0"`, `requires-python = ">=3.11,<3.14"`, `license = "MIT"`. `LICENSE` opens `MIT License / Copyright (c) 2025 Nous Research`. **SPDX `MIT`, permissive.** An HTTP-only client links nothing of Hermes, so the licence firewall (§8.5) is not engaged by consuming the API `[SOURCE]` R1 §1.1.
- Architecture (`website/docs/developer-guide/architecture.md`): one `AIAgent` (`run_agent.py`) serves CLI, gateway, ACP, batch and API server; *"Platform differences live in the entry point, not the agent."* The three "API modes" (`chat_completions`, `codex_responses`, `anthropic_messages`) are **outbound** toward LLM providers, not the inbound surface `[SOURCE]` R1 §1.2.

### 2.2 Deployment shapes, ports, config locations

Run modes: interactive CLI (`hermes` / `hermes chat`, prompt_toolkit); Ink TUI (`hermes --tui`, Node UI over
`tui_gateway` JSON-RPC); **messaging gateway (`hermes gateway`, long-running, hosts the `api_server` platform)**;
dashboard backend (`hermes dashboard`, also called `hermes serve` — §16; FastAPI + uvicorn on `127.0.0.1:9119`);
Hermes Desktop (Electron, attaches to a local or remote backend); ACP (`hermes acp`, stdio JSON-RPC for IDEs).
Defaults: API server `127.0.0.1:8642` (`DEFAULT_HOST`/`DEFAULT_PORT` in `api_server.py`); config
`~/.hermes/config.yaml`; secrets `~/.hermes/.env`; OAuth `~/.hermes/auth.json`; state `~/.hermes/state.db`
(SQLite + FTS5); persona `~/.hermes/SOUL.md`; gateway platforms `~/.hermes/gateway-config.yaml`; per-profile
home `~/.hermes/profiles/<name>/` with its own `.env`, `config.yaml`, `state.db`. Precedence: CLI args →
`config.yaml` → `.env` → built-ins; for the API-server block *"Environment variables take precedence over
`config.yaml` values"* `[SOURCE]` R1 §1.3–1.4.

### 2.3 The HTTP surface — the complete route table

Implementation: `gateway/platforms/api_server.py` (8032 lines, aiohttp — not the FastAPI dashboard).
`_http_route_table()` (L2161–2222) is authoritative. Enable with `API_SERVER_ENABLED=true` and
`API_SERVER_KEY=…` in `~/.hermes/.env`; `hermes gateway` then logs `[API Server] API server listening on
http://127.0.0.1:8642` `[SOURCE]` R1 §2.1.

**Every route is registered twice — once bare and once at `/p/{profile}<path>`** (`connect()` L7865–7867)
`[SOURCE]` R1 §2.2.

| Method | Path | Auth |
| --- | --- | --- |
| GET | `/health`, `/v1/health` | public — `/health` carries a non-empty `version` |
| GET | `/health/detailed` | bearer; **HTTP 200 even when degraded**, read `status` and `readiness.checks` |
| GET | `/v1/models` | bearer |
| GET | `/api/model/options` | bearer |
| GET | `/v1/capabilities` | bearer |
| GET | `/v1/skills`, `/v1/toolsets` | bearer, read-only |
| GET/POST | `/api/sessions` | bearer |
| GET/PATCH/DELETE | `/api/sessions/{id}` | bearer |
| GET | `/api/sessions/{id}/messages` | bearer |
| POST | `/api/sessions/{id}/fork`, `/chat`, `/chat/stream`, `/model` | bearer |
| POST | `/v1/chat/completions` | bearer |
| POST/GET/DELETE | `/v1/responses`, `/v1/responses/{id}` | bearer |
| POST/GET | `/v1/runs`, `/v1/runs/{id}`, `/v1/runs/{id}/events` | bearer **or** room grant |
| POST | `/v1/runs/{id}/approval`, `/steer`, `/stop` | same |
| GET/POST/PATCH/DELETE | `/api/jobs`, `/api/jobs/{id}` (+ `/pause`, `/resume`, `/run`) | bearer |
| POST/GET | `/v1/browser-control/register`, `/ws`; `/v1/artifacts/upload`, `/download/{id}` | bearer + feature flag (+ ticket / rate limit) |
| POST | `/api/platforms/{platform}/events` | **the target adapter's own verifier, not `API_SERVER_KEY`** |
| POST/GET | `/v1/room-members/invitations`, `/capabilities`, `/grants/refresh`, `/grants/revoke` | `Authorization: HermesRoom <token>` |
| POST | `/api/cron/fire` | NAS-minted JWT, not `API_SERVER_KEY` |

**There is no profile/bot roster on this listener** — proved by the exhaustive route table `[SOURCE]` R1
§2.2, §4.2, §11 item 14.

Hard limits in the adapter: `MAX_REQUEST_BYTES = 10_000_000`; `MAX_STORED_RESPONSES = 100`;
`CHAT_COMPLETIONS_SSE_KEEPALIVE_SECONDS = 30.0`; `MAX_NORMALIZED_TEXT_LENGTH = 65_536` per content part;
`MAX_CONTENT_LIST_SIZE = 1_000`; `_MAX_SESSION_HEADER_LEN = 256`; `_SESSION_SOURCE = "api_server"`
`[SOURCE]` R1 §2.3.

### 2.4 `/v1/chat/completions` — what is honoured and what is ignored

| Field | Behaviour `[SOURCE]` R1 §2.4 |
| --- | --- |
| `messages` | required; missing → 400 naming `messages` |
| `model` | **ignored when sent without `provider`** unless `gateway.platforms.api_server.direct_model_requests: true` |
| `provider` | a Hermes provider slug; when present the model is always honoured; conflicting with a `model_routes` alias → 400 |
| `model_options` | request-scoped `{"reasoning_effort","service_tier"}` |
| `stream` | `true` → SSE `chat.completion.chunk` frames + `data: [DONE]`, keepalive every 30 s |
| system message | **layered on top** of Hermes' core prompt — *"Your agent keeps all its tools, memory, and skills — the frontend's system prompt adds extra instructions."* |
| `file` / `input_file` / `file_id` / non-image `data:` | 400 `unsupported_content_type` |
| state | stateless per request — the full conversation rides in `messages`; continuity is by header (§2.7) |

Model precedence: session `/model` override → `model_routes` alias → request `model`+`provider` → global
defaults. The response is standard OpenAI (`id: "chatcmpl-…"`, `object: "chat.completion"`,
`choices[0].message`, `finish_reason`, `usage`) `[SOURCE]` R1 §2.4.

Vision content parts, exact shapes: Chat Completions `{"type":"image_url","image_url":{"url":…,"detail":"high"}}`;
Responses `{"type":"input_image","image_url":"data:image/png;base64,…"}`. Remote `http(s)` **and** `data:`
are accepted on both, and on `/api/sessions/{id}/chat[/stream]` `[SOURCE]` R1 §2.5.

### 2.5 Streaming frames

- Chat Completions: `chat.completion.chunk` frames **plus a named event `event: hermes.tool.progress`** with keys `tool`, `label`, `toolCallId`, `status`; lifecycle is a strict pair `running` → `completed` per call id; progress never appears in `delta.content`; terminator `data: [DONE]` `[SOURCE]` R1 §2.6.
- Responses: `response.created`, `response.output_text.delta`, `response.output_item.added/done`, `response.completed`, with live `function_call` / `function_call_output` items.
- Session chat stream: `assistant.delta`, `tool.started`, `tool.completed`, `run.completed`.
- Runs stream: lifecycle plus `subagent.start` / `subagent.complete` (status, summary, duration, tokens, `child_session_id`); per-tool child events are deliberately not forwarded.

### 2.6 Tool calling — the decisive asymmetry

**Hermes never hands the client a tool call to execute.** Quote: *"Tool calls in the `output` array were
already executed server-side by the Hermes agent — they are replayed with `"status": "completed"` for
structured tool UI, never as pending calls for the client to execute."* `/v1/capabilities` states it
structurally: `runtime.mode == "server_agent"`, `runtime.tool_execution == "server"`,
`runtime.split_runtime == false`. A client **cannot register tools over HTTP** — no route exists
`[SOURCE]` R1 §2.7, §8.1.

**Constraint.** A keeper drive-tool (`read_file` / `write_file` over a sync profile, §6) cannot be executed
by keeper on Hermes' behalf through `/v1/chat/completions`, because the wire never carries a pending call.
The drive-tool half of the ask is therefore possible only (a) against Ollama's `tools` (§3.6), or (b) by
exposing keeper to Hermes as an MCP server that Hermes calls (§2.13), each with its own consequences. This
is not a design choice; it is what the two protocols carry `[INFERENCE]` from R1 §2.7 and R2 §2.3.

### 2.7 Hermes-specific headers

| Header | Meaning `[SOURCE]` R1 §2.8 |
| --- | --- |
| `Authorization: Bearer <API_SERVER_KEY>` | primary auth |
| `Authorization: HermesRoom <token>` | RoomLink room-member grant (scheme `hermesroom`, case-insensitive) |
| `X-Hermes-Session-Id` | transcript-scoped id; advertised as `features.session_continuity_header` |
| `X-Hermes-Session-Key` | stable long-term-memory scope independent of session id; ≤256 chars, `\r`/`\n`/`\x00` rejected; echoed on JSON and SSE; recovery is reset-fenced (rows ended at `session_reset`, `session_switch`, `idle`, `daily`, `suspended` are excluded) |
| `Idempotency-Key` (`POST /v1/runs`) | 1–255 visible ASCII; replay → original `run_id`, 202, `Idempotency-Replayed: true`; same key + different payload → 409 `idempotency_key_conflict`; 24 h retention per credential |
| `Retry-After` | on the 429 concurrency rejection |

### 2.8 Model discovery

`GET /v1/models` → `{"object":"list","data":[{"id":"hermes-agent","owned_by":"hermes","root":"hermes-agent"}]}`;
under a named profile `id == "<profile>"`; `API_SERVER_MODEL_NAME` overrides. The source docstring says it
also lists *"any configured `model_routes` aliases"*, which the prose docs omit — so a deployment with
`model_routes` returns more than one entry. Documented limitation: it *"does **not** enumerate every
authenticated provider/model combination Hermes can route to"* `[SOURCE]` R1 §2.9.

`GET /api/model/options` (bearer on the API server; `X-Hermes-Session-Token` on the dashboard; RPC
`model.options`) returns `build_model_options_payload(ctx, include_unconfigured=True, refresh=…)` verbatim:
`{"providers":[{"slug","name","models":[…]}],"model","provider"}`; `?refresh=true` busts the cache and
probes all saved custom providers. Per-model capability keys: `supports_tools`, `supports_vision`,
`supports_reasoning`, `context_window`, `max_output_tokens`, `model_family` `[SOURCE]` R1 §4.3.

### 2.9 `/v1/capabilities` — the version-negotiation surface

Verified subset (documented skeleton plus every key asserted in `tests/gateway/test_api_server.py` and
`test_session_api.py`; the handler itself was not read, §16): `object: "hermes.api_server.capabilities"`,
`auth.{type:"bearer",required}`, `runtime.{mode,tool_execution,split_runtime}`, `features.{chat_completions,
responses_api, run_submission, run_status, run_events_sse, run_stop, run_steer, run_approval,
session_resources, session_chat, session_chat_streaming, session_fork, skills_api, model_options,
admin_config_rw:false, memory_write_api:false, realtime_voice:false,
runs_idempotency:{supported,durable,retention_seconds}, session_continuity_header, session_key_header}`,
`endpoints.*` path templates, `browser_extension_control.*`. `runs_idempotency.durable` is a **runtime**
value (false on the in-memory store) `[SOURCE]` R1 §2.10. The intended mechanism is capability probing, not
version comparison; Hermes' own peer client does exactly this (`hermes_cli/subcommands/peer.py`,
`_peer_run_durability()`) `[SOURCE]` R1 §10.3.

### 2.10 The native (non-OpenAI) protocols

Three protocols drive the same `AIAgent` (`programmatic-integration.md`): ACP (JSON-RPC/stdio), the **TUI
gateway** (JSON-RPC over stdio **or WebSocket**, `tui_gateway/ws.py`), and the API server (HTTP+SSE). The
doc's own recommendation for a custom desktop host wanting *"every Hermes feature (slash commands,
approvals, clarify, multi-agent, session branching)"* is the TUI gateway `[SOURCE]` R1 §3.

- Wire: newline-delimited JSON-RPC both ways; `gateway.ready` emitted on connect; the dashboard's `/api/ws` **is** the TUI gateway. Per-token frames (`message.delta`, `reasoning.delta`, `thinking.delta`) are coalesced on a 33 ms timer; `_WS_WRITE_TIMEOUT_S = 10.0` and a write timeout means the loop stalled, not that the socket died `[SOURCE]` R1 §3.1.
- Methods (verbatim list): `prompt.submit`, `prompt.background`, `session.steer`, `session.{create,list,active_list,activate,close,interrupt,history,compress,branch,title,usage,status}`, `clarify.respond`, `sudo.respond`, `secret.respond`, `approval.respond`, `config.set/get`, `commands.catalog`, `command.resolve`, `command.dispatch`, `cli.exec`, `reload.mcp`, `reload.env`, `process.stop`, `delegation.status`, `subagent.{interrupt,steer}`, `spawn_tree.{save,list,load}`, `terminal.resize`, `clipboard.paste`, `image.attach`; plus, found in source, `profiles.list`, `model.options`, `system.battery`, `process.{list,kill}`, `wake.start`, `wake.feed`, `complete.slash`, `slash.exec`, `image.generate` `[SOURCE]` R1 §3.2.
- Events: `message.delta/complete`, `tool.start/progress/complete`, `approval.request`, `clarify.request`, `sudo.request/expire`, `secret.request/expire`, `gateway.ready`, session lifecycle and errors. *"Expiry events carry the original `{ request_id }`"* `[SOURCE]` R1 §3.3.
- Destructive rewind: `prompt.submit` accepts `truncate_before_user_ordinal`, `truncate_before_row_id`, `confirm_truncate`, `confirm_empty_truncate`; codes 4004, 4018, 4029, 4030; success returns `survivor_user_row_ids` and every cached row id is stale afterwards `[SOURCE]` R1 §3.4.

### 2.11 Bots, and how a client discovers them

*"There is no new primitive to learn: a Bot **is** a Hermes profile — isolated config, memory, skills,
credentials, and chat history under `~/.hermes/profiles/<name>/`. Bot Mode is a UI over that primitive."*
(`bot-mode.md`). Per-bot: model & provider pin, custom `SOUL.md`, per-skill/toolset/MCP enablement, shared
credential pool; avatar, title, description, hidden flag and groups live in backend-synced profile metadata.
Bot Chat is a forever-chat whose identity is a session **titled exactly `"Bot Chat"`**
(`tui_gateway/methods_profiles.py`); `/new` and `/reset` inside it reroute to `/compact` `[SOURCE]` R1 §4.1.

**Discovery.** The bearer API server exposes **no** roster (§2.3). The two working doors:

1. **Dashboard REST `GET /api/profiles`** (auth `X-Hermes-Session-Token`, §2.15) → `{"profiles": ProfileInfo[]}` with `ProfileInfo { name, path, is_default, model, provider, has_env, skill_count, gateway_running, description, description_auto, display_name?, distribution_name, distribution_version, distribution_source, has_alias }` (`web/src/lib/api.ts`). Siblings: `/api/profiles/active`, `POST /api/profiles`, `PATCH|DELETE /api/profiles/{name}`, `/soul`, `/setup-command`, `/export`, `/import`, `/api/profiles/sessions`, `/sessions/sidebar`, `/projects/tree` `[SOURCE]` R1 §4.2(a).
2. **TUI-gateway RPC `profiles.list`** — *"the ws twin of the dashboard's `/api/profiles`"*, param `include_sessions` (default true), rows `{name, path, is_default, model, provider, description, display_name, skill_count, last_session{id,title,preview,started_at,last_active,message_count}|None, worker_session{id,source,title,last_active}|None, canonical_session{id,resolved_id,root_title,title,preview,started_at,last_active,message_count}|None}`. Opens each profile DB `read_only=True` because the desktop polls every 5 s `[SOURCE]` R1 §4.2(b).

Bot-to-bot (`message_agent`) is gated by `agent.bot_mode_protocol: true`; cross-machine peers are
`hermes peer add <name> --url <base> --key <API_SERVER_KEY>`, with typed failure reasons a client can branch
on (`provider_auth_or_access`, `provider_quota_limit`, `provider_rate_limit`, `provider_server_error`,
`context_overflow`, `missing_config`, `model_unavailable`, `runtime_offline`, `queued_expired`,
`delivery_timeout`, `target_busy`, `unknown`) `[SOURCE]` R1 §4.1.

### 2.12 Sessions and history

- `~/.hermes/state.db` holds session id, source, user id, **unique title**, model, system-prompt snapshot, full history, token counts, timestamps, **parent session id** (compression lineage). Id format `YYYYMMDD_HHMMSS_<hex>` (6-char hex CLI/TUI, 8-char gateway) `[SOURCE]` R1 §5.1.
- API server: `GET /api/sessions?limit=&offset=&source=&include_children=`; `GET /api/sessions/{id}/messages` → `payload["data"][]` each with `content`, and `payload["pagination"] == {"limit":500,"offset":0,"order":"latest","returned":500}` — **the default is the latest bounded page of 500** `[SOURCE]` R1 §5.2.
- Dashboard: `/api/sessions` (+`order`), `/{id}`, `/{id}/messages?limit=500&order=latest`, `/{id}/latest-descendant` → `{requested_session_id, session_id, path[], changed}`, `/search?q=`, `/stats`, `/bulk-delete`, `/prune`, `/import`, `/export`; `SessionInfo = {id, source, model, title, started_at, ended_at, last_active, is_active, message_count, tool_call_count, input_tokens, output_tokens, preview, parent_session_id?}`; cross-profile `GET /api/profiles/sessions?…&archived=exclude|only|include&order=created|recent&profile=<name>|all` `[SOURCE]` R1 §5.2.
- Semantics a client must respect: compression mints a **new continuation session** (`"my project"` → `"my project #2"`); `/v1/responses` chains via `previous_response_id` or a named `conversation`; `POST /v1/runs` with `session_id` loads that transcript under session turn leases; stored responses max 100, LRU, survive restarts `[SOURCE]` R1 §5.3.

### 2.13 Tools, MCP, filesystem permissions on the Hermes side

- Enumeration: `GET /v1/toolsets` → `{"object":"list","platform":"api_server","data":[{"name","label","description","enabled","configured","tools":[…]}]}`; `GET /v1/skills` → `[{"name","description","category"}]` `[SOURCE]` R1 §8.1.
- MCP: `mcp_servers:` in `config.yaml` with stdio (`command`/`args`), HTTP (`url`/`headers`) and OAuth (`auth: oauth`) transports; SDK `mcp==2.0.0` implementing **MCP revision 2026-07-28**; per-server `tools.include`/`tools.exclude` globs; OAuth 2.1 with PKCE, tokens at `~/.hermes/mcp-tokens/<server>.json` mode `0o600`; substitution `${VAR}`, `${env:VAR}`, `${userHome}`, `${workspaceFolder}` `[SOURCE]` R1 §8.2.
- Always-blocked write targets for `write_file`/`patch` (no prompt, no override): OS credential stores (`~/.ssh/`, `~/.aws/`, `~/.kube/`, `/etc/sudoers`, `~/.netrc`), Hermes credential stores (`auth.json`, `.env`, `.anthropic_oauth.json`, `mcp-tokens/`, `pairing/`), project secret files (`.env*`, `.envrc`) anywhere. `~/.ssh/config` is approval-gated. Allowed roots: `HERMES_WRITE_SAFE_ROOT` (`:`-separated on Unix). Honest scope, quoted: *"Write guards apply to `write_file` and `patch` only. The `terminal` tool runs as the same OS user and can still `cat` or overwrite denied paths"* `[SOURCE]` R1 §8.3.
- Command approval: `approvals.mode: smart|manual|off` (default `smart`), `timeout: 300` fail-closed, and **`unattended_mode: deny|approve` governs `api_server` sessions and defaults to `deny`** — an approval-needing command over the API server blocks instantly rather than hanging. Choices offered: `once | session | always | deny`. `UNRECOVERABLE_BLOCKLIST` (`tools/approval.py`) can never be overridden; container backends skip the check entirely `[SOURCE]` R1 §8.3.

### 2.14 Slash commands, deliverable mode, voice and wake word

Slash commands are §14.2; the voice pipeline and wake word are §10.7. Deliverable mode: *"The gateway scans
agent responses for file paths. Any absolute path (`/tmp/...`) or home-relative path (`~/...`) ending in a
supported extension gets extracted. Paths inside code blocks and inline code are ignored"*; an explicit
`MEDIA:` directive exists (`gateway/platforms/base.py`: `MEDIA_TAG_CLEANUP_RE`, `validate_media_delivery_path`,
symlinks rejected); extension allowlist by category (images `.png .jpg .jpeg .gif .webp .bmp .tiff .svg` inline;
video, audio, documents, data, geospatial, presentations, archives, web as attachments; `.py`/`.log`
**deliberately excluded**); config keys `gateway.strict` / `HERMES_MEDIA_DELIVERY_STRICT`,
`gateway.media_delivery_allow_dirs` / `HERMES_MEDIA_ALLOW_DIRS`, `gateway.trust_recent_files` /
`HERMES_MEDIA_TRUST_RECENT_FILES` (recency default 600 s), env wins `[SOURCE]` R1 §7.1–7.2.

**Constraint.** Deliverable mode is a **messaging-gateway** behaviour; the API server has no deliverable
endpoint — `/v1/artifacts/*` belongs to browser-control scope. An HTTP client sees the assistant text and
would have to parse the absolute path itself **and have filesystem access to the gateway host** `[SOURCE]`
R1 §7.2.

### 2.15 Auth and multi-tenancy — four credential schemes

1. **API server bearer.** *"`API_SERVER_KEY` is **required for every deployment**, including the default loopback bind"*; non-ASCII bearer → 401 not 500; with no key configured, `_check_auth` returns `None` — everything allowed. Env/keys: `API_SERVER_ENABLED` (false), `API_SERVER_PORT` (8642), `API_SERVER_HOST` (127.0.0.1), `API_SERVER_KEY`, `API_SERVER_CORS_ORIGINS`, `API_SERVER_MODEL_NAME`, `max_concurrent_runs` (10) `[SOURCE]` R1 §9.1.
2. **`/p/<profile>/` prefix routing.** Auth binds to the routed profile: the profile's own key from `~/.hermes/profiles/<profile>/.env`; the default key is **rejected** on named prefixes (July 2026 breaking change); an unserved prefix → 404 `_PROFILE_REJECTED` (#91583 defect 2) `[SOURCE]` R1 §9.2.
3. **Dashboard/desktop backend.** `X-Hermes-Session-Token`, injected into `index.html` as `window.__HERMES_SESSION_TOKEN__`, **never fetched via API**, rotating on restart. Gated mode uses cookie auth; WebSocket auth via a single-use ticket (TTL 30 s, `POST /api/auth/ws-ticket`), close codes 4403 / 4401. A loopback bind only accepts loopback clients; a public bind fails closed without an auth provider; the DNS-rebinding guard requires the `Host` header to match the bind `[SOURCE]` R1 §9.3.
4. **Native desktop sign-in (RFC 8252 + PKCE)** when `auth_flows` contains `native_pkce`: loopback listener → system browser → `GET /auth/native/authorize` → `POST /auth/native/token` → `{access_token, refresh_token, expires_at}`, refresh via `POST /auth/native/refresh`; tokens stored 0600, OS-keychain encryption opt-in `[SOURCE]` R1 §9.4. Plus **RoomLink grants** (`HermesRoom`) on `/v1/room-members/*` and the `/v1/runs` family, with 401 `invalid_room_grant` / 403 `room_reauthorization_required` `[SOURCE]` R1 §9.5.

CORS is **off by default** (`API_SERVER_CORS_ORIGINS` allowlist); every response carries
`Content-Security-Policy: default-src 'none'; frame-ancestors 'none'`, `Permissions-Policy: camera=(),
microphone=(), geolocation=()`, HSTS, `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`,
`Referrer-Policy: no-referrer` `[SOURCE]` R1 §9.6.

**Constraint.** Enumerating bots needs scheme 3 or the TUI-gateway WebSocket; chatting needs scheme 1 (or
2 per bot). A keeper "bot picker" over Hermes therefore holds **two credentials of different shapes** per
gateway, one of which (the dashboard token) is not obtainable over any API `[INFERENCE]` from R1 §4.2, §9.3.

### 2.16 Limits, error envelope, versioning risk

Concurrency: `max_concurrent_runs` default 10 → 429 `Too many concurrent runs (max N)` + `Retry-After`; SSE
buffer expires after 5 min unconsumed; no general per-IP/per-token rate limiter found `[SOURCE]` R1 §10.1.

Error envelope, from source: `{"error":{"message","type","param","code"}}`, `type` defaulting to
`"invalid_request_error"`; **every outward message is redacted** by `_redact_api_error_text` (masks values,
keeps structure, `force=True`). Status map: 400 malformed JSON / missing `messages` / `unsupported_content_type`
/ provider-vs-alias conflict; 401 bad bearer or default key on `/p/<named>/`; 404 unserved prefix; 409
`idempotency_key_conflict`, `run_not_accepting_steer`; 429 concurrency `[SOURCE]` R1 §10.2.

Known breaks a client must survive: the July 2026 prefix-auth break; #91583; `direct_model_requests`
defaulting false (a hard-coded `model` is silently ignored); older backends lacking `/api/audio/voice-config`,
`commands.catalog` ranking, `/api/profiles/sessions/sidebar`, native PKCE, `wake.feed`; **reverted in the
0.21.0 window — do not design against**: Model Council `/council`, the DCP context engine, the WS-only gateway
server (#94245 reverted by #96118), Electron rollback; session ids rotate on compression `[SOURCE]` R1 §10.3.

---

## 3. Ollama

All of §3 is `[SOURCE]` R2 unless marked. Pins: stable **v0.33.2** (2026-08-27), prerelease v0.33.3-rc0
(2026-09-02), `main` @ `e5e4377` (<https://api.github.com/repos/ollama/ollama/releases/latest>). Docs are
mid-migration: `docs/api.md` opens *"Ollama's API docs are moving to https://docs.ollama.com/api"*;
`docs/openai.md` is gone (404); the OpenAI-compat page is `docs/api/openai-compatibility.mdx`; `docs/openapi.yaml`
exists but **lags the Go types** `[SOURCE]` R2 §1
(<https://raw.githubusercontent.com/ollama/ollama/main/docs/api.md>).

### 3.1 Native endpoint map (`server/routes.go` `GenerateRoutes()`, L1879–1930)

| Path | Handler | Note |
| --- | --- | --- |
| `GET/HEAD /` | inline | literal `Ollama is running` |
| `GET/HEAD /api/version` | inline | `{"version":…}` — cheapest probe, unauthenticated |
| `GET /api/status` | `StatusHandler` | undocumented; `{"cloud":{"disabled","source"}}` |
| `GET/HEAD /api/tags` | `ListHandler` | local models, **with `capabilities`** (§3.3) |
| `POST /api/show` | `ShowHandler` | metadata + `capabilities` (cached since v0.23.2) |
| `POST /api/chat`, `/api/generate`, `/api/embed`, `/api/embeddings` | — | `/api/embeddings` superseded, still routed |
| `POST /api/pull`, `/api/push`, `/api/create`, `/api/copy`, `DELETE /api/delete` | — | model management, **unauthenticated** (§3.8) |
| `GET /api/ps` | `PsHandler` | loaded models with `context_length`, `size_vram`, `expires_at` |
| `POST /api/blobs/:digest`, `HEAD` | — | |
| `POST /api/experimental/web_search`, `/web_fetch`; `GET /api/experimental/model-recommendations` | — | proxied to cloud |
| `POST /api/me`, `/api/signout` | — | undocumented; ollama.com identity |
| `POST /v1/chat/completions`, `/v1/completions`, `/v1/embeddings`; `GET /v1/models`, `/v1/models/:model`; `POST /v1/responses`; `POST /v1/audio/transcriptions`; `POST /v1/messages` | via middleware | §3.5 |

`r.HandleMethodNotAllowed = true` — a wrong verb is 405, not 404 `[SOURCE]` R2 §2.

### 3.2 `/api/chat` and `/api/generate` — request and response

`ChatRequest` (`api/types.go:133–180`): `model`, `messages`, `stream`, `format`, `keep_alive`, `tools`,
`options`, `think`, `truncate`, `shift`, `logprobs`, `top_logprobs` (0–20). `Message`: `role` (lowercased on
unmarshal), `content`, `thinking`, `images`, `tool_calls`, `tool_name`, `tool_call_id`. `ToolCall` =
`{ "id"?, "function": { "index", "name", "arguments": object } }` — **`arguments` is a JSON object, not a
string**, insertion-ordered `[SOURCE]` R2 §2.3. `truncate` and `shift` are in the Go type and not in
`docs/api.md` (`api/types.go:111–117`) `[SOURCE]` R2 §2.1.

Response `ChatResponse` (`api/types.go:520–549`): `model`, `remote_model`, `remote_host`, `created_at`,
`message`, `done`, `done_reason`, `logprobs`, embedded `Metrics` (`total_duration`, `load_duration`,
`prompt_eval_duration`, `eval_duration` in **nanoseconds**; `prompt_eval_count`, `eval_count`) `[SOURCE]` R2 §2.2–2.3.

Streaming is **NDJSON** (`application/x-ndjson`), one object per line, no `data:` prefix, no `[DONE]`;
tool-call frames arrive as `message.tool_calls` with `done:false` (`docs/api/streaming.mdx`,
<https://raw.githubusercontent.com/ollama/ollama/main/docs/api/streaming.mdx>) `[SOURCE]` R2 §2.4.

`done_reason` exact set (`llm/server.go:248–266` + `server/routes.go:415,486,2516,2654`): `stop`, `length`,
`load`, `unload`, and **empty string** (`DoneReasonConnectionClosed`). Treat missing/empty as aborted, not
stop `[SOURCE]` R2 §2.5.

### 3.3 Model and capability discovery

- `GET /api/tags` → `ListModelResponse { name, model, remote_model, remote_host, modified_at, size, digest, details{parent_model,format,family,families,parameter_size,quantization_level}, capabilities[] }` (`api/types.go:832–842`). `capabilities` is served here too — one round trip classifies every local model — but the docs' example and `openapi.yaml`'s `ModelSummary` omit it, and the release that added it could not be found (§16). **A client must treat `capabilities` on `/api/tags` as best-effort and fall back to `/api/show`** `[SOURCE]` R2 §4.1, §14.1.
- `POST /api/show` → `license, modelfile, parameters, template, system, renderer, parser, details, messages, remote_model, remote_host, model_info, projector_info, tensors, capabilities, modified_at, requires` (`api/types.go:737–755`); the deprecated `name` request field still works via `cmp.Or(r.Model, r.Name)` `[SOURCE]` R2 §4.2.
- Vocabulary (`types/model/capability.go`, exhaustive today): `completion`, `tools`, `insert`, `vision`, `embedding`, `thinking`, `image`, `audio` — eight; **treat as open-ended and ignore unknown strings**. Derived by GGUF metadata and **chat-template sniffing** (`server/images.go:190,230,279`): a model can advertise `tools` and still emit malformed calls; a stripped template can advertise nothing. Mismatch → 400 `"%q does not support thinking"` / `"…generate"` / `"…chat"` (`server/routes.go:464,471,2631,2639`), after a `slog.Warn` relaxation attempt for thinking `[SOURCE]` R2 §4.3.
- Context length — three distinct answers: (1) trained max in `model_info["<general.architecture>.context_length"]` (no architecture-neutral alias); (2) what the server allocates: *"< 24 GiB VRAM: 4k context; 24-48 GiB VRAM: 32k context; >= 48 GiB VRAM: 256k context"* (`docs/context-length.mdx`), overridable by `OLLAMA_CONTEXT_LENGTH` (`envconfig/config.go:230`) or per request `options.num_ctx`; cloud models are context-maxed; *"agents, and coding tools should be set to at least 64000 tokens"*; (3) what a loaded instance runs with: `context_length` on `/api/ps` `[SOURCE]` R2 §4.4–4.5.
- `keep_alive`: Go duration or integer seconds; negative = infinite, zero = none, default 5 min (`envconfig/config.go:126–137`); `keep_alive: 0` + empty messages = explicit unload (`done_reason: "unload"`); empty messages alone = preload (`"load"`) `[SOURCE]` R2 §4.5.

### 3.4 Thinking

`think` is a boolean or `"low" | "medium" | "high" | "max"` (`api/types.go:1142–1155`). *"Thinking is enabled by
default in the CLI and API for supported models"* — clean output needs `think: false` explicitly; GPT-OSS
ignores booleans and *"the trace cannot be fully disabled"*. Native response field: `message.thinking`
(chat) / `thinking` (generate), streamed as deltas **before** content. OpenAI layer: `choices[].message.reasoning`
/ `choices[].delta.reasoning` (`openai/openai.go:32–48`) — **not** `reasoning_content`; inbound control
`reasoning_effort` (string) or `reasoning: {effort}` mapped to `think` `[SOURCE]` R2 §6
(<https://raw.githubusercontent.com/ollama/ollama/main/docs/capabilities/thinking.mdx>).

### 3.5 The OpenAI-compatible layer (`middleware/openai.go`, `openai/openai.go`)

`/v1/chat/completions` support matrix from `docs/api/openai-compatibility.mdx`
(<https://raw.githubusercontent.com/ollama/ollama/main/docs/api/openai-compatibility.mdx>):

| ✔ honoured | ✘ not honoured |
| --- | --- |
| `model`, `messages` (text, base64 image content, content-part arrays), `frequency_penalty`, `presence_penalty`, `response_format`, `seed`, `stop`, `stream`, `stream_options.include_usage`, `temperature`, `top_p`, `max_tokens`, `tools`, `reasoning_effort` / `reasoning.effort` | **Image URL** (base64 only), **`tool_choice`**, `logit_bias`, `user`, `n` |

The doc's *Logprobs ✘* row is stale: `openai/openai.go:50–52,121–122,272–274,718–719` plumbs logprobs and
release v0.12.11 shipped them on both surfaces `[SOURCE]` R2 §5.2. `/v1/responses`: non-stateful only
(no `previous_response_id`, no `conversation`, no `truncation`) `[SOURCE]` R2 §5.1. `/v1/messages`
(Anthropic Messages) exists since v0.14.0 `[SOURCE]` R2 §5.5.

Differences that bite a client written against OpenAI `[SOURCE]` R2 §5.4: (1) `/v1/*` is real SSE
(`data: …\n\n`, `data: [DONE]`; `middleware/openai.go:123,157`) while `/api/*` is NDJSON — two framers;
(2) native `arguments` is an object, OpenAI-shaped is a string; (3) `tool_choice` ignored; (4) no `n`;
(5) **context size cannot be set over `/v1`** — the doc says create a Modelfile with `PARAMETER num_ctx`;
native `/api/chat` takes `options.num_ctx` inline; (6) model names are `model:tag`; (7) the model must be
pulled first; (8) reasoning is a non-OpenAI field; (9) `top_p` defaults to `1.0` when absent
(`openai/openai.go:681`); (10) bodies may be zstd-compressed (20 MiB cap).

`api_key`: **no auth middleware exists** — `GenerateRoutes()` installs only CORS and the Host guard; the
doc's `api_key='ollama', # required but ignored`; on `/api/experimental/web_search` the client's
`Authorization` header is actively **stripped** (`server/routes.go:2168–2171`) `[SOURCE]` R2 §5.3.

### 3.6 Tools and vision

Tools: `tools` on `/api/chat` since v0.3.0; streamed tool calls since v0.4.6; `tool_name` on `role: "tool"`
since v0.9.6 `[SOURCE]` R2 §11. Vision: native `images` = **bare base64, no data-URI prefix**; OpenAI layer
accepts `image_url` **only as a `data:` URI**, and the doc's samples pass it as a *bare string*, not
`{"url":…}` (whether the object form is also accepted is §16); SVG rejected since v0.4.6; `vision` capability
gates it `[SOURCE]` R2 §7 (<https://raw.githubusercontent.com/ollama/ollama/main/docs/capabilities/vision.mdx>).

### 3.7 Structured output

Native `format` = `"json"` or a raw JSON Schema object, no wrapper; OpenAI `response_format` maps
`json_object` → `"json"` and `json_schema` → `.json_schema.schema` (**`name` and `strict` dropped**;
unknown `type` silently ignored → no constrained decoding, no error) (`openai/openai.go:685–695`). **Cloud
models do not support structured outputs.** v0.31.2 fixed format+think interaction — treat the pair as
version-sensitive `[SOURCE]` R2 §9.

### 3.8 Remote: binding, auth, CORS, TLS, cloud

- Default `http://127.0.0.1:11434` (`envconfig/config.go:20–22`); `OLLAMA_HOST=0.0.0.0:11434` to expose. **No authentication on the local/LAN API** (`docs/api/authentication.mdx`: *"No authentication is required when accessing Ollama's API locally"*; the only authenticated activities are cloud models, publishing, private downloads). **Binding to `0.0.0.0` publishes an unauthenticated inference and model-management API** including `DELETE /api/delete`. `OLLAMA_AUTH` is client-side Ed25519 request signing (`api/client.go:119,184`); no server-side verifier was found (§16) `[SOURCE]` R2 §8.1–8.2 (<https://raw.githubusercontent.com/ollama/ollama/main/docs/api/authentication.mdx>).
- CORS: `OLLAMA_ORIGINS` plus always-appended defaults including **`tauri://*`**, `app://*`, `file://*`, `vscode-webview://*` (`envconfig/config.go:86–108`) `[SOURCE]` R2 §8.3.
- **DNS-rebinding guard** `allowedHostsMiddleware` (`server/routes.go:1806–1842`): on a loopback listener the `Host` header must be empty, `localhost`, the machine hostname, a loopback/private/unspecified IP, or end in `.localhost`/`.local`/`.internal`, else **403 with no JSON body**; skipped entirely on a non-loopback bind. Every tunnelling recipe rewrites Host (`proxy_set_header Host localhost:11434`) `[SOURCE]` R2 §8.3.
- TLS: plain HTTP only; TLS is the proxy's job `[SOURCE]` R2 §8.3.
- Cloud: (a) local daemon proxying `-cloud` tags after `ollama signin` — client sees 502 and `{"error":"unauthorized","signin_url":…}` shapes (`server/routes.go:391,2598`); (b) `https://ollama.com` as a remote host with `Authorization: Bearer $OLLAMA_API_KEY`, same wire, keys don't expire; models retire on a published schedule; `OLLAMA_NO_CLOUD=1` disables cloud host-wide, visible via `/api/status` `[SOURCE]` R2 §8.4–8.5 (<https://raw.githubusercontent.com/ollama/ollama/main/docs/cloud.mdx>).

### 3.9 Error shapes

`docs/api/errors.mdx` (<https://raw.githubusercontent.com/ollama/ollama/main/docs/api/errors.mdx>): 400, 404,
429, 500, 502; body `{"error": "…"}`. **Mid-stream errors keep the 200 and append `{"error":…}` as an NDJSON
line** — a client must check every frame for `error`. Exact strings: 404 `model "<name>" not found, try pulling
it first` (`server/routes.go:3119`) — **no auto-pull**; 404 `model '<name>' not found`; **HTTP 499
`request canceled`** on client cancellation; 503 on `ErrMaxQueue`; 500 with raw Go text otherwise. **OOM has no
dedicated status or code** (§16). OpenAI-layer errors: `{"error":{"message","type","param","code":null}}` with
`type` ∈ `invalid_request_error | not_found_error | api_error` — `not_found_error` is not an OpenAI type
`[SOURCE]` R2 §10.

### 3.10 Minimum versions

| Feature | First release |
| --- | --- |
| tools on `/api/chat` | v0.3.0 (2024-07-25) |
| streamed tool calls | v0.4.6 (2024-11-28); structured `format` v0.5.0; `OLLAMA_CONTEXT_LENGTH` v0.5.13 |
| `capabilities` in `/api/show` | **v0.6.4** (2025-04-02) |
| `think` / `thinking` | v0.9.0 (2025-05-29) |
| `tool_name` on tool messages | v0.9.6 |
| `context_length` on `/api/ps` | v0.10.0 |
| `reasoning_effort` (OpenAI layer) | v0.11.5 |
| cloud models; logprobs | v0.12.0/v0.12.1; v0.12.11 |
| `/v1/messages` | v0.14.0; `REQUIRES` in Modelfiles and `ShowResponse.requires` same release |
| `think: "max"` | v0.21.3-rc0 / first stable v0.22.0 |
| `/v1/responses` | doc says v0.13.3; first release note v0.14.2 (§16) |

Probe ladder: `/api/version` (parse three integers, do not assume pre-release ordering) → `/api/tags`
(capabilities present?) → `/api/show` (absent below v0.6.4 → heuristics on `details.families` and
`template`). Unsupported **capabilities** 400 loudly; unsupported **fields** are silently dropped by Go's
JSON decoder — sending `think` to a v0.8 server yields reasoning inlined in `content` `[SOURCE]` R2 §11.

Licence: `ollama/ollama` is MIT (`https://raw.githubusercontent.com/ollama/ollama/main/LICENSE`); model
weights are licensed separately and `/api/show` returns a per-model `license` string `[SOURCE]` R2 §12.

---

## 4. The common denominator, and the streaming/tool-call state machine

### 4.1 What both back ends honour on the OpenAI wire

The intersection of R1 §2.4 and R2 §5.1, both `[SOURCE]`:

| Wire element | Hermes `/v1/chat/completions` | Ollama `/v1/chat/completions` | In the intersection? |
| --- | --- | --- | --- |
| `messages` with `system` / `user` / `assistant` / `tool` roles | yes | yes | yes |
| `stream: true` → SSE `chat.completion.chunk` + `data: [DONE]` | yes (keepalive 30 s) | yes | yes |
| `stream_options.include_usage` | not stated | yes | Hermes side `[UNVERIFIED]` |
| image content part, `data:` URI | yes (`image_url:{url,detail}`) | yes, **bare string** in docs | shape differs (§4.2) |
| image content part, remote `http(s)` URL | yes | **no** | no |
| `tools[]` request | ignored — tools are server-side | yes | no |
| `tool_calls` in `delta` for the **client** to execute | never | yes | no |
| `tool_choice` | n/a | ignored | no |
| `model` | ignored without `provider` unless `direct_model_requests` | required, `model:tag`, must be pulled | semantics differ |
| `reasoning_effort` | via `model_options.reasoning_effort` | `reasoning_effort` / `reasoning.effort` | spelling differs |
| reasoning in the response | not stated | `delta.reasoning` | Hermes side `[UNVERIFIED]` |
| `response_format` | not stated | `json_object` / `json_schema.schema` (name/strict dropped) | Hermes side `[UNVERIFIED]` |
| `GET /v1/models` | one alias (+ `model_routes` aliases) | every local tag | both, different meaning |
| `usage` block | yes | yes | yes |
| error body | `{"error":{"message","type","param","code"}}` | `{"error":{"message","type","param","code"}}` | same shape, different `type` vocabularies |
| auth | bearer **required** | **none**, any non-empty key accepted | no |
| `n` | not stated | no | no |

**Constraint.** The honest common subset is *system + user + assistant text, one streamed answer, `data:` images,
usage*. Everything the ask adds on top — tool-calls the client executes, model choice, remote images, forced
tools — is one-sided. A single "OpenAI-compatible provider" abstraction in keeper would have to carry a
per-back-end capability record or it will misreport half the features `[INFERENCE]` from the table.

### 4.2 Every divergence, in one table

| Divergence | Hermes | Ollama | Evidence |
| --- | --- | --- | --- |
| streaming transport | SSE on `/v1`; JSON-RPC over stdio/WS on the TUI gateway (33 ms coalescing) | SSE on `/v1`; **NDJSON** on `/api` | R1 §2.6, §3.1; R2 §2.4, §5.4 |
| named SSE events | `event: hermes.tool.progress` interleaved with chunks | none | R1 §2.6 |
| tool execution | server; replayed as `status: "completed"` | client; `tool_calls` with `arguments` **object** (native) / **string** (`/v1`) | R1 §2.7; R2 §2.3, §5.4 |
| tool registration | impossible over HTTP; MCP servers in `config.yaml` | per request `tools[]` | R1 §8.1–8.2; R2 §2.3 |
| model selection | profile-bound; `provider` required to honour `model` | `model:tag` required | R1 §2.4; R2 §5.4 |
| bot/profile discovery | not on the bearer listener; dashboard REST or WS RPC | n/a — models, not bots | R1 §4.2 |
| capabilities | `/v1/capabilities` (features) + `/api/model/options` (per model) | `capabilities[]` on `/api/tags` / `/api/show` | R1 §2.10, §4.3; R2 §4.3 |
| context window | `context_window` per model in `/api/model/options` | three answers; **not settable over `/v1`** | R1 §4.3; R2 §4.4, §5.4 |
| reasoning | `thinking.delta` (TUI gateway); OpenAI-side `[UNVERIFIED]` | `delta.reasoning`; native `message.thinking` first | R1 §3.1; R2 §6 |
| images | `data:` and remote on both APIs | `data:` only, bare string | R1 §2.5; R2 §7 |
| sessions | server-side, ids rotate on compression, `X-Hermes-Session-*` headers | none — client owns history | R1 §5; R2 §5.1 (`/v1/responses` non-stateful) |
| auth | bearer required; four schemes | none; DNS-rebinding Host guard | R1 §9; R2 §8.2–8.3 |
| cancellation | `POST /v1/runs/{id}/stop`; `session.interrupt` RPC | drop the connection → 499 | R1 §2.2, §3.2; R2 §10 |
| in-band error mid-stream | SSE error events (redacted) | NDJSON `{"error":…}` after a 200 | R1 §10.2; R2 §10 |
| retry hints | `Retry-After` on 429 | 429 / 503 with no documented `Retry-After` | R1 §2.8; R2 §10 |
| structured output | `[UNVERIFIED]` | `format` / `response_format`; **not on cloud** | R2 §9 |

### 4.3 The reassembly state machine (Chat Completions, `stream: true`)

The framing is WHATWG SSE: `field: value` lines, frames separated by a blank line, `:`-prefixed comments;
every frame is `data: <chat.completion.chunk>` and the stream ends with one literal `data: [DONE]` (stated in
prose under `stream_options.include_usage`: *"an additional chunk will be streamed before the `data: [DONE]`
message"*, and *"If the stream is interrupted, you may not receive the final usage chunk"*) `[SOURCE]` R3 §1.1
(OpenAI, <https://developers.openai.com/api/reference/resources/chat.md>). No machine-readable declaration
of `[DONE]` exists in the schema page (§16).

Chunk surface (`CreateChatCompletionStreamResponse`): `id` (same on every chunk), `choices[]`, `created`,
`model`, `object: "chat.completion.chunk"`, optional `moderation`, `obfuscation`, `service_tier`,
`system_fingerprint` (deprecated), `usage`; `choices[].{index, delta, finish_reason, logprobs}`;
`finish_reason` ∈ `stop | length | content_filter | tool_calls | function_call`; `delta.{content, role, refusal,
tool_calls[], function_call}`; `delta.tool_calls[].{index, id, type, function:{name, arguments}}` with the
warning *"the model does not always generate valid JSON, and may hallucinate parameters… Validate the
arguments in your code"* `[SOURCE]` R3 §1.2
(<https://developers.openai.com/api/reference/resources/chat/subresources/completions/streaming-events.md>).

The minimum correct machine `[SOURCE]` R3 §1.3, each rule cited to a shipped client:

```
state: content, reasoning, refusal, calls: IndexMap<u32, {id, name, args: String}>,
       finish_reason, usage, id/model/created (first chunk), saw_done, saw_any_valid_frame
per frame:
  comment (`:`)                       -> ignore (keep-alive)
  data == "[DONE]"                    -> saw_done = true; stop
  blank data                          -> skip
  JSON parse failure                  -> policy decision (see (i))
  json.error present                  -> terminate with provider error
  pick the choice with index == 0     (not choices[0])
  delta.role                          -> record once, never append
  delta.content non-empty             -> content.push_str
  delta.reasoning_content | reasoning -> reasoning.push_str
  delta.refusal                       -> refusal.push_str
  for tc in delta.tool_calls (loop, never .first()):
     slot = calls.entry(tc.index); id first-write-wins; name append; args.push_str(fragment)
  finish_reason present               -> record, but DRAIN content/tool_calls of the same chunk first
  usage non-null                      -> record
after loop: parse each call's args ONCE
```

| Trap | Rule and evidence |
| --- | --- |
| (a) arguments | accumulate the **string**, parse once at the end (`rust-genai` `src/adapter/adapters/openai/streamer.rs`, <https://github.com/jeremychone/rust-genai>) |
| (b) keying | by `index`, not array position and not `id` — *"the Chat Completions wire keys tool call fragments by chunk index"* (`rig-core` `openai_chat_completions_compatible.rs`, <https://github.com/0xPlaygrounds/rig>) |
| (c) choice | select `index == 0`; `n > 1` interleaves candidates (`rig` `providers/openai/completion/streaming.rs`) |
| (d) same-chunk payload | *"Some providers (e.g., Ollama) send tool_calls AND finish_reason in the same message"*; Mistral does it with content — a machine that `continue`s on `finish_reason` loses a call (genai) |
| (e) missing `finish_reason` | normal mid-stream, and at the end on xAI (genai) |
| (f) terminated vs truncated | only `[DONE]` or a `finish_reason` counts as completion; EOF without either **gets no terminal record** — synthesising one *"would present the partial turn as a successful, default-usage completion"* (rig; codex `codex-rs/codex-api/src/sse/responses.rs` `"stream closed before response.completed"`, <https://github.com/openai/codex>) |
| (g) in-band `error` | must not be swallowed; openai-python raises `APIError` when a decoded frame has `error` (`src/openai/_streaming.py`, <https://github.com/openai/openai-python>) |
| (h) empty deltas | role-only first chunk, `{"delta":{}}` last chunk, and a final `choices: []` usage chunk are all normal |
| (i) malformed frames | a **decision**: codex logs and continues; rig triages `Known / Unknown (warn-and-skip) / Corrupt (error the stream)` and refuses to demote a broken known frame; duplicate discriminator keys are rejected outright (`providers/internal/wire.rs`) |
| (j) keep-alives | SSE comments (dropped by `eventsource-stream` `RawEventLine::Comment`) **and** named `event: keepalive` (async-openai skips it); a bare `data:` yields no event (<https://github.com/jpopesculian/eventsource-stream>) |
| (k) gapped indices | nothing promises dense or ascending `index` (§16); an `IndexMap` avoids genai's `Vec::resize` placeholder duplication |
| (l) absent `id` | synthesise `call_{index}` (genai) / provenance-gated `tool-{index}` (rig); rig evicts a slot when a new id+name pair reuses an index |

Provider extras seen on `delta`: `reasoning_content` (DeepSeek-style), `reasoning` (Ollama), `reasoning_details`
(OpenRouter), `native_finish_reason`, `usage` at `/x_groq/usage`, `usage: null` mid-stream `[SOURCE]` R3 §1.4.

**Byte-boundary trap.** `reqwest::Response::bytes_stream()` yields TCP/HTTP2-frame-sized `Bytes`, not SSE
frames — one `Bytes` may hold three frames and half a fourth, and a multi-byte UTF-8 scalar may be split.
`eventsource-stream` (`src/utf8_stream.rs`, `src/event_stream.rs`, 569 lines, MIT OR Apache-2.0) handles both
(test: `[240,159]` then `[145,141]` ⇒ `["", "👍"]`); a hand-rolled parser needs a `String`/`BytesMut`
accumulator, both `\n\n` and `\r\n\r\n` delimiters, and `strip_prefix("data:")` with the space optional
`[SOURCE]` R3 §5.2.

### 4.4 The tool round trip

Request `tools[] = {type:"function", function:{name, description, parameters, strict}}`; `tool_choice`
∈ `none | auto | required | {type:"function",function:{name}}` (Responses nests the name flat — do not share
the serialiser); `parallel_tool_calls: false` ensures zero or one call `[SOURCE]` R3 §2.1
(<https://developers.openai.com/api/docs/guides/function-calling.md>). Strict mode requires
`additionalProperties: false` on every object and every property in `required` (optionality via `"null"` in
`type`); *"Chat Completions requests remain non-strict by default"*. Portable schema subset that survives
OpenAI-strict, vLLM, Ollama and llama.cpp: object root, `type`/`properties`/`required`/`additionalProperties:false`/
`description`/`enum`/`items`, `["string","null"]` unions; whether the local servers enforce `pattern`/`format`/numeric
bounds is §16 `[SOURCE]` R3 §2.2 (<https://developers.openai.com/api/docs/guides/structured-outputs.md>).

Reply side: append the assistant message **verbatim** (including its `tool_calls`), then one `role:"tool"`
message per call with `tool_call_id` and text-only content; dropping a call leaves an unanswered
`tool_call_id`, which OpenAI rejects on the next request. Failure handling: invalid JSON — validate; rig drops a
partial call **only** when `finish_reason == length` (*"Only a provider-declared output-length truncation
authorizes discarding malformed partial arguments"*); hallucinated tool name — resolve locally and answer the
`tool_call_id` with an error string rather than dropping; infinite loops — **no Chat Completions parameter
exists**; Responses' `max_tool_calls` covers built-in tools only; client-side bounding (turn counter + budget)
is mandatory `[SOURCE]` R3 §2.3–2.4
(<https://developers.openai.com/api/reference/resources/responses/methods/create.md>).

### 4.5 Timeouts, cancellation, retries

- reqwest: `timeout` is a **total** deadline (*"from when the request starts connecting until the response body has finished"*); `read_timeout` *"applies to each read operation, and resets after a successful read"*; `connect_timeout` needs a tokio timer; `pool_idle_timeout` default 90 s. **Never set a total `timeout` on a streaming LLM call** `[SOURCE]` R3 §6.1. keeper's `keeper-sync/src/http.rs:24–40` states the same doctrine in its own words — `ClientBuilder::timeout` *"is the wrong question"*, `read_timeout` bounds silence `[REPO]` G4 §3.3.
- Idle timeout wraps **every poll** (codex `timeout(idle_timeout, stream.next())`), ×4 for the unary compact call; axum's default keep-alive is a bare `:` every 15 s, so calibrate idle above the server's keep-alive `[SOURCE]` R3 §6.2 (<https://github.com/tokio-rs/axum>).
- Cancellation: drop is the mechanism, but a forwarding task must `select!` on `tx.closed()` or it *"would keep this task - and the upstream response it holds - alive until the next event arrives"* (async-openai `client.rs` v0.41.3, <https://github.com/64bit/async-openai>); codex threads `tokio_util::sync::CancellationToken` so a UI stop also stops tool execution `[SOURCE]` R3 §6.3. keeper has **no** `CancellationToken` and no `tokio-util`; its three shipped idioms are `Arc<AtomicBool>` in a registry (`keeper/src/ipc.rs:415–444`, `export_cancel` `:2729–2733`), a task handle whose `Drop` aborts (`sessions_ipc.rs:4590–4600`), and `AbortOnDrop` (`ipc.rs:1012–1019`) `[REPO]` G4 §4.3.
- Retries (OpenAI error-codes guide, <https://developers.openai.com/api/docs/guides/error-codes.md>): 429 rate limit → honour `Retry-After`, else exponential backoff with jitter; 429 with `error.code` ∈ `credit_balance_exhausted | *_spend_limit_exceeded | *_usage_limit_exceeded` → **not** retryable (discriminate on `code`, not `type`); 5xx and transport → retry; 503 "Slow Down" → hold rate 15 min. Codex `codex-rs/codex-client/src/retry.rs` is a copyable `RetryOn { retry_429, retry_5xx, retry_transport }` with `saturating_pow` backoff and ±10 % jitter `[SOURCE]` R3 §7.1–7.2.
- **Retrying a partially-streamed completion is unsafe**: no resume token (`reqwest-eventsource`'s `Last-Event-ID` reconnection is exactly wrong), a restart yields a *different* sample, and usage is lost. Retry the whole sampling request from the last committed history state and discard every partial delta `[SOURCE]` R3 §7.3.

### 4.6 Rust crates on the path

| Crate | Version | Licence | Note `[SOURCE]` R2 §13, R3 §5.1 |
| --- | --- | --- | --- |
| `reqwest` | 0.13.4 | MIT OR Apache-2.0 | already in the tree at 0.13, `["json","rustls"]` — **no `stream` feature** `[REPO]` `src-tauri/Cargo.toml:84` G2 §11.2 |
| `eventsource-stream` | 0.2.3 (2022-02-17) | MIT OR Apache-2.0 | pure `Stream<Bytes> → Stream<Event>`, works with reqwest 0.13; deps `futures-core`, `nom ^7.1`, `pin-project-lite` |
| `reqwest-eventsource` | 0.6.0 | MIT OR Apache-2.0 | **pins reqwest ^0.12** — incompatible with a 0.13 tree |
| `sse-stream` | 0.2.5 | MIT OR Apache-2.0 | `http-body`-oriented |
| `async-openai` | 0.41.3 | MIT | typed tool-call chunks, `with_api_base`, correct `select!` cancellation; `default-features = false, features = ["rustls","chat-completion"]` for chat only |
| `rig-core` | 0.42.0 | MIT | the most rigorous reassembly reviewed |
| `genai` | 0.6.5 | MIT OR Apache-2.0 | multi-provider incl. native Ollama; non-terminal branch reads only `tool_calls.first()` |
| `ollama-rs` | 0.3.6 | MIT (manifest `license-file`) | native NDJSON; **`tool-implementations` feature pulls GPL-3.0 `calc` and `html2md`** — never enable; discards `details`/`capabilities` from `/api/tags`; no `/api/ps`, no `/api/version`; `ThinkType` has no `Max` and hard-errors on it |
| `tokio-util` | 0.7.19 | MIT | `CancellationToken`; not in the workspace today `[REPO]` G4 §6.1 |

No GPL/AGPL/LGPL in the SSE/OpenAI set; the only landmine is `ollama-rs/tool-implementations` `[SOURCE]` R2 §13.1, R3 §5.1.

---

## 5. keeper as it is today: what an implementer must follow

All of §5 is `[REPO]` G2 unless marked.

### 5.1 The two-layer law and the crate firewalls

`src-tauri/crates/keeper-core/src/vm.rs:1-7`: *"Every type that crosses the Tauri IPC boundary lives here,
derives `serde` + `ts_rs::TS`, is `#[ts(export)]`, and renames fields to camelCase. Timestamps are `i64`
milliseconds…"* Four crates: `keeper` (Tauri shell), `keeper-core` (pure domain + VMs), `keeper-sync`
(git/LFS/`sync.db`), `keeper-syncd` (Linux daemon). Firewalls gated in `package.json:25-27`:
`check:core-tauri-free`, `check:core-sync-free`, `check:syncd-lean` — `keeper-core` must not depend on tauri
**or** on `keeper-sync`/`gix`; `keeper-syncd` may depend only on `keeper-sync` (§6.9 for the consequence).

### 5.2 Registering a pane

- `PrimaryView` union at `src/lib/stores/primary-view.ts:44-56` (twelve arms); vanilla zustand store `:66-69`; router is the ternary chain in `src/components/layout/app-shell.tsx:304-427`, every capability-gated branch is `flag && primaryView === "x"` (`:344-349`: a stale view *"can never render a pane over a record this build has no database for (AD-137)"*).
- **No lazy loading and no mounted-state preservation** — every pane unmounts on a switch; four hydration effects are mounted at the shell for that reason (`app-shell.tsx:142-157`). A surface that must remember anything across a switch hydrates at the shell or stores it in Rust.
- Sidebar entry: `SidebarView` at `src/components/layout/sidebar-pane.tsx:38-41`; gated entries are one `const` each (`TASKS_VIEW` `:127`), composed at `:256-257`; the rule is **absence, not disabling** (`:106-109`).
- Global overlays mounted unconditionally after the row: `SearchOverlay`, `ExportDialog`, `NewChatDialog`, `CommandPalette` (`app-shell.tsx:433-438`); phone tier replaces the row with `<PhoneShell />` (`:291-295`).

### 5.3 The free keyboard slots

| Chord | Surface | Hook |
| --- | --- | --- |
| ⌘1 / ⌘2 | Inbox / Archive | `src/hooks/use-view-shortcuts.ts:27`, `:43` |
| ⌘3 | Approval | `use-approval-shortcut.ts:16` |
| ⌘4 | Bridges | `use-bridges-shortcut.ts:16` |
| ⌘5 | Recording | `use-recording-shortcut.ts:22` (gate `recording`) |
| ⌘6 | Notes | `use-notes-shortcut.ts:62` (gate `notes`) |
| ⌘7 | Sessions | `use-sessions-shortcut.ts:75` (gate `sessions`) |
| ⌘8 | Tasks | `use-tasks-shortcut.ts:57` (gate `sessions`, deliberately) |
| **⌘9** | **free** — `grep '"9"' src/hooks src/components/layout` returns nothing | — |

There is **no central registry** (`use-view-shortcuts.ts:7-9`); each chord is an ad-hoc `window` keydown
listener. `use-tasks-shortcut.ts:39-74` is the canonical template: IME guard, `metaKey || ctrlKey`, `altKey`
refusal, key match, capability self-gate, `isTypingTarget`, then `preventDefault` + `setView`. Two encoded
rules: **no dead chord** (event left alone when the capability is off, `:60-62`, citing AD-27) and **the
registry chip is display-only; the hook owns the binding** (`:18-20`). ⌘⌥N/K/J/V (notes) and ⌘⌥L (sessions)
chips live in `palette.rs:609,617,625,649,676`.

### 5.4 Pane conventions

- Exported copy constants, never inline strings (`src/components/sessions/sessions-pane.tsx:55-83`); two distinct empty states per pane (`tasks-pane.tsx:70-71`, `sessions-pane.tsx:70`).
- Chrome: `<section>`/`<header>`/`<ScrollArea>` (`tasks-pane.tsx:47-51`); `PaneHeader` owns its bottom edge and 40 px height (`pane-header.tsx:1-50`), enforced by the design gate; column widths in `document.cookie`, never `localStorage`, never IPC (`src/lib/column-widths.ts:1-32`).
- Loading: one-shot reads with a stale-read token and `Promise.allSettled` (`tasks-pane.tsx:2121-2201`), rows already read stay on a refusal; **one clock per pane, never per row** (`files-pane.tsx:1630-1634`); stream-triggered re-read for Sessions (`sessions-pane.tsx:33`). The canonical subscription hook is `src/hooks/use-bridge-health.ts:23-69` — `cancelled` flag plus unsubscribe of a late-arriving id survives StrictMode.
- Windowing: `useWindowedRows` (`src/components/ui/window-list.tsx:235`), measured rows, two extra rows kept mounted (`:24-32`), jsdom needs `withListGeometry` from `src/test/layout.ts`. The Matrix conversation pane does **not** window (`conversation-pane.tsx:1587-1618` maps every row) `[REPO]` G6 §1.1.
- AD-C7: one component, two modes, keyed on presence of the subject; inline disclosure, never a dialog (`tasks-pane.test.tsx:907-911` asserts no `role="dialog"`); option text is the stored spelling (`task-form.tsx:858-860`).

### 5.5 One command and one stream — every file to edit, in order

| # | File | Anchor |
| --- | --- | --- |
| 1 | `src-tauri/crates/keeper-core/src/vm.rs` (or a feature `vm.rs`) | derive block `:23-33`; `*Vm` / `*Req` / `*Batch` naming; `#[ts(type = "number")]` for `u64` (`sync_ipc.rs:2754-2781`) |
| 2 | domain crate | `keeper-sync/src/tasks.rs`, `keeper-sync/src/db.rs` |
| 3 | `src-tauri/crates/keeper/src/<area>_ipc.rs` | one-shot `sync_ipc.rs:2084-2088`; stream pair `:2788-2822` — **subscribe returns an id, the engine drops a sink the moment `Channel::send` returns false** (`:2785-2787`); doc comment ends `Rejects with:` |
| 4 | `src-tauri/crates/keeper/src/ipc.rs` | `#[cfg(not(desktop))]` twins live here (`:1481-1490`), only if a phone can call it |
| 5 | `src-tauri/crates/keeper/src/lib.rs` | **inside the one `keeper_with_commands!` macro** (`:686-702`, shared `:702-900`, desktop `:908-1111`, mobile `:1117-1129`); a second `.invoke_handler(` silently discards the first — shipped as v0.4.0–v0.4.2; guarded by `ipc.rs:13942-13950` |
| 6 | `src/lib/ipc/gen/<Vm>.ts` | generated by `cargo nextest run`; `.cargo/config.toml:5-6`; never hand-edit; 273 files; drift gate `package.json:24` `bindings:check` |
| 7 | `src/lib/ipc/client.ts` | the only importer of `@tauri-apps/api/core` (`:9`, asserted `src/test/command-registration.test.ts:102-108`); `subscribe<TBatch>` arms `onmessage` **before** `invoke` (`:1103-1115`) |
| 8 | `src/hooks/use-<x>.ts` | stream lifecycle, mounted in `AppShell` |
| 9 | `dev/mock-shell.ts` | `ANSWERS` `:1025`, `HANDLERS` `:2098`; every `*_subscribe` falls through to `"sub-mock"` (`:1526-1528`) and **never emits** |
| 10 | tests | `command-registration.test.ts` scans both lists, floors `>200` (`:141-148`) |

DW-4 (closed, won't-fix): a failed `subscribe` leaves one dangling `Channel` callback because `Channel` cannot
be torn down from TS (`deferred-work.md:27-39`); the cleanup an implementer writes is the hook's
`cancelled` + late-id unsubscribe.

### 5.6 View models and the mock shell

- Tolerance convention: `TaskVm.kind`/`.mode` are `String`, not enums, *"so a row a newer keeper wrote must reach the view as the spelling it has"* (NFR-43) — the pattern for provider/model spellings (`db.rs:186-190`). Counter-example: `SyncProfileVm` flattens closed enums to `String` and the frontend re-declares legal values by hand (`src/lib/stores/sync.ts:40,44`) — a live drift risk `[REPO]` G4 §1.10.
- `dev/mock-shell.ts` serves the real components with no Rust (`:1-45`), installed by a Vite serve-only plugin (`vite.config.ts:18-28`), refuses over a real shell (`:2836-2843`). **The capabilities literal is `satisfies CapabilitiesVm` (`:1027-1048`)** — a new flag added in Rust breaks `bun run typecheck` here until answered, and answering `false` makes every fixture for the surface unreachable, which is the exact regression the comment records. A stream-only surface renders its empty state forever under `bun run dev` unless a `HANDLERS` entry exists.

### 5.7 Capability gating

Twelve booleans on `CapabilitiesVm` (`keeper-core/src/vm.rs:92-147`), computed in the shell at
`src-tauri/crates/keeper/src/ipc.rs:1424-1459`: seven pure `cfg!(desktop)`; `recording` = runtime probe
(`:1437`); `sync` = `git_report(&state).state == SyncGitState::Ok` (`:1444`); `overlayTitleBar` (`:1449`);
`notes` and `sessions` both = `notes_available(&state)` = `cfg!(desktop) && sync-ok` (`:1454`, `:1458`).
Served by one `capabilities` command on every platform (`lib.rs:705`), read once by `useCapabilitiesHydrate()`
(`src/App.tsx:27`), mirrored with a frozen all-absent default (`src/lib/stores/capabilities.ts:24-37`);
`isReducedCapabilityPlatform` folds a new flag in automatically (`:81-84`). *"`false` means the surface is
**absent** on this build"* (`vm.rs:85-86`, AD-27 no dead buttons) `[REPO]` G4 §9.3.

A new flag costs three edits: the field on `CapabilitiesVm`, its computation in `ipc.rs:1426-1459`, and both
the frozen default and the mock-shell literal. Standing tension: `use-tasks-shortcut.ts:9-15` and
`palette.rs:918-927` argue a flag whose condition equals an existing one is *"two names for one fact"*; a chat
surface needs neither git nor `sync.db`, but its drive-tool half is exactly `sync` — the epic must say which.

`src/test/no-user-agent-gating.test.ts:1-16` is a **raw-text scan including comments** for `navigator.userAgent`,
`navigator.platform`, the Tauri OS plugin and `import.meta.env` over `src/**` — a new pane cannot even mention
them in a comment.

### 5.8 Settings, persistence, migrations

- Six settings stores exist by the repo's own count (`epic-46-the-file-is-the-setting.md:28-32`). The two that matter: `keeper.db` `settings` (`registry.rs:98-107`, `get_setting` consults the TOML layer stack first `:208-211`) and the six-tier TOML stack (`keeper-core/src/config/mod.rs:13-23`, later wins per key; a malformed layer is skipped whole, never fatal, `:47-55`). Every key is a `KeySpec` row in `config/keys.rs` with `Scope`, `Settable`, `Shape`; a test fails by name on any unclassified `get_setting`/`set_setting` (`:10-13`); `docs/settings-keys.md` is generated and pinned (`:46-49`). The rule that decides where a provider list may live: *"no settings-table key is about one folder"* (`keys.rs:36-44`) `[REPO]` G4 §10.
- Secrets never go in `settings`: `Platform::keychain_set/get/delete` (`ipc.rs:658-700`), `sync/<id>/credential` written *"never where the engine's own persistence can see it"* (`sync_ipc.rs:1573-1576`); `keeper-sync/src/credential.rs:1-24` — *"A fourth consumer gets a method here or it does not get the token"* (`:21`).
- Three databases: `keeper.db` (accounts, `settings`, `pins`, drafts; *"there is no token column"*, `registry.rs:1-10`), `archive.db`, `sync.db` (`keeper-sync/src/db.rs:1-13`, 8403 lines). **No schema-version constant, no numbered migrations**: WAL, idempotent `CREATE TABLE IF NOT EXISTS`, additive `ensure_*_column` helpers (`db.rs:4-8`, `:234-238`); content rewrites keyed on a `meta` marker (`:139-143`, `ensure_prune_default` `:242-295`); read `PRAGMA table_info` into a `Vec` and drop the statement before `ALTER` (`:346-350`). AD-139: a new record is **typed columns**, every later column nullable or defaulted (`db.rs:184-202`), and *"a knob ships with every surface that must write it, in the same story"* (`ARCHITECTURE-SCHEDULED-TASKS.md:272-274`). Newest-first is `id DESC`, never a timestamp (`:126-131`); batch writes answer per id (`set_tasks_enabled` `:3463-3510`); store functions are synchronous over `&Connection`, called through `spawn_blocking` (`sync_ipc.rs:369-370`).

### 5.9 The command palette contribution API

The catalog is authored once in Rust — `keeper_core::palette::palette_actions` — and consumed by the ⌘K
palette, the ⌘? cheat sheet, the native menu bar and the tray (`src/components/command-palette/actions.ts:4-6`).
One entry is an `action(id, title, category, keywords, shortcut_chip, requires_open_chat)` closure
(`palette.rs:679-708`); `toggle(...)` pairs share a `toggle_group` (`:442-456`); inline `PaletteActionVm` for
`requires_recording` (`:496-508`). `CATEGORY_ORDER` (`:878-900`) and `registry_sections(recording, notes)`
(`:928-996`) drop `NOTES | SESSIONS | TASKS` categories together when `notes` is false (`:932-937`); Rust tests
pin that ⌘8 has exactly one claimant (`:1744-1747`). Frontend: `paletteActionHandlers` map (`actions.ts:61`),
dispatch fails soft on an unknown id (`:212-223`); the drift guard is `actions.test.ts:120-144`. Cost of a new
surface: one `action(...)`, one category const in `CATEGORY_ORDER` (and the `registry_sections` filter if
gated), one handler, one pair of assertions.

### 5.10 Gates a new pane trips

`vitest` jsdom, `testTimeout: 15_000` (`vitest.config.ts:12-28`); `lefthook.yml` pre-commit biome + rustfmt +
secret scan, pre-push `tsc`, clippy (three non-shell crates where glib is absent), `bun run test`,
`bun run check:design`. Convention tests in `src/test/`: `no-user-agent-gating`, `command-registration`,
`config-command-platform`, `file-controlled-keys`, `capture-capability` / `print-capability` (new windows),
`file-scheme-registration` (new scheme), `task-host-tick` (AD-62 no second clock), `tray-notes-labels`.
The design gate `scripts/check-design.mjs` (513 lines): `raw-color`, `palette-default`, `no-gradient`,
`no-glass`, **`no-emoji` (U+1F300–1FAFF, U+2728, U+2600–26FF in any chrome; exempt only `lib/emoji/` and
`*reaction-*`, `:113-121`)**, `opacity-on-text`, `type-scale`, `seam-uncancelled`, `seam-doubled`, recomputed
WCAG `contrast`. Existing identity vocabularies that pass: the fixed lucide set in
`src/components/notes/space-icons.ts:1-28` (*"Nothing is ever removed from this map"*), and the eight-hue
account wheel (`registry.rs:26` `HUE_WHEEL_SIZE = 8`, `AccountVm.hueIndex`). **There is no user-chosen
colour anywhere** (`src/lib/account-hue.ts:2-9`: assigned, never chosen) `[REPO]` G6 §8.2; DESIGN.md:326-329:
*"conversational agents do not each get a mascot: a bot in keeper is a kept instrument, and it wears the
same cell"* `[REPO]` G6 §8.3.

**The shell does not build on Linux** (`command-registration.test.ts:23-27`, `lefthook.yml:30-38`,
`scripts/check-macos.sh:5-11`); `bun run check:rust:macos` on `hesperia` is the real workspace gate. That is
why invariants are TypeScript tests over Rust text, and why decisions belong in `keeper-core` (§6.9).

---

## 6. The drive as a filesystem

All of §6 is `[REPO]` G3 unless marked.

### 6.1 Vocabulary

| Word | Meaning | Type |
| --- | --- | --- |
| profile | one synced folder = `(id, name, local_path, remote_url, branch, …)`; the unit of sync, reservation and addressing (`profile id` + profile-relative subpath) | `keeper_sync::profile::SyncProfile`, `profile/mod.rs:758` |
| folder | a profile's `local_path` tree; also the config tier `<folder>/.keeper/keeper.toml` | `config/mod.rs:18-19` |
| vault | a **notes** vault, a subfolder inside a profile with no identity of its own — *"The profile id. A vault has no identity of its own (AD-54)"* | `notes_vault.rs:122-124`; `NotesConfig` `profile/mod.rs:268` |
| space | two things: a note space (`spaces/` in the vault, `default_spaces.rs:676`) and a session space (`<zone>/_spaces/<name>.md`, `sessions/spaces.rs:58`) | — |
| note | markdown in a vault with a ULID in frontmatter | `keeper_core::notes` |
| session | one folder in a sessions zone holding one LLM work session; shapes `Flat` / `Folder` | `SessionsConfig` `profile/mod.rs:607` |

**The path is the kind in exactly two folders**: `spaces/` mints a space, `templates/` a template
(`notes/mod.rs:41-45`, `notes/seed.rs:495-501`) — a tool-call writing `.md` into `spaces/` mints a rail row.

Root validation: on every create, update and load (`profile/mod.rs:1151-1154`); absolute (`:1158-1163`);
subpaths refuse leading `/` and `..` (`:1177-1185`); vault subfolder non-empty, relative (because `Path::join`
with an absolute right side discards the left, `:345-347`), no `..`, never `.obsidian` (`:336-388`); recordings
and sessions zones may not overlap the vault (`:530`, `:644-647`). The **live** vault is a registry entry —
`register_one` returns `None` when the root cannot be canonicalized (`notes_vault.rs:351-354`); a layer that
reads `profile.notes` instead claims writability that does not exist (`vault_and_scope`, §6.4).

### 6.2 Every containment guard, with its rule

| Guard | `file:line` | Rule |
| --- | --- | --- |
| `browse::resolve` | `keeper-sync/src/browse.rs:646` | lexical `plain_segments` **first and unconditionally**, then canonicalize **both** root and target, target must `starts_with` root; `Ok(None)` = nothing on disk, not a refusal (`:627-645`) |
| `browse::lexical_join` | `browse.rs:674` | join with no I/O, total over strings |
| `browse::plain_segments` | `browse.rs:707` | exactly one `Component::Normal` per `/`-segment; empty segment refused (`"a//b"` ≠ `"a/b"`, `:692-695`); `U+FFFD` anywhere refused (`:716-720`) |
| `files_write::resolve_existing` | `files_write.rs:1179` | `browse::resolve` with `Ok(None)` → `WriteRefusal::Missing` |
| `WriteScope::vault_relative` | `files_write.rs:425` | **component-wise** prefix, never string `starts_with` — `10-notes-archive` beside `10-notes` (`:421-424`) |
| `WriteScope::route` | `files_write.rs:518` | containment/existence before the vault question; then not-a-directory; then vault-root refusal (`:491-511`) |
| `WriteScope::workspace_position` | `files_write.rs:599` | `<zone>/active/<s>/workspace/**` and `archive/<yr>/<s>/workspace/**` refuse every write (AD-113); case-folded (`:617-624`) |
| `files_write::collides` | `files_write.rs:1153` | case-insensitive name collision — APFS/NTFS replace, do not create (`:1142-1152`) |
| `files_write::check_name` | `files_write.rs:1197` | empty → not-plain → >255 **bytes** → `RESERVED_NAMES = [".keeper", ".obsidian", ".git"]` (`:113`) |
| `file_serve::resolve_served_path` | `file_serve.rs:97` | profile id refused before any path work (`:31-35`) |
| `file_serve::is_inside` | `file_serve.rs:137-141` | canonicalize both, `starts_with`; any error → `false` |
| `notes_vault::contained` / `is_internal` / `is_refused_dir` | `keeper/src/notes_vault.rs:1830`, `:639-642`, `:1162-1164` | lexical; refuses `.keeper`/`.obsidian`/`.git` components; the index never descends into them |
| `Engine::materialize_request` | `engine.rs:10148` | `plain_segments` → `browse::resolve` → paused-profile → volume-ready |
| `Engine::path_durability` | `engine.rs:4639-4642` | absolute **and** no `ParentDir` **and** under recordings root |
| `export` destination | `export.rs:229-246` | must canonicalize; refuses a destination inside the source |
| `git::commit::index_key` | `git/commit.rs:484-496` | repository-relative, no `ParentDir` |
| `sessions::files::check_rel` / `check_dir` | `sessions/files.rs:319-348` | called twice because the slug fold *"is a namer"* (`:345-347`) |
| `sessions::files::check_deletable` / `check_rewritable` | `sessions/files.rs:384-392`, `:413-424` | `AGENTS.md`, `README.md`, `about.md` undeletable; `artifacts/**` not rewritable |

Symlinks: lexical guards cannot see a parent replaced by a symlink pointing out; `browse::resolve` refuses
after resolution (AD-59) (`engine.rs:10178-10184`, `files_write.rs:70-74`). macOS case-insensitivity is handled
in three places with the failure each prevents (`files_write.rs:1142-1152`, `:617-624`, `sessions/files.rs:449-458`),
and deliberately **not** on the vault subfolder (`:389-393`). **Why all of this lives in `keeper-sync`**:
*"The shell does not build on Linux, so a containment rule written there is a security rule proved on no
machine any of this is developed on"* (`files_write.rs:76-84`, `browse.rs:22-23`).

### 6.3 What the app already exposes over IPC

Profile-addressed (`sync_ipc.rs`), address = `(profile id, subpath)`: `sync_read_text` `:3699` (→ `TextFileVm`;
**a read calls `engine.note_use` `:3711`** — a read is a recorded use for the TTL), `sync_read_document` `:3897`,
`sync_write_entry` `:4208` (routes through `WriteScope::route` `:4229-4259`), `sync_create_entry` `:4732`
(**vault only**), `sync_delete_plan` `:4513`, `sync_delete_entries` `:4623` (trash, never `unlink`),
`sync_read_frontmatter` `:4342`, `sync_write_frontmatter` `:4400` (**`expect` compare-and-swap**),
`sync_materialize_entry` `:3514`, `sync_release_entry` `:3591`, `sync_pin_entry` `:3644`, `sync_export_entry`
`:3967`. The read/write asymmetry is stated at `:3672-3677`: *"a file outside a vault can be listed and viewed
but not written… so a single read-write command would have to refuse half of itself."* Listing is
`sync_browse` (`:2824-2843`), lazy one-directory-per-call, `LISTING_CAP = 1000` with truncation reported
(`browse.rs:101-109`), tier-0 exclusions invisible (`BUILTIN_EXCLUDES` `exclude.rs:44`).

Vault-addressed (`notes_ipc.rs`): bodies **stream, they do not return** (`:12-16`, `Channel<NoteBodyBatch>`
with `Reset` / `External` / `Diverged` / `Renamed` / `Gone`). Session-addressed (`sessions_ipc.rs`):
`sessions_file_delete` refuses `README.md`/`AGENTS.md`/`about.md` (`:3777-3782`).

What "editable text" means (`keeper_core::text_file`, `text_file.rs:1-53`): `TEXT_EDIT_MAX_BYTES = 1_000_000`
decimal; over the limit = truncated **and read-only**; binary = refusal never lossy (`:46-53`); exact bytes,
no normalisation (`:91-95`).

### 6.4 Two writers, made unrepresentable

There is no permission model; there is a **routing** model. `WriteScope` (`files_write.rs:365-376`) is built
per call from the vault the shell can actually reach; `WriteRoute<V>` (`:843`) carries the vault inside the
variant; `VaultPath(String)` (`:795`) has exactly one constructor; `UnmanagedPath` (`:819`) is *"Absolute and
already canonicalised… a `PathBuf` arriving at `write_unmanaged` from anywhere else would be a write to an
arbitrary location on the machine"*. *"Not a boolean, and not a comment: the mistake is not expressible"*
(`:62`). Disclosure obligation: `unmanaged_caveat` before the first keystroke (`:682-688`), both forms composed
in Rust (`:723-727`). Shell helpers: `vault_and_scope` (`sync_ipc.rs:4028-4046`, live registry vault plus the
sessions fence from the stored profile), `routable_profile` vs `writable_profile` (`:4105-4109`, `:4071-4087`)
split because the owner's `AGENTS.md` inside a profile but outside the vault was unreachable (`:4092-4098`).

### 6.5 Virtual and placeholder files — the sharpest trap for a `read_file` tool

FR-331: *"A virtual file's worktree bytes are exactly the committed pointer, and the path reads clean in
`git status`"* (`epic-56…:130`, `browse.rs:69-73`). **A naive read returns ~130 bytes of LFS pointer text.**
Honest size/oid: `stage::worktree_pointer` (`lfs/stage.rs:1044`), `indexed_pointer` (`:963`), `indexed_size`
(`:1157`). Policy: `.keepervirtual` at the repo root, gitignore dialect (`lfs/virtual_policy.rs:72`), four
ascending sources (`:44-49`); `resolve` does no I/O (`:27-34`); **patterns authorize hydration, only per-object
proof authorizes deletion** (`:15-23`, git-lfs#3092). Materialization is a verb somebody calls — never a read:
*"A `grep -r`, Spotlight, a backup agent, an antivirus scanner or a `du` walks the tree and hydrates
everything"* (`epic-56…:102-106`). **An LLM tool doing `read_file` over a subtree is exactly the `grep -r` in
that sentence** `[INFERENCE]` G3 §4. Doors: `sync_materialize_entry` (`id, subpath, keep_for_ms`) and
`keeper-syncd materialize`. `materialize` re-reads its target one instant before `rename(2)` (`lfs/stage.rs:1795-1814`);
`dehydrate` refuses five times (modified, open, remote-unproven, pinned, already-pointer; `:2070`). Two clocks:
`last_used_ms` and `synced_at_ms` (FR-341). macOS `SF_DATALESS` cannot be set from user space; Finder
integration is closed (AD-130). Also `stability::is_dataless` skips iCloud placeholders before opening
(`docs/sync.md:163-168`, `copy.rs:823-830`) `[REPO]` virtual-files research §8.

### 6.6 Atomic writes and the watcher race

Temp-and-rename under `.keeper.<ulid>.tmp`, a prefix tier 0 excludes, in **three** copies that cannot share a
crate: `notes_vault::atomic_write` (`notes_vault.rs:1750-1763`), `files_write::write_unmanaged`
(`files_write.rs:880-897`, *"a `kill -9` between write and rename leaves no torn file"*, no `mark_dirty` and
no reconciler `touch` by construction `:865-869`), `lfs::stage::materialize` (`:1771`). The exclusion:
`exclude.rs:77-78` — *"committing it would make the engine race with itself."*

**No advisory file lock on user content.** What exists: per-zone plan mutex (`sessions_exec.rs:4-10`), a
journal not a lock (`<zone>/.keeper/sessions-journal.json`), one operation per profile with
`db::requeue_running` (`http.rs:12-16`), `create_new` as the trash lock (`files_write.rs:987-992`), rename
refuses to overwrite (`notes_vault.rs:1807-1809`), `expect` CAS on frontmatter, `materialize`'s last-instant
re-read.

The watcher: one recursive `notify` watcher per profile (`watch.rs:3-15`), 500 ms debounce, 15-minute backstop
rescan; three mandatory companions (`:37-71`); **echo suppression deliberately not wired** (`:51-68`, *"`EchoSuppressor`
has no production registrar"*); keeper's own vault write is not hidden from the watcher — the echo is stopped
one layer up by comparing disk revision against the delivered one (`notes_vault.rs:1714-1723`). App and daemon
share no profile store and no data dir (`epic-56…:61`), so no cross-process lock exists; a write during a scan is
an ordinary watcher event and `git status` decides. The documented failure is starvation, not corruption: a
343-second status walk holding the profile reservation (`lfs/stage.rs:1753-1759`).

Never `unlink` (NFR-30): vault → `<vault>/.keeper/trash/<ulid>/<rel>` (`notes_vault.rs:1770-1790`); unmanaged →
`NSFileManager trashItemAtURL:` (safe objc2, `files_write.rs:1061-1071`) or freedesktop trash; no trash →
`WriteRefusal::NoSystemTrash`, file kept (`:161-171`); a directory is refused everywhere (`:66-69`).

### 6.7 `AGENTS.md` is keeper's own file in the sessions domain

`keeper_core::sessions::shape::AGENTS = "AGENTS.md"` (`sessions/shape.rs:26-27`) is the **shape predicate** —
its presence makes a session flat (`:84-85`). The bytes are a Rust const, ~100 lines (`template.rs:97-194`),
written *"because the reader is as often an agent as a person"* (`:91-96`), asserted by tests (`:1191-1193`).
It is undeletable (`sessions/files.rs:384-392`; deleting it *"flips a flat session back to folder-shaped"*,
`:354-363`), it travels (`pattern.rs:857-859`), a migration writes it before it moves the record
(`migrate.rs:31-40`), and a seed prompt exists (`template.rs:240-247`). `CLAUDE.md` appears once, as a test
fixture name (`session-templates.test.tsx:253`). **Constraint.** Creating an `AGENTS.md` anywhere under a
sessions root changes that folder's contract; a chat surface reading context files must reuse this
vocabulary or introduce a second one deliberately `[INFERENCE]` G2 §11.5, G3 §6.

Epic 38 ("your agent writes here too") is the **observer** side — an external agent writes with its own tools;
keeper notices, merges, marks provenance; AD-63 *"keeper adds no parallel history store"*; *"never lock a
note; watch it"* (`epic-38…:15-27`). `notes/merge.rs` named by story 38.2 was **not found** in the module
list (§16). `SyncLane::Worktree` forces `pushOnly` — *"A bidirectional bot lane would let an agent pull a
human's review edits back under itself — the airlock would leak"* (`profile/mod.rs:1170-1176`, `:1677-1679`).

### 6.8 What does not exist

No user-granted, revocable, per-subtree file capability for a third party (grep for `ollama|hermes|tool_call|
function_call` over `src-tauri/crates; src; docs` → no matches; `permission|scope|allow|consent|Policy` resolve
to TCC recording permissions, Tauri capability JSON, `IncognitoScope` and sync policies) `[REPO]` G3 §10.1.
Tauri capabilities are the real allowlist boundary: `capabilities/default.json:6-12` grants `core:default`,
`opener:default`, `deep-link:default`, `dialog:allow-open`, `notification:default`; the opener allows exactly
`http://*`, `https://*`, `mailto:*`, `tel:*` (`src/lib/notes/follow-link.ts:23-28`). The product invariant:
PRD FR-41 *"**No agent/AI send path in MVP**… Nothing in MVP may send without explicit user approval"*
(`prds/prd-keeper-2026-07-03/prd.md:480`), *"Two dispatch triggers only (composer send, Approval Pane
approve); both user-initiated"* (`addendum.md:50`); the AI-bot client is on the post-MVP list (`prd.md:518`,
`docs/constraints-and-limitations.md:47-48`).

### 6.9 Which crate can own a tool-call engine — the firewall tension

Dependency graph from the manifests: `keeper` → `keeper-core` + `keeper-sync`; `keeper-syncd` → `keeper-sync`
only; `keeper-core` and `keeper-sync` have no first-party deps (`keeper/Cargo.toml:24,81`,
`keeper-syncd/Cargo.toml:22`). Three CI gates enforce it (`package.json:25-27`). `keeper-sync/Cargo.toml:10-15`:
*"deliberately BOTH `tauri`-free and `keeper-core`-free."* The house rule, stated four times
(`text_file.rs:3-12`, `sync_ipc.rs:3685-3690`, `file_protocol.rs:14-27`, `files_write.rs:76-84`): decisions
live in `keeper-core`, the shell is a call site.

**Constraint.** The tool-call engine needs both `keeper-core`'s decision home and `ts-rs` export
(`keeper-core/Cargo.toml:16`) **and** `keeper-sync`'s containment functions and `http.rs` policy. Exactly one
of three is true and the epic must pick: (1) drive-touching half in `keeper-sync`, protocol half in
`keeper-core`, shell composes — the shape `sync_write_entry` already has (`sync_ipc.rs:4214-4218`); (2) whole
thing in `keeper-sync` with no `ts-rs` VMs there (`browse::BrowseEntry` is *"not a view model"* for this
reason, `browse.rs:111-118`); (3) a fifth crate with its own firewall row. Under (1) or (2) `keeper-syncd`
gets the engine for free, which the owner asked for (`epic-56…:21-22`); under a shell-side design it gets
nothing `[REPO]` G3 §9.3.

---

## 7. Permissions, consent and prompt injection

### 7.1 MCP `roots` — advisory metadata, enforcement on the server, consent on the client

Spec 2025-06-18 (still served; 2025-11-25 is current and the security page redirects there)
(<https://modelcontextprotocol.io/specification/2025-06-18/client/roots>,
<https://modelcontextprotocol.io/specification/2025-11-25/index>) `[SOURCE]` R6 §1.1–1.3:

- Clients declare `{"capabilities":{"roots":{"listChanged":true}}}`; servers send `roots/list`; result `{"roots":[{"uri":"file:///…","name":"…"}]}`; `uri` **MUST** be `file://`; change notification `notifications/roots/list_changed`; errors `-32601`, `-32603`.
- Clients MUST *"Only expose roots with appropriate permissions"*, *"Validate all root URIs to prevent path traversal"*; servers SHOULD *"Validate all paths against provided roots"*; clients SHOULD *"Prompt users for consent before exposing roots to servers"*.
- *"While MCP itself cannot enforce these security principles at the protocol level, implementors SHOULD…"*; tools *"represent arbitrary code execution"*; annotations are untrusted unless from a trusted server; *"there SHOULD always be a human in the loop with the ability to deny tool invocations"* (<https://modelcontextprotocol.io/specification/2025-11-25/server/tools>).
- The security best-practices page is scoped to OAuth authorization, not filesystem safety (<https://modelcontextprotocol.io/docs/2025-11-25/tutorials/security/security_best_practices>).

The reference filesystem server (`@modelcontextprotocol/server-filesystem` 0.6.3,
<https://raw.githubusercontent.com/modelcontextprotocol/servers/main/src/filesystem/>) `[SOURCE]` R6 §1.4–1.5:
tools `read_file` (deprecated), `read_text_file{path,head?,tail?}`, `read_media_file`, `read_multiple_files{paths[]}`,
`write_file{path,content}`, `edit_file{path,edits[{oldText,newText}],dryRun}` → unified diff, `create_directory`,
`list_directory`, `list_directory_with_sizes{path,sortBy}`, `directory_tree{path,excludePatterns}`,
`move_file{source,destination}`, `search_files{path,pattern,excludePatterns}` (filename glob, not content),
`get_file_info`, `list_allowed_directories`. **No delete, no grep, and no byte or line cap at all** —
`readFileContent` is a bare `fs.readFile`, `list_directory` a bare `readdir`, `directory_tree` unbounded
`[SOURCE]` R6 §4.1. When the client has `roots`, `listRoots()` **replaces** the CLI list; both the literal and
the `realpath` form of each allowed dir are stored (*"the macOS /tmp -> /private/tmp symlink issue"*).

### 7.2 The three CLIs' permission models

| Dimension | Claude Code (docs to v2.1.257) | Codex CLI (Apache-2.0) | Gemini CLI (Apache-2.0) |
| --- | --- | --- | --- |
| rule primitive | `Tool` / `Tool(specifier)` in `permissions.allow / ask / deny` | `sandbox_mode: read-only \| workspace-write \| danger-full-access` + `approval_policy: untrusted \| on-request \| never \| {granular}` | TOML `[[rule]]` with `toolName`, `commandPrefix`, `argsPattern`, `decision: allow \| deny \| ask_user`, `priority` |
| conflict resolution | *"deny, then ask, then allow. The first match… rule specificity doesn't change the order"* | sandbox boundary first, then approval policy | `final_priority = tier_base + (toml_priority / 1000)`, Default 1 … Admin 5; **Workspace tier non-functional** (issue #18186) |
| path syntax | gitignore semantics; anchors `//` (fs root), `~/`, `/` (**settings source, not root**), `./`; only `Edit(path)` and `Read(path)` are consulted; a `Read` deny also blocks Edit/Write | writable roots; `.git` (incl. `gitdir:` pointer target), `.agents`, `.codex` recursively read-only | `rootDirectory`; `.gitignore`/`.geminiignore` are filters, not boundaries |
| symlinks | allow needs **both** link and target to match; deny matches **either** | `[UNVERIFIED]` (§16) | `[UNVERIFIED]` |
| "always allow" persistence | Bash/WebFetch permanent per repo in `.claude/settings.local.json`; **file edits: until session end** | policy-driven | scoped to the current mode and all more permissive modes |
| trust gate | workspace trust dialog gates `allow` + `additionalDirectories`; never shown in `-p`/SDK | project `.codex/config.toml` loads only when trusted | Trusted Folders, off by default, `~/.gemini/trustedFolders.json`; untrusted ⇒ no workspace settings, `.env`, MCP, custom commands, auto-accept |
| non-interactive | deny/ask still apply | `read-only` recommended | `ask_user` → `deny` |
| OS enforcement | optional sandbox for Bash only; rules are in-process | OS sandbox is the default layer | in-process only |
| egress | `WebFetch(domain:*.example.com)` with a documented wildcard rule | `network_access` + `features.network_proxy.domains` allowlist, DNS-rebinding classification | not in the policy doc |

`[SOURCE]` R6 §2 (<https://code.claude.com/docs/en/permissions>, <https://code.claude.com/docs/en/settings>,
<https://learn.chatgpt.com/docs/agent-approvals-security>, <https://learn.chatgpt.com/docs/config-file/config-reference>,
`docs/reference/policy-engine.md` and `docs/cli/trusted-folders.md` on <https://github.com/google-gemini/gemini-cli>,
<https://github.com/google-gemini/gemini-cli/issues/18186>).

Two vendor caveats that decide the enforcement layer: *"Permission rules are enforced by Claude Code, not by
the model"*, and Read/Edit deny rules *"don't apply to arbitrary subprocesses that read or write files
indirectly"* `[SOURCE]` R6 §2.1. Hermes says the same of its own guards (§2.13).

### 7.3 Path safety — the canonical rules and the CVEs

1. Canonicalise, then prefix-check **on separator boundaries**: `normalizedPath === normalizedDir || normalizedPath.startsWith(normalizedDir + path.sep)` — the `+ path.sep` is the whole fix for **CVE-2025-53110** (naïve prefix: `/private/tmp/allow_dir` admits `/private/tmp/allow_dir_sensitive_credentials`; CVSS 7.3) `[SOURCE]` R6 §5.1, §5.3 (<https://www.wiz.io/vulnerability-database/cve/cve-2025-53110>, <https://cymulate.com/blog/cve-2025-53109-53110-escaperoute-anthropic/>). keeper's `vault_relative` already does the component-wise version (§6.2).
2. `realpath` and re-check — **CVE-2025-53109**, GHSA-q66q-fx2p-7w4m, *"Path validation bypass via symlink handling"*, CVSS 4.0 base 8.4, patched 0.6.3 / 2025.7.01 (<https://github.com/modelcontextprotocol/servers/security/advisories/GHSA-q66q-fx2p-7w4m>) `[SOURCE]` R6 §5.2.
3. Close TOCTOU with non-racy primitives: `flag: 'wx'` exclusive create and `rename(2)` onto the target, then `chmod` to restore mode bits; Claude Code re-verifies at open (<https://code.claude.com/docs/en/errors#refusing-after-a-symlink-changed>) `[SOURCE]` R6 §5.1, §5.5. keeper's `materialize` last-instant re-read is the same class (§6.6).
4. `lstat` the destination on move; hard links defeat path policy entirely; macOS `/var`, `/tmp`, `/etc` → `/private` (Gemini `canonicalizeMacosPath`); NFC/NFD normalisation handled only by the MCP server (`resolveUnicodeEquivalentPath`); **case-insensitivity handled by neither** (§16); Windows 8.3 short names — CWE-58 (<https://cwe.mitre.org/data/definitions/58.html>): `Read(**/credentials.json)` does not match `CREDEN~1.JSO` `[SOURCE]` R6 §5.1, §5.4.

### 7.4 Rate and size guards, side by side

| Guard | MCP fs 0.6.3 | Gemini CLI | Claude Code |
| --- | --- | --- | --- |
| max read size | **none** | `MAX_FILE_SIZE_MB = 20` → `FILE_TOO_LARGE` | token-budget; `PARTIAL view` |
| max lines | none | `DEFAULT_MAX_LINES_TEXT_FILE = 2000` | token page |
| max line length | none | `MAX_LINE_LENGTH_TEXT_FILE = 2000` + `'... [truncated]'` | explicit-limit read errors |
| files per call | unbounded | — | Glob capped at 100 |
| directory cap | **none** | no documented count cap | no directory tool; Bash byte-capped |
| tool output | none | `truncateToolOutputThreshold` 40 000 chars | Bash ≈30 000 inline, spill file truncated past 64 MiB |
| binary | base64 media / embedded resource | `Cannot display content of binary file` | images resized above 500 KB |

`[SOURCE]` R6 §4.1, §9 (`packages/core/src/utils/constants.ts` on <https://github.com/google-gemini/gemini-cli>).
keeper's own ceilings are `TEXT_EDIT_MAX_BYTES = 1_000_000` and `LISTING_CAP = 1000` (§6.3).

### 7.5 macOS TCC for files

Apple's panes (<https://support.apple.com/guide/mac-help/change-privacy-security-settings-mchl211c911f/mac>):
Files & Folders is request-driven (*"The listed apps have requested access"*); **Full Disk Access can only be
granted by the user adding the app in System Settings — there is no API to prompt for it** `[SOURCE]` R6 §7.
`NSDocumentsFolderUsageDescription`
(<https://developer.apple.com/documentation/bundleresources/information-property-list/nsdocumentsfolderusagedescription>):
implied consent when the user picks a file in an Open/Save panel, drags it, or opens it in Finder — *"any time
in the future"*; the first unimplied access prompts once; reset only with `tccutil reset
SystemPolicyDocumentsFolder <bundleID>`; siblings `NSDesktopFolderUsageDescription`, `NSDownloadsFolderUsageDescription`.
Claude Code documents TCC's per-responsible-process attribution: a background session host *"requests access
to protected folders such as `~/Desktop`, `~/Documents`, and `~/Downloads` separately from your terminal"*
`[SOURCE]` R6 §7. Security-scoped bookmark API details are §16.

### 7.6 Prompt injection through file contents

The lethal trifecta (Simon Willison, 2025-06-16, <https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/>):
private data + untrusted content + external communication; *"LLMs are unable to reliably distinguish the
importance of instructions based on where they came from"*; on guardrails, *"in web application security 95%
is very much a failing grade"*; the design-patterns paper's rule: *"once an LLM agent has ingested untrusted
input, it must be constrained so that it is impossible for that input to trigger any consequential actions."*
The file variant: a repo file (or a contributor's `AGENTS.md`, which is read **automatically, nearest-file-wins**,
§7.7) carries instructions that a later `write_file` or network tool obeys `[SOURCE]` R6 §6.1.

Deployed mitigations `[SOURCE]` R6 §6.2 (<https://code.claude.com/docs/en/security>): no auto-approve on write
(Claude Code edit approvals not persisted; Gemini `write_file`/`replace` *"Requires manual user approval"*);
provenance separation (Claude Code *"Web fetch uses a separate context window"*; Codex `web_search = "cached"`);
human-in-the-loop for egress; allowlisting over detection (Codex `network_proxy.domains`, `deny` wins,
`allow_local_binding = false`); OS enforcement below the model (*"even if a prompt injection bypasses Claude's
decision-making"*); second-model review (Codex `approvals_reviewer = "auto_review"`, fail-closed, policy at
<https://github.com/openai/codex/blob/main/codex-rs/core/src/guardian/policy.md>); trust gate before repo config
executes. **Output filtering is not a deployed control in any of the three** (§16).

Vision-specific: llama.cpp reads *"path to local file"* out of `image_url.url` — never populate image parts from
untrusted content `[SOURCE]` R3 §3.3, §10 (<https://github.com/ggml-org/llama.cpp>, `tools/server/README.md`).

### 7.7 Agent context files

| File | Discovery / merge `[SOURCE]` R6 §3 |
| --- | --- |
| `AGENTS.md` (Agentic AI Foundation / Linux Foundation, <https://agents.md/>) | *"just standard Markdown"*, no required fields, **no size guidance**; nearest file wins; *"explicit user chat prompts override everything"*; Gemini consumes it via `context.fileName`; Aider via `read:` |
| `CLAUDE.md` (<https://code.claude.com/docs/en/memory>) | managed → user `~/.claude/CLAUDE.md` → project `./CLAUDE.md` or `./.claude/CLAUDE.md` → `./CLAUDE.local.md`; **concatenated** root→cwd; `@path` imports, max 4 hops, external-path import prompts once; *"target under 200 lines"*; **reads `CLAUDE.md`, not `AGENTS.md`** — bridge is `@AGENTS.md` or a symlink |
| Cursor (<https://cursor.com/docs/rules>) | `.cursor/rules/*.mdc` (`.md` ignored), `alwaysApply` / `description` / `globs` truth table; *"under 500 lines"*; Team Rules take precedence; also reads `AGENTS.md` |
| `GEMINI.md` | `context.fileName` (list allowed); upward walk stops at boundary markers |
| OKF (Google Cloud, Apache-2.0, <https://raw.githubusercontent.com/GoogleCloudPlatform/knowledge-catalog/main/okf/SPEC.md>, <https://okf.md/>) | *not* an AGENTS.md competitor — a corpus of markdown-with-frontmatter concept documents (`type` the only required key; `index.md`/`log.md` reserved; unknown fields must not be rejected) |

On the wire, context files are text parts of a `system`/`developer` message — *"For developer messages, only
type `text` is supported"*; `prompt_cache_breakpoint` marks a reusable prefix `[SOURCE]` R3 §4.1.

### 7.8 keeper's own consent prior art

The recording-permission flow is the closest shape `[REPO]` G3 §5.2, G5 §2.1: a tri-state, never a bool
(`ScreenRecordingAccess`, `TccPermission`); one composite gate `RecordingPermissionVm.can_start`; **live
detection, never a cached grant** (a fresh sidecar child per probe, `client.ts:2044-2062`); safe default deny
(`DEFAULT_RECORDING_PERMISSION`, `use-recording-permission.ts:46-49`, pinned by test `:101`); re-detect on
`visibilitychange`/`focus`, coalesced at 500 ms (`:53-64`); request explicit and once, a prior denial shows no
prompt (`client.ts:2068-2070`); denied resolves to a deep link, not an error (`:26-34`); off-by-default is what
makes lazy permission true (`recording-mic.ts:6-9`); the install-day bug — an ungated poll posted the TCC prompt
every 3 s (`recording-source.ts:25-29`, test `:111-115`); exactly one observer (`:224-225`).
`NotificationPermission` is `Granted | Denied | Unknown` and *"Never re-prompts"* (`client.ts:2030`).
`sync_get_credential`'s doc records that per-command gating was **refused**: *"Every `#[tauri::command]` in the
invoke handler is equally reachable… `tauri.conf.json` sets `"csp": null`… A prompt here would be theatre, not
a control"* (`sync_ipc.rs:1602-1641`, DW-147) `[REPO]` G4 §2.3.

**Constraint.** Nothing in keeper today models "this bot may read this subtree"; the existing vocabulary is
routing (§6.4), disclosure (`unmanaged_caveat`), and TCC-shaped live detection. The industry models (§7.2) all
enforce in the host process, persist file-edit approvals for at most a session, and gate capability-granting
config behind a trust dialog `[INFERENCE]`.

---

## 8. Credentials, multi-tenant config and outbound HTTP today

All of §8 is `[REPO]` G4 unless marked.

### 8.1 The one multi-tenant record: `SyncProfile`

`keeper-sync/src/profile/mod.rs:755-992`. *"Nothing in the engine is global"* (`:3-6`); serde types that round-trip into `sync.db`, the app's `config.json` namespace and `keeper-syncd`'s TOML, so *"defaults live here and nowhere else"* (`:8-11`). `id` is an opaque ULID, *"what the journal, the keychain entry and the log spans key on"* (`:759-761`); sub-feature flags `notes`/`recordings`/`sessions: Option<…>` where *"`#[serde(default)]` here IS the migration"* (`:960-965`) and *"`None` is 'holds no recordings', never 'recordings with the defaults'"* (`:977`); *"the vault id is the profile id, so there is no picker, no second path to validate, no import and no migration"* (`:258-261`). Defaults are `pub const`s with reasons (`:129-224`); two out-of-range conventions coexist (floor `:146-150` vs refuse `:929-932`). Validation is one funnel (`:1151-1154`) invoked from `db::upsert_profile` (`db.rs:1574`). Store: `profiles(id, json, state, last_error, updated_ms)` (`db.rs:73-79`); `list_profiles` skips unreadable rows with a warn and `unreadable_profile_ids` recovers the fact (`:1594-1638`). **No duplicate-path guard** (`sync_ipc.rs:1294`). Layering: `in_force` applies the folder's TOML on read, `as_stored` strips it on write (`profile/folder.rs:780`, `:810`; why: `db.rs:1566-1572`); accepted keys are **derived from the type, never listed** (`:1275-1293`).

IPC: `sync_profiles` `:1228`, `sync_profile_save` `:1265` (re-reads after write), `sync_profile_remove` `:1312`, `sync_profile_set_enabled` `:1322`; `SyncProfileReq`'s rule *"Every `Option` here means… the caller did not express this field"* (`:655-659`), and `Option<Vec<T>>` because *"a bare `Vec` cannot tell 'the form did not show this' from 'the user emptied the list'"* (`:684-687`); the pinned/effective pair (`settle_ms` / `effective_settle_ms`, `:146-156`). Selection idioms: notes' active vault is Rust-owned (`registry.rs:1074-1092`); sessions' active root is frontend-only (`sessions-roots.ts:1-9`). The two recorded warnings against a third store shape: six settings stores already (`epic-46…:28-32`) and *"Config both surfaces must honour lives in the folder TOML tier"* (`epic-56…:61`, AD-132) `[REPO]` G2 §11.9.

### 8.2 Secrets

| Secret | Key | Written by |
| --- | --- | --- |
| Matrix session | `session/<account_id>` | `auth.rs:397-399`, `:657` |
| SDK store passphrase | `store_passphrase/<account_id>` | `auth.rs:406-408` |
| key-backup recovery key | `recovery_key/<account_id>` | `account.rs:4786-4788` |
| sync PAT | `sync/<profile_id>/credential` | `profile/mod.rs:1146-1149` `secret_key()` |

Port: `keeper_core::platform::Platform::keychain_set/get/delete` (`platform.rs:34,38,42`); `keeper-sync`'s parallel `secret_get/set/delete` with the clause *"MUST NOT write it anywhere the engine's own persistence can see it"* (`keeper-sync/src/platform.rs:296-303`). Desktop: `keyring` 3, service `dev.tgorka.keeper` (`ipc.rs:513-514`, `:658-708`); a process-wide `SecretCache` because macOS re-evaluates the keychain ACL on every read (`keeper-core/src/platform.rs:104-183`, `ipc.rs:622-639`). Linux: `keyring` `linux-native` keyutils — the default catalog is a mock that loses every write (`keeper/Cargo.toml:99-110`). iOS: `security-framework` direct (`ipc.rs:837-891`). `keeper-syncd`: env `KEEPER_SYNC_SECRET_<KEY>` then a `0600` file (`keeper-syncd/src/platform.rs:329-405`). The secret never crosses into a list VM (`sync_ipc.rs:1595-1598`, AD-34-7). Hygiene: `AccessToken` with a redacting `Debug` (`credential.rs:38-46`); `Authorization` marked sensitive (`lfs/batch.rs:422-429`); *"a message never carries a credential, a token, or file content"* (`error.rs:8-11`), enforced by a log-scan test; git helpers answer `Store`/`Erase` with no opinion (`git/fetch.rs:198-201`). **The pre-commit secret scan matches only PEM keys and Matrix `syt_` tokens** (`lefthook.yml:16-21`) — an `sk-…` key or a Hermes gateway key passes it untouched.

### 8.3 HTTP clients and the timeout doctrine

Nine call sites (`matrix-sdk` internal; Beeper `reqwest` 30 s at `auth/beeper.rs:89-92`; provisioning 30 s/120 s at `bridges/transport/provisioning.rs:113-116`; `gix` over `blocking-http-transport-reqwest-rust-tls`; the `git` binary; LFS batch client; LFS transfer client; `keeper-syncd` update `blocking` 120 s; `tauri-plugin-updater`) — `Engine` holds exactly two clients (`engine.rs:912-914`). Shared builder `client_with(user_agent, connect, read, pool_idle)` (`keeper-sync/src/http.rs:129-142`), UA `keeper-sync/<ver>` (`lib.rs:86`). The doctrine (`http.rs:1-40`): *"`reqwest` applies no timeout of its own"*; sixteen hours of silent `running` downloads observed 2026-08-18; `ClientBuilder::timeout` *"bounds a whole request, which is the wrong question"*; `read_timeout` *"bounds silence"*. Constants: `CONNECT_TIMEOUT` 15 s (`:52`), `READ_TIMEOUT` 60 s (`:62`), `TRANSFER_READ_TIMEOUT` 30 min (`:83`), `POOL_IDLE_TIMEOUT` 20 s (`:107`, *"This is a hypothesis"*). TLS: **rustls only**, one stack (`src-tauri/Cargo.toml:83-84`); applied as a rejection reason (`lfs/mod.rs:14-16`). Proxy: no explicit handling; production clients inherit reqwest's env-var detection, untested; the only `no_proxy()` is in a test (`durability_matrix_stages.rs:961-982`). Bounded body reading is a house rule: `bounded_body` (`lfs/batch.rs:733-739`, *"Never `reqwest::Response::text`"*). Chunked consumers use `Response::chunk()`, not `bytes_stream()`, because the crate *"deliberately does not depend on"* `futures_util::StreamExt` (`lfs/basic.rs:573-576`). **No SSE consumer, no `tokio-util`, no `Semaphore`, no `MAX_CONCURRENT`** (G4 §11). Concurrency is one operation per profile via `reserve` (`engine.rs:5273-5283`), `Reservation` released on `Drop`, refused as `SyncError::Busy`; *"the serialisation is the rate limit"* (`http.rs:12-15`).

### 8.4 Errors, backoff, the state vocabulary

`keeper-sync/src/backoff.rs` is the only backoff utility (base 2 s, max 600 s, full jitter, pure, SplitMix64 over the monotonic clock — deliberately no `rand`) (`:22-103`). `Retriability { Transient, Permanent, Deferred }` on the error, not the call site (`error.rs:19-34`); `SyncError` variants a provider client maps almost one to one — `Network`, `Auth` (Permanent; *"a retry storm against a git host is how an account gets rate-limited"*), `Forbidden` (separate because the remedy is the opposite), `Quota`, `Integrity`, `Busy`, `Refused(ContentRefusal)`, `Cancelled` (*"Never surfaced as a failure"*) (`error.rs:36-245`); `code()` kept separate from `Display` (`:330-356`). `CoreError` rolls per-module `thiserror` enums into one root mapped to `IpcError` **exactly once** in the shell (`keeper-core/src/error.rs:3-5`, `ipc.rs:1145-1330`); sync is outside that funnel (`sync_ipc.rs:892-903`). `IpcError { code, message, account_id, retriable }` (`vm.rs:1827-1841`), 24 `IpcErrorCode` variants named per remedy (`:156-275`); whether adding a variant is free is §16. State: `ConnectionStatus` `Online | Offline` (`vm.rs:338-344`); `ProfileState` with `Offline` and `MediaAbsent` explicitly not errors (`profile/mod.rs:99-115`); the tray sentence composed in Rust (`progress.rs:577-659`: *"'up to date' is only honest when nothing is waiting for ANY reason"*, *"a remaining time… is a guess dressed as a fact"*); `LayerFault` for config-file problems (`config/mod.rs:197-245`). Copy rule UX-DR10 *"sentence case, no exclamation marks, honest state narration, Glossary-capitalized nouns"* (`epics.md:212`), UX-DR11 persistent never toast-only for loss-risk states (`:213`).

### 8.5 The licence firewall and the dependency inventory

`src-tauri/deny.toml:7-23` allow-list: `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `MIT`, `MIT-0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `BSL-1.0`, `CC0-1.0`, `MPL-2.0`, `Unicode-3.0`, `OpenSSL`, `CDLA-Permissive-2.0`; `confidence-threshold = 0.8`; bans `git2` and `libgit2-sys` because they are GPL C mis-declared as MIT/Apache in metadata (`:31-47`) — *"cargo-deny reads the metadata, so the check would go green while GPLv2 C code linked into keeper"*; `allow-git = ["https://github.com/tgorka/gitoxide"]` only (`:52-60`). JS twin: `bun run check:licenses` (`scripts/check-js-licenses.ts`). Every new workspace dependency carries a justification paragraph (`Cargo.toml` passim). **Constraint.** `piper-rs` (declares MIT, statically compiles GPL-3.0 espeak-ng, §10.4) and `ollama-rs/tool-implementations` (GPL `calc`, `html2md`, §4.6) are exactly the mis-declared class `deny.toml` says the allow-list cannot catch `[INFERENCE]` from G4 §6.3, R4 §4.2, R2 §13.1.

Present in `[workspace.dependencies]` (`Cargo.toml:20-311`): `tauri` 2 + plugins (opener, deep-link, dialog, global-shortcut, notification, autostart, updater, process), `serde`, `serde_json`, `ts-rs` 12, `tokio` 1 (`macros, rt-multi-thread, sync, time, process, io-util, fs`), `tracing`, `thiserror` 2, `matrix-sdk` 0.18, `futures-util` 0.3, `url` 2, `percent-encoding`, `mime`, `mime_guess`, `reqwest` 0.13 (`json`, `rustls`), `dirs`, `fs4`, `chrono`, `keyring` 3, `security-framework` (iOS), `objc2` 0.6 + `objc2-web-kit`, `block2`, `objc2-foundation`, `objc2-user-notifications`, `ulid`, `rusqlite` 0.37 `bundled`, `gix` (tgorka fork), `notify` 8, `notify-debouncer-full`, `globset`, `sha2`, `hex`, `tempfile`, `clap` 4, `toml` 0.9, `http-body` 1, `bytes` 1, `base64` 0.22, `gix-quote`, `zip` 4, `quick-xml`, `flate2`. **Absent and relevant**: `futures` (only `futures-util`), `tokio-util`, `tokio-stream`, `async-trait`, any SSE crate, any image codec (`qrcode` drops the `image` renderers, `:62-67`), any audio crate, `uuid` (ids are `ulid`), `tauri-plugin-clipboard-manager`. Frontend (`package.json:39-90`): no markdown renderer outside CodeMirror/`@lezer/markdown`, no syntax highlighter beyond CodeMirror, no virtualised-list library, no `react-router`.

---

## 9. Egress

### 9.1 The stated policy

`docs/egress.md` (113 lines) `[REPO]` G3 §8.1: *"Client only, phones home to nothing it does not have to"* (`:3-4`); the complete app destination list is each account's Matrix homeserver, `api.beeper.com` only with a Beeper account, each sync profile's git remote **host**, the GitHub `latest.json` update endpoint, `*.githubusercontent.com` during an update (`:20-26`); daemon-only rows for the LFS endpoint and `api.github.com/.../releases/latest` (`:36-41`); *"a host, never a URL"* (`:56-67`); **the no-telemetry invariant is hard**: *"no telemetry, analytics, or crash reporting — and no opt-in scaffolding for any of it… any change that would add a new egress destination must be reflected here"* (`:96-100`); screen recording adds no egress, enforced by source scan (`:84-93`).

### 9.2 Every mechanical gate, and the exact tokens it bans

| Gate | Where | What it enforces |
| --- | --- | --- |
| derived live list | `keeper_core::egress::compute_egress` (`keeper-core/src/egress.rs:1-25`), command `egress_list` (`keeper/src/ipc.rs:10951`) | Settings → About renders the set computed from the accounts registry and the engine's profile rows; the closed `EgressKind` = `"homeserver" \| "beeper" \| "gitRemote" \| "update"` (`src/lib/ipc/gen/EgressKind.ts`); `remote_host` (`egress.rs:72-124`) discloses **the host and nothing else**, strips userinfo, handles scp-shorthand, refuses `file:` and a single-letter Windows drive authority `[REPO]` G2 §11.3, G3 §8.3 |
| pinned update constant | `EGRESS_UPDATE_ENDPOINT` (`egress.rs:36-37`) vs `tauri.conf.json:58`; test `egress_update_endpoint_matches_tauri_conf` (`keeper/src/ipc.rs:13914-13927`) | build fails the moment they diverge |
| Swift sidecar source scan | `keeper/src/zero_egress.rs:34-89` (`#[cfg(test)]`-only, **not** an app-wide network ban, `:1-14`) | fails on any of `urlsession`, `urlrequest`, `nwconnection`, `nwlistener`, `import network`, `http`, `socket`, `cfstream` in `tools/keeper-rec/Sources`, case-insensitively, tokens built by concatenation, *"must never pass vacuously"* (`:62-75`) |
| recording frontend source scan | `src/components/recording/zero-egress.test.ts:39-82` | scans `src/components/recording/*.tsx`, `src/components/layout/recording-pane.tsx`, `recording-summary-card.tsx`, **`src/components/command-palette/actions.ts`**, `src/lib/recording-*.ts`, `src/lib/stores/recording-*.ts`, `src/hooks/use-record*.ts`; fails on `fetch(`, `XMLHttpRequest`, `WebSocket`, `sendBeacon`, `EventSource`, `https?://`, `axios` (`:66-74`), Title-case `Upload` / `Share` / `Cloud` (`:79-81`), and `transc`+`ri` (`:82`) `[REPO]` G5 §3.2 |
| per-release egress diff | `.github/workflows/release.yml:163-205` | diffs `docs/egress.md` against the previous tag into the job summary `[REPO]` G3 §8.2 |
| note-embed rule | `keeper/src/note_protocol.rs:31-34` | *"Remote `http(s)` image sources in a note body are not fetched at all… a note that auto-fetches a URL an agent wrote is a tracking pixel"* |

### 9.3 Which gates a network-talking AI surface trips

1. **The derived list** — a Hermes or Ollama base URL is a new `EgressKind` variant plus a `compute_egress` input, reduced to a host through `remote_host`; the constant-sync rule (`egress.rs:30-37`) is the pattern. `docs/egress.md` must gain the row (`:96-100`) `[REPO]` G2 §11.3, G3 §8.3.
2. **The release diff note fires** — by design; that is the visibility `[REPO]` G3 §8.3.
3. **`actions.ts` is inside the recording zero-egress scan** — a palette entry for the chat surface whose title or keywords contain `Upload`, `Share`, `Cloud`, an `https://` literal, or the substring `transcri` turns that gate red; likewise any file at `src/lib/stores/recording-voice.ts` or `src/hooks/use-record*.ts` `[REPO]` G5 §3.2.
4. **The no-telemetry paragraph is absolute about "no opt-in scaffolding"** — a user-configured endpoint does not violate it; anything resembling a keeper-operated default endpoint would `[REPO]` G3 §8.3.
5. **Plain `http://127.0.0.1:11434` is HTTP, not HTTPS** — the only precedent for plain-http is the LFS endpoint check (`egress.md:38`); `tauri.conf.json:48` sets `"csp": null`, so no CSP constrains anything, but the webview makes no HTTP calls, all domain logic is Rust `[REPO]` G3 §8.3.
6. **Rendering model output that contains image URLs** meets the note-embed rule head-on `[REPO]` G3 §8.3.

### 9.4 A user-supplied base URL — SSRF and credential binding

OWASP SSRF Prevention Cheat Sheet (<https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html>) `[SOURCE]` R3 §10: *"Do not accept complete URLs from the user"*; disable redirect following (`reqwest::redirect::Policy::none()`); validate the **resolved** address, not the string, and treat DNS rebinding as the residual bypass; for arbitrary user URLs *"Allow lists cannot be used here… the block-list approach… is the best solution"*, covering private ranges, localhost, link-local, and hex/octal/dword encodings. Desktop twists: `http://` to a LAN address is legitimate (Ollama `http://localhost:11434/v1/`), so the rule cannot be "block private IPs" — it is **never send a stored credential to a host other than the one it was entered for**; scheme allowlist `http`/`https` only; build headers with `HeaderValue::from_str` (rejects `\n`, hyperium/http `src/header/value.rs`) and `set_sensitive`; keep the key out of the URL and logs (async-openai wraps it in `secrecy::SecretString`); **bound the response** — cap total bytes, per-frame size and `arguments` length, because the idle timeout does not catch a fast infinite stream; headers < 64 KiB total; *"Render nothing as trusted"*. keeper's `bounded_body` rule (§8.3) and `sensitive_auth` (§8.2) are the existing analogues.

---

## 10. Voice

### 10.1 Wake-word engines — code licence versus model licence

The structural finding of R4: *"for nearly every engine in this space the code licence and the model licence differ, and in three cases (openWakeWord, Piper, sherpa-onnx TTS) the permissive code licence is actively misleading"* `[SOURCE]` R4 header.

| Engine | Code | Models | Rust | Verdict `[SOURCE]` R4 §1 |
| --- | --- | --- | --- | --- |
| openWakeWord (<https://github.com/dscripka/openWakeWord>) | Apache-2.0 (LICENSE) | **CC BY-NC-SA 4.0 — non-commercial** (README `# License`: *"due to the inclusion of datasets with unknown or restrictive licensing"*); backbone is Google's Apache-2.0 `speech_embedding`, so a self-trained head on synthetic data is clean | none; C++ port `rhasspy/openWakeWord-cpp` MIT, last push 2024-07-09 | pre-trained models unusable in a shipped product; 16 kHz 16-bit mono, 80 ms / 1280-sample frames, threshold 0.5, `vad_threshold` via bundled Silero; speex NS Linux-only; design target FA < 0.5/h, FR < 5 % |
| sherpa-onnx KWS (<https://github.com/k2-fsa/sherpa-onnx>) | Apache-2.0; last push 2026-09-01 | zh+en 3M and en 3.3M zipformer models; **per-model dataset terms unverified** (§16) | `sherpa-rs` 0.6.8 MIT — **repo archived 2026-03-08** | open-vocabulary (*"any keywords without re-training"*), per-keyword `:boost` and `#threshold`; **ASR/KWS without TTS is GPL-free** (`sherpa-onnx-static-no-tts.pc.in`) |
| Porcupine (Picovoice) | Apache-2.0 bindings; engine under **proprietary Terms of Use** (2026-03-18) | `.ppn` under Terms | none on crates.io | revocable licence, mandatory AccessKey, §5.4.2 on-device usage telemetry pushed to Picovoice, §7.4 benchmark gag, no self-serve pricing |
| Snowboy | `NOASSERTION`, last push 2021-10-13 | — | — | dead |
| `GiviMAD/rustpotter` | Apache-2.0 | — | native Rust, last push 2023-10-01 | ~3 years stale |

### 10.2 VAD, inference runtime, capture

- **Silero VAD** is MIT for code and model (`snakers4/silero-vad` LICENSE; README *"no telemetry, no keys, no registration"*); < 1 ms per 30+ ms chunk on one CPU thread; ~2 MB; 8 kHz or 16 kHz only; **hard frame size 512 samples @ 16 kHz** (`utils_vad.py` L60–63) = 32 ms, which does not divide openWakeWord's 1280-sample frame — buffer separately `[SOURCE]` R4 §7.
- **`ort`** (pykeio, <https://github.com/pykeio/ort>) 2.0.0-rc.13, `MIT OR Apache-2.0`, ONNX Runtime 1.28, **no stable release**; default features include `download-binaries` and `copy-dylibs`, which drop a `libonnxruntime.dylib` into the bundle — one more Mach-O to sign; docs prefer static linking (`ORT_LIB_PATH`) or `load-dynamic`; `coreml` feature exists `[SOURCE]` R4 §2.1 (<https://ort.pyke.io/setup/linking>). Alternatives: `tract-onnx` 0.23.5 (pure Rust, no dylib, Sonos, in-tree Parakeet ASR example), `candle-core` 0.11.0 (ONNX second-class) `[SOURCE]` R4 §2.2.
- **`cpal`** 0.18.2, Apache-2.0, CoreAudio on macOS, stable device IDs; master README floor macOS 14.2; `realtime` feature not documented for macOS `[SOURCE]` R4 §6. Apple's Voice-Processing I/O AEC from `cpal` is `[UNVERIFIED]` (§16); the only maintained permissive Rust AEC is `webrtc-audio-processing` 2.1.0 (repo BSD-3-Clause, crates.io `non-standard`, LICENSE file 404 — §16) `[SOURCE]` R4 §7.

### 10.3 STT

| Engine | Code / model licence | Rust | Notes `[SOURCE]` R4 §3 |
| --- | --- | --- | --- |
| whisper.cpp + `whisper-rs` 0.16.0 | MIT / MIT (OpenAI weights); `whisper-rs` **Unlicense**; repo moved to `codeberg.org/tazz4843/whisper-rs` | binding to C++ | `coreml` + `metal` features; encoder on ANE *"more than x3 faster"*; memory `tiny` ~273 MB, `base` ~388 MB, `small` ~852 MB, `medium` ~2.1 GB, `large` ~3.9 GB; **first run compiles the `.mlmodelc` on-device** — a user-visible cold start; `coremltools` (Python + Xcode CLT) needed to produce it |
| faster-whisper / CTranslate2 | MIT / MIT | Python only | sidecar-only; or via `speaches` (MIT) over HTTP |
| sherpa-onnx ASR | Apache-2.0 | archived `sherpa-rs` | `osx-arm64-static` prebuilt; `-no-tts` avoids espeak-ng |
| Vosk | Apache-2.0 code; **models mix Apache, AGPL, LGPL-3.0, GPLv3, CC-BY-NC-SA** | `vosk` 0.3.1 MIT, 2024-10-27 | small-model WER 9.85 librispeech; big models up to 16 GB |
| Apple `SpeechAnalyzer` / `SpeechTranscriber` | system | `objc2-speech` 0.3.2 `Zlib OR Apache-2.0 OR MIT`, 2025-10-04 — macOS 26 symbols **unverified** (§16) | **macOS 26.0+ only**; legacy `SFSpeechRecognizer.requiresOnDeviceRecognition` is honoured only if `supportsOnDeviceRecognition` |
| Moonshine | MIT code; English and all streaming models MIT; legacy non-English non-streaming under a non-commercial licence | — | pushed 2026-08-31 |
| `thewh1teagle/vibe` | MIT | Tauri app | existence proof for whisper-in-Tauri |

### 10.4 TTS — the GPL landmines

- **Piper.** `rhasspy/piper` legacy is MIT; the current **`OHF-Voice/piper1-gpl`** is GPL-3.0 because it *"embeds espeak-ng"* (GPL-3.0). **`piper-rs` 0.2.0 declares `MIT` on crates.io and statically compiles GPL-3.0 espeak-ng** via `espeak-rs`; voices on HF are MIT `[SOURCE]` R4 §4.2 (<https://github.com/OHF-Voice/piper1-gpl>, <https://github.com/thewh1teagle/piper-rs>).
- **sherpa-onnx TTS** fetches espeak-ng in `cmake/espeak-ng-for-piper.cmake`; a TTS-enabled build is a GPL-3.0 binary despite the Apache-2.0 LICENSE; `-no-tts` is the way out `[SOURCE]` R4 §4.4.
- **Kokoro**: code Apache-2.0, `Kokoro-82M` weights Apache-2.0 (*"trained exclusively on permissive/non-copyrighted audio"*), but the reference G2P (`misaki`) pulls espeak-ng; `kokoro-tts` crate 0.3.3 Apache-2.0; `misaki`'s licence unverified (§16) `[SOURCE]` R4 §4.3.
- **KittenTTS** (`KittenML/KittenTTS`): Apache-2.0 code **and** weights, ONNX, < 25 MB — the cleanest small-TTS position found `[SOURCE]` R4 §4.5.
- **NeuTTS** (`neuphonic/neutts-air`): repo LICENSE is the NeuTTS Open License v1.0 with a **$5M annual-revenue threshold that reaches Outputs**; `neutts-air` weights Apache-2.0 but HF-gated; Python-only, GGUF + ONNX codec; watermark on by default and silently disabled under `uv sync`; Ryzen 9 HX 370 CPU 119 tok/s, iMac M4 CPU 111 tok/s (SLM only) `[SOURCE]` R4 §4.1.
- **Supertonic**: MIT code, `openrail` weights, **archival announced 2026-07-23** `[SOURCE]` R4 §4.5.
- **`AVSpeechSynthesizer`** (framework `AVFAudio`, macOS 10.14+): offline, zero bundle cost, `stopSpeaking(at: .word)` — *"word-boundary stop is exactly the primitive barge-in needs"*; `write(_:toBufferCallback:)` yields PCM; whether `objc2-av-foundation` exposes `AVFAudio` is §16; `/usr/bin/say` is the process-spawn fallback `[SOURCE]` R4 §4.6.
- OpenAI-compatible `/v1/audio/speech` servers: `speaches` (MIT; Piper inside, but as a separate process), `Kokoro-FastAPI` (Apache-2.0) `[SOURCE]` R4 §4.7.

### 10.5 Sidecar shipping cost on macOS

Tauri `externalBin` requires `-$TARGET_TRIPLE` naming, `shell:allow-execute` with `sidecar: true` in the capability file, and stdin/stdout as IPC (<https://v2.tauri.app/develop/sidecar/>). Signing (<https://v2.tauri.app/distribute/sign/macos/>): Apple Developer account, an Apple device for signing, notarisation via App Store Connect keys. **Every executable and dylib added to the bundle is another Mach-O that must be signed with the Developer ID and hardened runtime and survive notarisation**; a PyInstaller bundle is an interpreter plus dozens of extension `.so`s, each individually signed; sidecars ship only through the app updater `[SOURCE]` R4 §5. keeper already carries one sidecar: `keeper-rec`, a zero-dependency SwiftPM executable, macOS 13+, **Apple Silicon only, no universal build** (`scripts/build-keeper-rec.sh:20-24`), `externalBin` at `tauri.conf.json:65`, signed with `keeper-rec.entitlements` (`:74-77`), where an ad-hoc signature makes *"every rebuild look like a different app to TCC"* (`build-keeper-rec.sh:46-51`); one fresh child process per round-trip so TCC attributes the request to keeper (`ipc.rs:132-137`, AD-36); `PREFLIGHT_TIMEOUT` ≈ 5 s, `MIC_PROMPT_TIMEOUT` 120 s (`recorder.rs:100-119`) `[REPO]` G5 §1. D-1 in `docs/decisions.md` defers the paid Apple Developer Program `[REPO]` G1 §6.

### 10.6 Mic permission and always-on listening in keeper

- Only the sidecar touches a microphone; the webview never does (grep `getUserMedia|mediaDevices|AudioContext|MediaRecorder` over `src; src-tauri` → no matches) `[REPO]` G5 §2.3. Entitlements `com.apple.security.device.audio-input` and `.camera` (`keeper-rec.entitlements:20-23`; without them tccd *"refuses to even post the prompt"*, `:7-14`); `NSMicrophoneUsageDescription` = *"keeper records your microphone only when you enable it for a recording. Everything is saved locally on this Mac. Nothing uploads."* (`Info.plist:18-19`) `[REPO]` G5 §2.2. Apple: *"This key is required if your app uses APIs that access the device's microphone"* — the process is killed on first access without it `[SOURCE]` R4 §6.
- The sidecar **only writes files** (`AVAssetWriter`, `Capture.swift:928`); no PCM-out-over-stdio method exists in `main.swift:371-532`; no level-meter event `[REPO]` G5 §2.3. An audio-only capture target already exists (`AudioOnly`, `vm.rs:2772-2773`, mono 48 kHz AAC to `audio-####.m4a`) `[REPO]` G5 §1.4, §2.3.
- **Nothing in the codebase discusses always-on capture**; the tray forces itself visible during a live session because *"an invisible live recording is a bug"* (`tray.rs:19-25`), the macOS capture pill is never suppressed (`docs/recording.md:319-320`), the mic is requested lazily and ephemerally (`recording-mic.ts:6-13`). An always-on listener would be the first thing in keeper to hold a microphone outside a Recording Session `[REPO]` G5 §2.3. Whether the orange indicator persists for an idle CoreAudio stream, and what happens on mic contention, is `[UNVERIFIED]` (§16).
- **No STT, TTS or wake word exists** (grep `whisper|transcri|text-to-speech|speechsynth|vosk|coreml|wake.?word|SFSpeech|AVSpeech` over `src; src-tauri; tools` → zero hits beyond chat-export "transcript") and transcription is a repeatedly restated **won't**: `brainstorm-intent.md:60-64`, `.memlog.md:132-133`, `epic-16-context.md:22-23`, `epic-20-context.md:24-25`, `notes/recording_note.rs:14-18` (*"No transcription, no summarisation, no inference of any kind"*), `docs/egress.md:87-91` `[REPO]` G5 §3, §3.1. No playback of a raw byte buffer exists — every `<audio>` is fed by a URL behind one of the four custom schemes (`media-viewer.tsx:1-10`, `gallery-block.ts:378-385`) `[REPO]` G5 §4. Global hotkeys via `tauri-plugin-global-shortcut` (`hotkey.rs:1-6`) with a soft conflict list are the push-to-talk precedent `[REPO]` G5 §9.

### 10.7 Hermes' voice and wake-word pipeline

CLI: `voice.record_key` (`ctrl+b`) → beep → two-stage VAD (RMS 200 for ≥ 0.3 s, end after 3.0 s silence, hard stop at 15 s) → STT (`provider: local | groq | openai | mistral | xai`, fallback local > groq > openai, 26-phrase Whisper hallucination filter) → agent → sentence-by-sentence TTS (`edge | elevenlabs | openai | neutts | minimax | mistral | gemini | xai | kittentts | piper`); `barge_in: true`, `barge_in_threshold_multiplier: 3.0`, `barge_in_grace_seconds: 0.5`. Remote clients: `GET /api/audio/voice-config` hands the client the provider credential to call STT/TTS **directly** (`voice.client_direct: true`), with `POST /api/audio/transcribe` + a speech WebSocket as the relay; **neither route is on the API-server table** — they belong to the dashboard/TUI backend; `/v1/capabilities` reports `realtime_voice: false` `[SOURCE]` R1 §7.3. Wake word: `wake_word.provider: openwakeword | sherpa | porcupine`, `sensitivity`, `confirmation_frames`; with `capture: client` the desktop resamples to **16 kHz mono int16** and streams over the `wake.feed` RPC; detection emits `wake.detected`; on Apple Silicon Hermes selects tflite because openWakeWord's ONNX backend *"will arm, show as listening, and never fire"* (dscripka/openWakeWord#336) `[SOURCE]` R1 §7.4. Engine pins in `pyproject.toml` (`openwakeword==0.6.0`, `onnxruntime==1.27.0`, `sherpa-onnx==1.13.4`, `pvporcupine==4.0.3`, `faster-whisper==1.2.1`, `sounddevice==0.5.5`) carry **no licence statement in the Hermes repo** — moot for an HTTP client, mandatory before vendoring `[SOURCE]` R1 §7.5.

### 10.8 The barge-in state machine

Assembled from verified primitives, with `pipecat` (BSD-2-Clause) and `livekit/agents` (Apache-2.0) as Python specifications `[SOURCE]` R4 §7:

```
IDLE ──wake score>θ & VAD active──> LISTENING
LISTENING ──VAD silence > hangover──> THINKING ──> SPEAKING
SPEAKING ──VAD active on mic (post-AEC)──> BARGE_IN
BARGE_IN ──AVSpeechSynthesizer.stopSpeaking(at: .word)──> LISTENING
any ──error/timeout──> IDLE
```

What makes it *correct* is AEC — without it the VAD hears the app's own TTS. Push-to-talk vs wake word: the CPU cost of always-on KWS is negligible on M-series (Raspberry Pi 3 runs 15–20 openWakeWord models on one core; Silero < 1 ms/chunk), open-mic continuous ASR is the expensive mode (~388 MB resident for `base` plus continuous ANE work), and **the real asymmetry is TCC: always-on holds the microphone grant and the system indicator for the whole session** `[SOURCE]` R4 §8. No vendor publishes battery figures (§16).

---

## 11. Images and attachments

### 11.1 The no-base64-over-IPC rule

Stated four times, all normative `[REPO]` G5 §6.1: `keeper-core/src/media.rs:5-6` (*"never as base64/JSON/`Vec<u8>` over IPC"*), `vm.rs:1065-1068` (`keeper-media://`, AD-4), `keeper/src/note_protocol.rs:4-6` (AD-4, AD-58), `notes/vm.rs:790-792`. The one sanctioned inbound exception is the **outbound** pasted image as a raw binary IPC body (§11.2); the one sanctioned inline data-URI is the ≤ 64×64 app-picker icon (`vm.rs:2694-2698`). Attachments never cross IPC in either direction: *"the webview hands over a path Tauri's own drag-drop event or the file picker gave it, and Rust does the reading"* (`notes_vault.rs:1851-1853`, AD-58) `[REPO]` G3 §10.

### 11.2 Clipboard paste — the only proven route

`src/components/chat/composer.tsx:696-721` is the **only** clipboard read in the app: `clipboardData.items` → first `image/*` → `getAsFile()` → `arrayBuffer()` → `attachmentsStore.add({kind:"bytes", bytes, filename, mime, size})`; a non-image paste falls through to default text paste; tests at `composer.test.tsx:337-343`, `:358-362`. The bytes leave as `InvokeBody::Raw` with `accountId`/`roomId`/`filename`/`mime`/`caption` in percent-encoded request headers (`client.ts:1772-1777`, `ipc.rs:3938-3943`, *"~1× size, never base64/JSON"*) `[REPO]` G5 §5.2, G6 §1.5. The store models exactly two shapes, `path` and `bytes` (`attachments.ts:6-13`). Every clipboard **write** is `navigator.clipboard?.writeText` with a swallowed rejection (`files-pane.tsx:1374-1380`, `recording-row.tsx:195-202`) `[REPO]` G5 §5.1.

`notes_attachment_paste` (`notes_ipc.rs:3942-3967`) is registered (`lib.rs:1082`) and **refuses with `Unsupported`**: *"Reading an image off the system clipboard needs a clipboard backend — `tauri-plugin-clipboard-manager` or an equivalent — and this build links none… and the phase's dependency rules add no Rust crates"*; the phase-5 architecture assumed the plugin was adopted (`ARCHITECTURE-NOTES-PHASE5.md:273-276`) and it never was; nothing calls the command (`notes/vm.rs:798-802`). The capture window has no paste handling `[REPO]` G5 §5.3, G2 §11.4.

### 11.3 Drop, the media protocol, size rules

- **HTML5 drag-and-drop does not work in this shell**: wry's handler always claims the event (`tauri-runtime-wry-2.11.4/src/lib.rs:4862-4896`, `wry-0.55.1/src/wkwebview/drag_drop.rs:88-95`), so page `drop` never fires; the in-house replacement is `usePointerDrag` (`use-pointer-drag.ts:6-19`); the only OS-file drop target is `conversation-pane.tsx:807-824` via `getCurrentWebview().onDragDropEvent` (paths, Rust reads bytes) `[REPO]` G5 §7, G6 §7.2.
- Four custom protocol handlers registered in `lib.rs:222-270`: `keeper-media://` (SDK media cache, also iOS, `media_protocol.rs:46-72`, 200/206/416/404), `keeper-note://` (vault root, desktop), `keeper-recording://` (recordings root by session id), `keeper-file://` (a profile's `local_path`, `file_protocol.rs:57-125`). The house rule for a fifth handler (`file_protocol.rs:14-27`): *"There is no path arithmetic in this file and there must never be any"*; every refusal is one 404 (`:29-32`); the name test runs before sqlite (`:75-83`) `[REPO]` G5 §6.2.
- Size rules: `MAX_RANGE_CHUNK = 8 MiB` (`note_protocol.rs:61`); `X-Content-Type-Options: nosniff` on every response including 404s (`:283`, `:336-341` — without it an agent-authored note could become an HTML injection point); Content-Type from an extension allow-list, never sniffing (`:29-31`); remote `http(s)` sources not fetched (`:31-34`) `[REPO]` G5 §6.3. One extension vocabulary: `IMAGE_EXTENSIONS` = `png jpg jpeg gif webp avif bmp svg`, `VIDEO_EXTENSIONS`, `AUDIO_EXTENSIONS` (`recordings_fts.rs:1005-1016`), `kind_for_file_name` by extension alone; *"A second table anywhere is a table that drifts"* (`:1002-1004`); a TypeScript duplicate is forbidden (`document.rs:269-273`) `[REPO]` G5 §6.4. keeper generates no thumbnails when sending (`send.rs:351-353`) `[REPO]` G5 §6.5.

### 11.4 What the back ends and the industry accept

OpenAI (<https://developers.openai.com/api/docs/guides/images-vision.md>): PNG, JPEG, WEBP, non-animated GIF; up to 512 MB per request, 1,500 images, 30,000 patches per image (rejected, not resized); `detail` ∈ `low | high | original | auto`, and `low` *"does not always use fewer tokens than `high`"* `[SOURCE]` R3 §3.2. Ollama: `data:` only, bare string, SVG rejected (§3.6); vLLM ignores `image_url.detail`; llama.cpp accepts a **local file path** (§7.6) `[SOURCE]` R3 §3.3. Hermes accepts remote and `data:` on both APIs (§2.4). Open WebUI gates paste on `visionCapableModels` and badges the thumbnail when only some selected models see images; HEIC is transcoded (`MessageInput.svelte`) `[SOURCE]` R5 §9 (<https://github.com/open-webui/open-webui>). Claude Code inserts an `[Image #N]` chip; Codex takes `-i`/`--image` `[SOURCE]` R5 §9.

**Constraint.** Image mode has exactly one proven inbound route in keeper (webview paste → raw binary body) and no Rust-side clipboard read; the Rust side needs a new dependency, which is a licence-firewall and dependency-rules decision `[REPO]` G5 §5.3. Sending an image to Ollama means base64 **inside the JSON request body**, which is the inverse of the app's IPC rule and lives in Rust, not the webview `[INFERENCE]` from R2 §7 and G5 §6.1.

---

## 12. Chat UX prior art worth copying, and keeper's reusable components

### 12.1 Industry patterns `[SOURCE]` R5

- **Provider connections** (Open WebUI, <https://docs.openwebui.com/getting-started/quick-start/connect-a-provider/starting-with-openai-compatible>): a list of connections with URL, key, enable/disable toggle, model-id allowlist and a **Verify Connection** button whose false-negative is documented — *"Some providers do not implement the `/models` endpoint… chat completions will still work"*; OpenRouter *"returns thousands of models"* → allowlist + cache; duplicates refused with `Model ID is already added`. LibreChat `endpoints.custom[]` with `apiKey: "user_provided"`, `baseURL`, `models.fetch`, `titleModel`, `iconURL` `[SOURCE]` R5 §7.
- **Per-message metadata**: Open WebUI's ⓘ button tooltip is the pretty-printed `usage` object (`prompt_tokens`, `completion_tokens`, `eval_count`, `eval_duration`, `load_duration`, `prompt_eval_duration`, `total_duration`), persisted as a `usage` JSON column; the button is **`aria-hidden="true"`** — a defect to design against; LibreChat stores `model`, `endpoint`, `tokenCount`, `finish_reason`, `error`, `unfinished`, and `contextMeta {calibrationRatio, encoding}` `[SOURCE]` R5 §3.1–3.2. Reasoning blocks: tag-based detection, collapsible "Thinking", and on replay rebuilt per provider (Ollama → `<think>`, others omitted) because *"Chat Completions APIs reject assistant messages with unknown fields"* `[SOURCE]` R5 §3.3.
- **Streaming UI**: `done=False` rows and an `unfinished-assistant probe` index (Open WebUI); LibreChat writes `unfinished: !!text.length, error: true` on SSE error; **continue** as a first-class peer of **regenerate**; response auto-scroll as a user setting; Telegram Bot API 10.3 `MessageGenerationStopped` for user-initiated stop `[SOURCE]` R5 §6.
- **Pins**: Cherry Studio (AGPL — design reference only) keeps a server-authoritative `/pins?entityType=` with `groupKind: 'pinned' | 'provider'` and suppresses a pinned model from its provider group except while searching; LibreChat `modelSpecs` treats a curated list and a free picker as mutually exclusive; Open WebUI pins **chats**, not models `[SOURCE]` R5 §4.
- **A11y**: WCAG 2.2 SC 4.1.3; Open WebUI's transcript is `<ul role="log" aria-live="polite" aria-relevant="additions" aria-atomic="false">` (<https://www.w3.org/WAI/WCAG22/Understanding/status-messages.html>, <https://developer.mozilla.org/en-US/docs/Web/Accessibility/ARIA/Guides/Live_regions>) — *"Start with an empty live region, then… change the content"*; hover-revealed metadata needs a keyboard-reachable, labelled equivalent `[SOURCE]` R5 §10.
- **Licence audit of the prior art**: LibreChat MIT (package.json says ISC), opencode MIT, Gemini CLI and Codex Apache-2.0; **Chatbox GPL-3.0 and Cherry Studio AGPL-3.0 — not linkable**; Open WebUI carries a branding clause (≤ 50 users exemption) and is not OSI-approved; LobeChat bespoke `[SOURCE]` R5 §0.

### 12.2 keeper's components, one by one `[REPO]` G6 §1

| Component | Path | Reusable by a non-Matrix chat? |
| --- | --- | --- |
| `ConversationPane` | `layout/conversation-pane.tsx:498` | **No** — hardcodes `subscribeTimeline`, `roomsStore`, `outboxStore`, `incognito`, `markRoomRead`, `paginateBackwards` (`:58-92`); no windowing (`:1587-1618`); no day separators (grep → 0) |
| `toRenderedRows` | same file `:173-219` | pure, 47 lines, but typed over `TimelineItemVm[]` |
| `MessageBubble` | `chat/message-bubble.tsx:142` | **type-coupled only**: `item: MessageVm` is the generated Matrix VM; body is **plain text** (`:205`); everything else is opaque-key callbacks |
| `MessageActions` | `chat/message-actions.tsx:32` | **yes, fully generic** (*"holds no IPC or store knowledge"*, `:8-9`) |
| `ReactionPopover` | `chat/reaction-popover.tsx:28` | yes; `CURATED_EMOJI` fixed const, no picker dependency |
| `HistoryBoundary` | `chat/history-boundary.tsx:32` | **yes** — `idle \| paginating \| offline \| atStart \| error`, *"it stops, never spins forever"* |
| `Composer` | `chat/composer.tsx:146` | draft persistence Matrix-keyed (`accountId`, `roomId` → `saveDraft`); autogrow 8 lines, Enter sends, `↑` edits last own, paste handler — otherwise generic |
| `UndoSendPill`, `RoomAvatar`, `RedactedStub`, `UtdStub`, `MediaAttachment` / `MediaPreviewOverlay` | `chat/*` | **No** — Matrix by definition |
| `useWindowedRows` | `ui/window-list.tsx:235` | yes (§5.4) |
| `subscribe<TBatch>` / `TimelineOp` | `client.ts:1103-1115`, `gen/TimelineOp.ts` | generic index-based diff stream; *"never re-sorts, filters, or re-indexes"* |

There is **no markdown renderer for a message body** and no code-block/syntax highlighting even in the note renderer (`notes/editor/live-preview.ts:1-21`); the single markdown renderer is CodeMirror-based `[REPO]` G6 §0. There is **no per-message metadata toggle** anywhere (grep `Show (details|source|raw|metadata|debug)|developer mode` → 0), and the timeline VM forbids raw JSON crossing IPC; progressive-disclosure precedents are the click-to-reveal popover, `FavoritesSection`'s persisted `aria-expanded`, `list-fold.tsx`'s *"Show all N"*, and `aria-pressed` chips (`note-filter-bar.tsx:22-27`) `[REPO]` G6 §2. Empty-state shape: a `max-w-[36ch]` sentence plus **exactly one** `Button variant="outline" size="sm"` (`notes-empty-state.tsx:69-91`, `recordings-empty-state.tsx:41-57`); five exemplar strings of the house voice at G6 §10.2 (`"Queued — sends when you're back online"`, `"Recorded locally. Nothing uploads."`) `[REPO]` G6 §10.

---

## 13. Sessions, archives, history and pinning prior art

### 13.1 keeper's own `[REPO]` G6 §3, §7

- **Phase 7 sessions**: files are the truth — *"Nothing here is stored state, so nothing here can disagree with a Finder edit for longer than one rescan (AD-110)"* (`sessions/model.rs:1-6`); layout constants `ACTIVE_DIR = active`, `ARCHIVE_DIR = archive` (filed by close year), `TEMPLATE_DIR = _template`, `WORKSPACE_DIR = workspace` (unversioned, read-only to keeper), `ARTIFACTS_DIR = artifacts`, `README = README.md` (`:12-25`); `SessionStatus` is location (`:45-55`); lineage `KEY_CONTINUES = "session-continues"`, `KEY_CONTINUED_BY = "session-continued-by"` under `keeper:` in README frontmatter as a flow list (`:27-43`), read by `lineage(fm)` (`:222-258`); two freshness signals `workspaceMs` / `recordMs`, never one (`:125-156`); `## Log` parsed as `### YYYY-MM-DD[ — title]` headings, file order newest last (`:171-220`).
- VMs: `SessionRowVm { id, path, title, status, archivedYear, workspaceMs, recordMs, lastLogDate, lastLogLine, snippet, tags, pinned, unread, origin(local|agent|remote), headRev, conflict, lineage }` (`sessions/vm.rs:34-75`); `SessionRootVm` (`:13-28`); `SessionRefVm` (`:82-87`); `SessionPatternVm { id, kind, label, detail, mtimeMs, copies[], skips[] }` (`:95-114`); `SessionLogEntryVm { date, title, body }` (`:145-152`).
- IPC (`sessions_ipc.rs`): `sessions_roots` `:36`, `sessions_list` `:52` (rows arrive whole, no windowing — AD-114), `sessions_rescan` `:85`, `sessions_detail` `:114`, `sessions_patterns` `:757`, `sessions_create(root_id, title, pattern_id)` `:851` — **the continue path is one argument, not a second verb** (`:835-848`; `continues`/`continued-by` written into BOTH READMEs), `sessions_log_today` `:1180`, `sessions_set_pinned` `:1590`, `sessions_archive(…, promotes)` `:1646`, `sessions_unarchive` `:1742`, `sessions_delete` `:1707` (to `.keeper/trash`), `sessions_search` + `_cancel` `:4621`/`:4672` (streamed, `MAX_HITS = 500`, order is the session's then the file's, never relevance). Store `sessions-list.ts:1-9`, filter haystack `[title, path, snippet, lastLogLine, ...tags]` (`:146-149`), `isStale` 14 days (`:111-120`). "New like this" opens the board's ONE create row (`session-actions.tsx:103-108`); archive/delete confirmations quote the zone's checklist verbatim (`:52-66`); lineage chips render inert (`session-detail.tsx:485-495`). UX-DR85–92 (`EXPERIENCE-SESSIONS.md:177-213`); UX-DR92: *"A user who knows notes already knows sessions… Divergence in look or behaviour is a defect, not a choice."*
- **Chat archive** is a separate primary view over the same list pane with `archive_room`/`unarchive_room` (`client.ts:2847-2861`) and a dedicated inbox channel (`:1161`) `[REPO]` G2 §11.10. **Recordings archive** (`recordings-pane.tsx:10-42`) is the house doctrine for a browse surface: search first, 200 ms debounce, monotonic `seqRef`, windowed rows keyed by id, count line from the backend total (`count-label.ts:4-18`), `RecordingFilterVm { query, tags, participant, startTs, endTs, durability, profileId, limit ≤ 200 }` `[REPO]` G6 §4.
- **Pins**: three models ship — chat pins in `keeper.db` `pins(account_id, room_id, sort_order)` with hand order, `set_pin` / `reorder_pins` in ONE `BEGIN IMMEDIATE` transaction (`registry.rs:241-296`), IPC `pin_room` `:10680`, `reorder_pins(order: Vec<PinRef>)` `:10716`; note pin = frontmatter `pinned: true`; session pin = README frontmatter; drag-to-reorder via `usePointerDrag` in `pins-strip.tsx` (`LIFT_DRAG_TOLERANCE_PX = 10`), with `Move up` / `Move down` on the phone (UX-DR28) `[REPO]` G6 §7.

### 13.2 Industry schemas `[SOURCE]` R3 §8, R5 §5

| Source | Fields worth knowing |
| --- | --- |
| LibreChat `message.ts` (MIT) | `messageId`, `conversationId`, **`parentMessageId`** (a tree, not a list), `model`, `endpoint`, `sender`, `isCreatedByUser`, `text` (flattened for search), `content` (structured parts), `attachments`, `tokenCount`, `summaryTokenCount`, `summary`, `contextMeta{calibrationRatio, encoding}`, `finish_reason`, **`error` and `unfinished` as different booleans**, `thread_id`, `feedback{rating, tag, text}`, UI-only fields labelled as such, `expiredAt` |
| LibreChat `convo.ts` | `conversationId`, `title` (default `'New Chat'`), `messages[]`, `isTemporary`, `agent_id`, `subagentThread{rootConversationId, parentConversationId, parentMessageId, parentToolCallId, subagentType, depth}` |
| Open WebUI `chats.py` / `chat_messages.py` | `Chat{id, user_id, title, chat(JSON), share_id, archived, pinned, folder_id, summary, current_message_id, last_read_at}`; `ChatMessage{done, status_history, error, usage, content, output}`; search prefixes `tag:`, `folder:`, `pinned:`, `archived:`, `shared:`; unread sort excludes chats with an unfinished assistant row |
| Codex rollout JSONL (Apache-2.0, `rollout_payload.rs`) | tagged records `session_meta \| response_item \| compacted \| turn_context \| token_usage_record \| world_state \| event_msg …`; provider-shaped `ResponseItem` stored **verbatim** with harness metadata separate; `compacted{message, replacement_history, window_id, previous_window_id, compaction_response_id}` keeps the summary **and** what it replaced |

Synthesis from R3 §8.3: per assistant message store `role`, structured `content`, `tool_calls[]{id, name, arguments_raw, arguments_parsed?}`, `finish_reason`, `model` **as returned**, `usage{prompt, completion, total, cached, reasoning}`, `provider_request_id`, created / first-token / last-token timestamps, `error` / `truncated` flags, and the request-shaping `turn_context` separately. Auto-titling: LibreChat `titleModel`/`titleTiming`, Open WebUI `TASK_MODEL` with a published prompt (*"2-4 words… single, raw JSON object"*) and a 1000-output-token footgun `[SOURCE]` R5 §5.2. Archive vs delete vs fork: Codex `archive`/`unarchive` (*"clean up the session picker without deleting the transcript"*), `delete --force` only with a UUID, `resume --last` scoped to cwd, `fork`; LibreChat's three fork scopes; Open WebUI fork keeps folder and pinned state and is unavailable while streaming `[SOURCE]` R5 §5.3. Context budget strategies that ship: Codex compaction as a lifecycle with persisted windows and a ×4 timeout for `/responses/compact`; llama.cpp `--context-shift` **disabled by default** (over-fill errors, never silently forgets) `[SOURCE]` R3 §4.2.

---

## 14. Slash commands

### 14.1 The four mechanisms `[SOURCE]` R5 §1

| | Telegram Bot API 10.3 | Claude Code (≤ v2.1.246) | opencode / Gemini CLI / Codex CLI | Open WebUI |
| --- | --- | --- | --- | --- |
| defined where | server-side `setMyCommands` (≤ 100, 1–32 chars, `[a-z0-9_]`), 7 scopes × `language_code`, **ordered fallback list** | `.claude/skills/<n>/SKILL.md` = `.claude/commands/<n>.md` (merged); enterprise > personal > project; plugin `/<plugin>:<name>`; nested dirs `/apps/web:deploy` | opencode `.opencode/commands/*.md` (file name = command); Gemini **TOML** `.gemini/commands/**.toml` (`git/commit.toml` → `/git:commit`, **project wins**); Codex prompts `~/.codex/prompts/*.md` (deprecated) + `.agents/skills/**/SKILL.md` invoked with **`$`** | DB row in Workspace → Prompts; `/` menu merges built-ins + Prompts + Skills |
| client vs server | **client** renders the menu and sends **ordinary text** with a `bot_command` entity; *"Scoping is presentation, never authorisation"* — the bot must validate | client-side expansion into the prompt | client-side; Gemini shell-escapes `{{args}}` inside `!{}` and confirms the resolved command | server-side Prompt fires; `/compact`, `/status`, `/fork` act on the chat, not the model |
| arguments | free text | `$ARGUMENTS`, `$ARGUMENTS[N]`, `$N`, `$name`; shell-style quoting; no recursive expansion; `\$1` escape | `$1..$9` / `$ARGUMENTS` / `KEY=value` (Codex, `$$` escape); `{{args}}` (Gemini) | typed variable **form** before send |
| unknown command | delivered as plain text | *"`Enter` submits your text as typed and reports Unknown command"* — no guessing after a typo | `[UNVERIFIED]` (§16) | built-in wins; `Model not found: <id>` |

Trigger characters converged on **one per referent class**: Open WebUI `/` commands+prompts, `@` models+context, `$` skills, `#` files, `:` emoji; Claude Code `/` commands, `@` file paths; Codex `$` skills in the CLI, `@` in ChatGPT `[SOURCE]` R5 §2. Escaping a literal leading `/` is documented nowhere (§16).

### 14.2 Hermes: one server-side registry, enumerable

`COMMAND_REGISTRY` in `hermes_cli/commands.py` drives both the CLI and the messaging gateway; installed skills become dynamic `/<skill-name>` commands. **`commands.catalog`** (TUI-gateway RPC, `tui_gateway/methods_tools.py`) returns `pairs [["/name","description"]]`, `canon` (alias → canonical), `commands` (`"/name" → {argument_mode, desktop}`), `categories`; companions `complete.slash`, `command.resolve`, `command.dispatch`, `slash.exec`; both include built-ins, user `quick_commands` and skill-derived commands. Resolution is case-insensitive prefix, full names and aliases beat prefixes. Permissions: admins get everything, users `user_allowed_commands` plus the floor `/help` and `/whoami`. **This catalog is not on the API-server route table** — reaching it needs the TUI-gateway WebSocket (§2.10) `[SOURCE]` R1 §6. The command set is ~100 entries across Session / Configuration / Tools & Skills / Info / Messaging-only / CLI-only, with `/new`, `/reset`, `/compress`, `/model`, `/voice`, `/wake`, `/approvals`, `/tools`, `/skills`, `/memory` among them `[SOURCE]` R1 §6.3.

### 14.3 keeper: absent in chat, present in the note editor

No slash-command parser exists (grep → only path-prefix checks); `composer.tsx` never inspects the first character — `/me` is sent literally `[REPO]` G6 §6.1, G2 §11.8. The note editor's `slash-menu.ts` is the deliberate narrow precedent: `OPEN_SLASH = /^\/\w*$/` (`:22`), triggers only at the start of an empty line, a closed set of ten literal insertions (`:58-96`), `text` computed at accept time (`:112-113`), the `from` anchor **after** the slash because a `/` inside cmdk fuzzy matching filtered everything out (`:127-133`), a CodeMirror `CompletionSource` bound to no `<textarea>`; *"Offering a template here that silently did nothing would be worse than not offering it"* (`:10`) `[REPO]` G6 §6.2. The nearest "type a verb" surface is the ⌘K palette with its `>` action mode (`command-palette.tsx:44-49`), Rust-ranked (`palette.rs:289-408`) `[REPO]` G6 §5. **Constraint.** A keeper `/` menu over Hermes is client-side presentation of a server-side registry; over Ollama there is no registry at all, so every command is keeper's own `[INFERENCE]` from R1 §6, R2 §2.

---

## 15. Risk register

| # | Trap | Evidence | Observable symptom |
| --- | --- | --- | --- |
| 1 | Sending `model` to Hermes without `provider` | §2.4, R1 §2.4 | the profile's default model answers; the picked model is silently ignored |
| 2 | Expecting Hermes to return a pending `tool_call` | §2.6 | keeper's drive tools never fire; tool UI shows only replayed `completed` calls |
| 3 | Looking for a bot roster on `:8642` | §2.11 | `/v1/models` returns one alias; no profiles endpoint (404) |
| 4 | Reusing the default `API_SERVER_KEY` on `/p/<bot>/` | §2.15 | 401 on every named bot since July 2026; 404 on a single-profile gateway |
| 5 | Pinning a Hermes session id | §2.12 | id drifts on compression; history "disappears" into `#2` |
| 6 | Treating Ollama HTTP 200 as success | §3.9 | a stream ends with `{"error":…}` after partial text; the answer is a fragment |
| 7 | Treating `done_reason: ""` as `stop` | §3.2 | a connection drop is rendered as a finished answer |
| 8 | Setting `num_ctx` over `/v1` on Ollama | §3.5 | agent prompts truncated at the 4k VRAM-tier default |
| 9 | Forgetting `think: false` on a thinking model | §3.4 | reasoning text appears in the answer (old server) or costs tokens silently |
| 10 | Deserialising native Ollama `arguments` as a string | §3.2, §4.2 | serde error on the first tool call |
| 11 | Tunnelling Ollama without rewriting `Host` | §3.8 | 403 with no JSON body, mistaken for auth |
| 12 | Assuming a remote Ollama is credentialed | §3.8 | an `0.0.0.0` bind exposes `DELETE /api/delete` to the LAN |
| 13 | SSE reassembly: `continue` on `finish_reason`; parsing `arguments` per fragment; keying calls by array position; synthesising a completion on EOF without `[DONE]` | §4.3 (a)(b)(d)(f) | a tool call in the last chunk is lost; `{"ci` is not JSON; two parallel calls garble into one; a truncated answer is committed as complete |
| 14 | A total `reqwest` `timeout` on a streaming call | §4.5, §8.3 | long healthy generations die at the deadline; `http.rs` warns identically |
| 15 | Forwarding SSE through `mpsc` without `select!` on `tx.closed()` | §4.5 | one leaked socket per cancelled turn |
| 16 | Retrying a partial stream by splicing | §4.5 | duplicated or contradictory text; lost usage |
| 17 | Assuming `/api/tags` carries `capabilities` | §3.3 | empty capability list on an older daemon; vision/tools hidden |
| 18 | Enabling `ollama-rs/tool-implementations` or adding `piper-rs` | §4.6, §10.4, §8.5 | `cargo deny` passes; a GPL-3.0 binary ships |
| 19 | A second `.invoke_handler(`; the new capability flag answered `false` in `dev/mock-shell.ts`; a stream-only pane with no mock `HANDLERS` entry | §5.5, §5.6 | every earlier command answers `Command <name> not found` (shipped v0.4.0–v0.4.2); the surface is invisible under `bun run dev` with typecheck green; the empty state shows forever |
| 20 | A palette entry mentioning `Upload`/`Share`/`Cloud`/`transcri`/`https://` | §9.3 | the recording zero-egress test goes red |
| 21 | An emoji or a user-chosen colour on a bot identity | §5.10 | `check-design.mjs` `no-emoji` / `raw-color` / `palette-default` fails; DESIGN.md forbids mascots |
| 22 | `read_file` walking a subtree | §6.5 | 130-byte pointer text returned as content; or mass hydration and `note_use` keeping everything materialized |
| 23 | Writing `.md` into `spaces/` or `AGENTS.md` under a sessions root | §6.1, §6.7 | a rail row is minted; a session's shape flips |
| 24 | String-prefix containment | §6.2, §7.3 | `10-notes-archive` escapes `10-notes` (CVE-2025-53110 class) |
| 25 | Containment written in the shell crate | §6.2, §6.9 | a security rule proved on no Linux dev box |
| 26 | Relying on the system prompt to stop file-borne instructions | §7.6 | prompt injection via a contributor's `AGENTS.md` |
| 27 | Populating `image_url` from a tool result | §7.6, §11.4 | local-file exfiltration on llama.cpp-class servers |
| 28 | Storing the API key in `settings` or `sync.db` | §8.2 | violates the platform port contract; leaks into a config file |
| 29 | Trusting the pre-commit secret scan for `sk-…` keys | §8.2 | a committed key passes the hook |
| 30 | Adding an egress host by hand instead of through `compute_egress` | §9.2–9.3 | Settings → About lies; the release diff misses it |
| 31 | An always-on mic via the sidecar | §10.6 | first always-on listener in the app; the orange indicator for the whole session; sidecar has no PCM path |
| 32 | Bundling `ort` with `download-binaries`; openWakeWord pre-trained models | §10.2, §10.1 | an unsigned `libonnxruntime.dylib` fails notarisation; CC BY-NC-SA in a shipped product |
| 33 | A second clock for the token cursor | §5.10 | AD-62 `task-host-tick` test |
| 34 | A hover-only metadata button | §12.1 | unreachable by assistive technology (Open WebUI's `aria-hidden` defect) |

---

## 16. Open questions and `[UNVERIFIED]` inventory

Each item names what was looked for and what was tried. None is filled by inference.

1. **Hermes `/v1/capabilities` complete JSON** — §2.9 is a subset assembled from docs plus test assertions; `_handle_capabilities` not read (R1 §11.2). Also unverified on the API server: session-row field names on `/api/sessions` (only the message envelope is asserted, R1 §11.3); `POST /v1/runs/{id}/approval` body key (values `once|session|always|deny` known, R1 §11.4); full `POST /v1/runs` schema (R1 §11.5); `/api/jobs` payload (R1 §11.6); `POST /api/sessions/{id}/model` schema (R1 §11.7); `/api/audio/voice-config` and `/api/audio/transcribe` schemas and owning server (R1 §11.8); how a third party obtains a `HermesRoom` grant (R1 §11.9); `hermes serve` vs `hermes dashboard` (R1 §11.10); session-source spelling `api_server` vs `api-server` — try the underscore first (R1 §11.11); `commands.catalog` skill-ranking sub-object (R1 §11.12); per-IP/per-token rate limiting (R1 §11.13); Hermes' behaviour for `stream_options.include_usage`, `response_format`, and reasoning fields on `/v1/chat/completions` (not stated in R1 — §4.1).
2. **Hermes voice/wake engine upstream licences** — pins known from `pyproject.toml`; each upstream LICENSE not fetched (R1 §11.1).
3. **Which Ollama release added `capabilities` to `/api/tags`** — 250 release bodies scanned, commit search returned zero (R2 §14.1). Also: server-side verification of `OLLAMA_AUTH` signatures (R2 §14.2); the release that introduced `/v1/responses` (R2 §14.3); OOM error text and status (R2 §14.4); image size/count limits and accepted formats (R2 §14.5); `/v1/audio/transcriptions` contract (R2 §14.6); `modelfile` crate licence (R2 §14.7); Ollama Cloud rate limits (R2 §14.8); whether Ollama's OpenAI layer accepts `image_url` as `{url}` as well as a bare string (R2 §14, R3 §11.6); the Anthropic-compat field matrix (R2 §14.10).
4. **`data: [DONE]` in a normative schema** — found only in prose and implementations (R3 §11.1); the Chat Completions create-method reference page 404s (R3 §11.2); ordering/density guarantees for `delta.tool_calls[].index` (R3 §11.3); custom-header APIs in `genai`/`rig` (R3 §11.4); whether vLLM/Ollama/llama.cpp enforce the structured-output keyword subset (R3 §11.5); Codex's numeric auto-compaction threshold (R3 §11.7); reqwest drop-cancellation as a documented guarantee (R3 §11.8); the 8 MB stored-image line vs the 512 MB request limit (R3 §11.10).
5. **Voice** — openWakeWord CPU cost on Apple Silicon (R4 §10.1); static ONNX Runtime 1.28 arm64 size (§10.2); `tract` opset completeness (§10.3); `objc2-speech` / `objc2-av-foundation` coverage of macOS 26 `SpeechAnalyzer` and of `AVFAudio`'s `AVSpeechSynthesizer` (§10.4); `NSSpeechRecognitionUsageDescription` and the hardened-runtime audio-input entitlement (§10.5); macOS privacy-indicator behaviour and mic contention under always-on capture (§10.6); Apple Voice-Processing I/O from Rust (§10.7); `misaki` G2P licence (§10.8); **per-model licences of sherpa-onnx KWS/ASR/TTS models — the largest remaining licence risk** (§10.9); Whisper latency by utterance length on M-series (§10.10); battery/CPU figures for PTT vs wake word (§10.11); `microWakeWord` star count anomaly (§10.12); `cpal` 0.18.2 vs master README (§10.13); `webrtc-audio-processing` LICENSE file 404 (§10.14); `whisper-rs` 0.16.0 source of truth on Codeberg (§10.15); `say(1)` details (§10.16).
6. **Chat UX** — escaping a literal leading `/` in any composer (R5 §11.1); unknown-command behaviour for opencode, Gemini CLI, Codex CLI (§11.2); T3 Chat (HTTP 429 on every fetch, §11.3); Msty (client-rendered docs, §11.4); Cherry Studio's user-visible pin UX (§11.5); Chatbox's data model (GPL, not researched, §11.6); TTFT/tokens-per-second as a rendered UI field (§11.7); temperature per message (§11.8); request ids in a UI (§11.9); stream re-attachment after a client reload (§11.10); background per-endpoint health monitoring (§11.11).
7. **Permissions** — Apple security-scoped bookmark API page (404; symbol names unsourced, R6 §10.1); macOS case-insensitivity in agent path checks — code shows plain `startsWith`, no advisory confirms (§10.2); Windows reserved device names (§10.3); MCP filesystem `0.6.4` vs `0.6.3` (§10.4); Codex `writable_roots` canonicalisation and symlink semantics (§10.5); Gemini `rootDirectory` enforcement per tool (§10.6); `.cursorrules` current status (§10.7); output filtering as a deployed mitigation — reported absent (§10.8); Claude Code licence (proprietary, no LICENSE, §10.9); OKF v0.1→v0.2 delta (§10.10).
8. **Repository gaps** — `.github/workflows/*` job matrix not quoted (G1 gaps); `docs/decisions.md` AD wording not verified against code comments (G4 §12.1); `docs/sync.md` (3009 lines) and `docs/sessions.md` not read (G3 §10.3, G4 §12.7); `keeper/src/menu.rs` second-hand (G2 gap 3); `config/keys.rs` `KEYS` body not enumerated (G2 gap 5); how a second long-lived async service would be held in `AppState` — `keeper/src/sync.rs` not read (G2 gap 7); `PanelTargetVm` variant addition partly determined (G2 gap 8); what a new floating window needs in `capabilities/*.json` (G2 gap 10, G5 gap 6); whether an AI surface's commands need `#[cfg(not(desktop))]` twins depends on the flag's iOS value, undecided (G2 gap 12); whether `IpcErrorCode` may be added freely — no test pins the set (G4 §12.8); whether `keeper` compiles on the current Linux host (G3 §10.2); `notes/merge.rs` (story 38.2) not found in the module list (G3 §10.8); `stability.rs`, `provenance.rs`, `conflict.rs` not read (G3 §10.7); the `keeper-recording://` non-overlap proof for a fifth scheme (G5 gap 8); whether the "no new Rust crates" rule that blocked `notes_attachment_paste` still binds (G5 gap 5); the phone tier's obligations for a new surface, UX-DR21–28 (G6 gap 8); whether any surface persists a list order other than chat pins (G6 gap 6); whether `MessageBubble` survives a non-Matrix VM structurally (G6 gap 4).

**Sources.** Full citation lists — publisher, URL, access date 2026-09-02 — live in the six R digests; repository grounding with `path:line` for every claim lives in the six G digests (front-matter). Version pins: Hermes Agent 0.21.0 / `v2026.8.31`; Ollama v0.33.2 / main `e5e4377`; MCP spec 2025-11-25 and `@modelcontextprotocol/server-filesystem` 0.6.3; Claude Code docs to v2.1.257; Telegram Bot API 10.3; crates.io metadata as of the access date. Nothing in this document was read from a private repository other than this worktree.
