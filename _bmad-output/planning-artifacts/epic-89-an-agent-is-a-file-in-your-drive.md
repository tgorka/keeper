# Epic 89 — An agent is a file in your drive

created: '2026-10-02'
status: planned 2026-10-02; build follows in story order on the agents stack
source: the owner's three rounds of 2026-10-01/02 (excerpts verbatim below, from `_bmad-output/planning-artifacts/agents-coordinator-decisions-2026-10-02.md` § *Owner's asks*). Other inputs:
- the binding architecture, `architecture/architecture-keeper-2026-07-03/ARCHITECTURE-AGENTS.md` (AD-360…AD-416; this epic builds AD-360…AD-366, AD-369, AD-390 and AD-396). Its *Data formats* are the grammars this epic implements, and are not restated here.
- the research, `research-agents-2026-10-02.md` (cited `§n.m`);
- the coordinator's pins P1–P15 and rulings R1–R29 (`_bmad-output/planning-artifacts/agents-coordinator-decisions-2026-10-02.md`), and the program map (`_bmad-output/planning-artifacts/agents-program-map-2026-10-02.md`);
- the two reviews of 2026-10-02, accepted in full by R28 and R29: `_bmad-output/planning-artifacts/agents-review-security-2026-10-02.md` (S-01…S-35) and `_bmad-output/planning-artifacts/agents-review-consistency-2026-10-02.md` (F1…F25). *The reviews' amendments*, below, says where each one lands in this epic;
- the digests D1 (runtime extraction), D2 (sessions for agents), D3 (provider sites), G1 (bots) and G5 (facts);
- upstream sources read on 2026-10-02 through the GitHub API, each cited by commit in 89.1.

Line numbers are in the current worktree on branch `agents-plan`: origin/main `86dee5b7` (release 0.9.0) plus the uncommitted plan files. Where a digest's line number has drifted, the current one is cited. The citations corrected after the consistency review (F16) were checked on 2026-10-02 against this worktree, with epic 89's code in progress in it.
binds: FR-767…FR-776; NFR-115 (89.4's share), NFR-116 (89.5's share), NFR-117 (89.5's share), NFR-118 (89.3's share), NFR-119 (89.1's share), NFR-120 (89.5's share), NFR-121 (89.6's share); AD-360…AD-366, AD-369, AD-390, AD-396. All of these are allocated in the architecture's *Requirements allocated here* and its ADs. Deferred items in DW-355…DW-360 and DW-430 (DW-355 is the architecture's); UX decision UX-DR127.
- **The previous ceilings** (program map, C1): epic 88; AD-359; FR-766; NFR-111; UX-DR126; DW-354; D-30. The architecture allocates AD-360…AD-416, FR-767…FR-822 and NFR-112…NFR-122.
- **No earlier allocation.** On 2026-10-02 a grep over `_bmad-output`, `docs`, `src`, `src-tauri/crates`, `dev`, `AGENTS.md`, `README.md` and `CLAUDE.md` looked for `epic-89`, `epic89-`, `DW-E89-` and `UX-DR-E89`.
  - It found only the architecture's DW-355 (Naia, `ARCHITECTURE-AGENTS.md:1234`) and D-31, which cites it (`docs/decisions.md` § D-31).
  - Deferred items in DW-355…DW-360 and DW-430. Every FR, NFR, AD and D number this epic binds is the architecture's.
see-also:
- D-4 (the endpoint is the person's; its revisit trigger is met by 89.6), D-21 (a derived index is disposable), D-30 (a grammar is a contract), D-31 (an agent lives in a drive, and its session's log is the truth; `docs/decisions.md` § D-31), D-34 (labels; `docs/decisions.md` § D-34);
- AD-6, AD-24, AD-40, AD-55/56, AD-65 (`browse::resolve` is the one containment rule), AD-146 (the closed provider set), AD-151 (unknown is never false), AD-154 (amended for agent sessions only), AD-158, AD-159, AD-342 (the voices flag, the recipe 89.2 copies), AD-116…AD-121 (the flat session);
- `docs/sessions.md`, `docs/sync.md` (folder flags), `docs/egress.md`.

## The owner's ask

Verbatim (round 1, 2026-10-01):

> i want to explore topic of creating an agents system in keeper - similar to grok bot or hermes bot for work with having different sessions (active/archived), different persons will hav diffrent tools to use (and/or access to local drive or notes or use the keeper itself or the whole computer) - it could connect to model provider cliproxy like this omp. there will be one main person (like nixi for me) to talk to on everyday basis
> i want bmad style personalities and different purposes (coding, exploring, designing, marketing, hr, psychologist etc)
> Use sessions in the drives as a working place of the agent with all the data he needs, scripts he needs to use etc logs but also the message history and actions taken - so the session can be used after. - also after sync by drive the work can be continued on other device that will sync the data in sessions (data is all he needs) - but make sure its fast to operate.

Round 2 (2026-10-01):

> nixi etc are everywhere - because session is sync - they can have tags (like nixi with electra or hesperia tag) to know what materialization of nixi is used.
> i like hermes self improvement mehanism and continues memory
> i own the server infrastructure and tgrive is only for tgorka - make sure the sensitive part goes only to the private bots/drives (nixi needs to be told to use what drive context - but can multiple)

Round 3 (2026-10-02):

> - memory, skills, soul, etc bot data find a right place in the drives fro this files (tgdrive i neuradrive)
> - rewriting to rusr recommended parts (add separate source module)

## The verdict, ask by ask

The epic is a plan, so the verdict is what the plan does.

| # | The ask (verbatim) | Verdict | How this epic meets it | Mechanism |
| --- | --- | --- | --- | --- |
| 1 | "memory, skills, soul, etc bot data find a right place in the drives" | **planned** | An agent's home is a folder under `80-agents/` in the drive whose readers it serves: `agent.toml`, `SOUL.md`, `USER.md`, `MEMORY.md`, `journal/`, `proposals/`. The zone's `_drive.toml` names the readers. | AD-361, AD-362; 89.2, 89.3 |
| 2 | "i want bmad style personalities" | **planned** | `SOUL.md` carries BMAD's persona fields. A BMAD agent is imported by BMAD's own merge rule, ported to Rust. | AD-362, AD-363, AD-396; 89.1, 89.3 |
| 3 | "rewriting to rusr recommended parts (add separate source module)" | **planned** | `src-tauri/crates/keeper-ported`, one module per upstream, each with an `UPSTREAM.md`. This epic ports `agentskills` and `bmad::config`. | AD-396; 89.1 |
| 4 | "Use sessions in the drives as a working place … logs but also the message history and actions taken" | **planned** | A flat session plus `agent.toml` and an append-only JSONL log. The log holds every model step, tool call and tool result. | AD-365, AD-366; 89.5 |
| 5 | "after sync by drive the work can be continued on other device … (data is all he needs)" | **planned** | Replay rebuilds the exact messages array, tool steps included, from the session folder alone. | AD-365; 89.5 |
| 6 | "but make sure its fast to operate" | **planned** | A log is never read on the hot path. `<zone>/.keeper/agents.db` answers lists and boards. Chunks never become LFS objects. | AD-366, NFR-116; 89.5 |
| 7 | "they can have tags (like nixi with electra or hesperia tag)" | **planned, in part** | Every log line and chunk name carries the writing host's slug. The copy (`nixi@electra`) is Epic 90's. | AD-366; 89.5 (lines), 90.4/90.6 (copies) |
| 8 | "make sure the sensitive part goes only to the private bots/drives" | **planned, in part** | Labels (readers, integrity) and their lattice. Enforcement at every sink is 92.6. | AD-390; 89.4 |
| 9 | "it could connect to model provider cliproxy like this omp" | **planned** | A third provider kind, `openai`, read against CLIProxyAPI at `https://electra.siren-alsephina.ts.net:8452`. | AD-369; 89.6 |
| 10 | "i like hermes self improvement mehanism and continues memory" | **planned, in part** | This epic gives Hermes' caps and `§` format, read once per session as a frozen snapshot. Proposals and consolidation are Epic 95's. | AD-364; 89.3 |

## What the triage found

| Need | Verdict | Evidence |
| --- | --- | --- |
| A folder flag for a zone | **present, as a recipe** | The voices flag (AD-342): `DEFAULT_VOICES_SUBFOLDER` (`keeper-sync/src/profile/mod.rs:247`), `VoicesConfig` (`:898-913`) with `validate` (`:933`), `voices` (`:1307`), `voices_root` (`:1525`), the validation call (`:1668-1670`), `("voices", Allowed)` (`profile/folder.rs:277`), the shell's VM, request and apply arm (`keeper/src/sync_ipc.rs:228-235`, `:812-818`, `:1233-1250`, `:1300-1306`), the Files role (`FilesFolderRoles`, `keeper-core/src/vm.rs:4194`; `FilesFolderRoleVm`, `:4170`), the account record (`org_account/manifest.rs:81`, `:492`; `org_account/state.rs:111`; `keeper/src/account_settings.rs:376`), the form (`src/components/sync/add-folder-form.tsx:769`, `:2299-2307`) and the store (`src/lib/stores/sync.ts:483-541`). |
| An agent concept | **absent; the word "bot" is taken** | `keeper-core/src/bots` and `[[provider.bot]]` mean provider models (D2 §7). Ruling R1 names the concept *agent*. `keeper-core/src/lib.rs:12-55` has no `agents` module. |
| A per-agent system prompt | **absent** | The only system message is the context bundle plus the spoken-language sentence (`keeper/src/bots_ipc.rs:1177-1186`; G1 §2). |
| Frontmatter for a soul | **present, a subset** | `notes::frontmatter` parses scalars, block and flow lists, one level of maps and `"…"` escapes (`frontmatter.rs:16-23`; `unescape_double`, `:725`). A scalar is single-line by construction (`:54-55`), so a multi-line value exists only as `\n` escapes inside a double-quoted string, which `quote_double` (`:1011`) writes. It does **not** parse block scalars (`|`, `>`). Writing goes through `Frontmatter::serialise_new` (`:258`). |
| Tool calls kept for replay | **absent** | A turn stores a `user` and an `assistant` row only (`bots_ipc.rs:1275-1284`; G1 §2). The wire replays `ToolCall::arguments_raw` verbatim (`bots/chat.rs:340-353`, `:449-463`). |
| An append primitive in a session | **absent, by construction** | `drive_write` cannot create (`files_write.rs:503-505`); `files::compile_new` (`sessions/files.rs:671`) refuses `.jsonl` through `check_rel` (`:209-212`), whose extensions are `NewFileKind::parse`'s (`:108`; D2 §4). Ruling R3 gives the log its own writer. |
| A derived per-zone index | **present, for notes** | `notes/search_index.rs` defines the notes index and its file name (`SEARCH_DB_FILE`, `:21`); the shell opens it at `<vault>/.keeper/search.db` (`keeper/src/notes_vault.rs:1139`). D-21 makes it disposable, and `.keeper/` is Tier-0, never committed (`docs/decisions.md:1099-1127`). |
| A third provider kind | **absent; AD-146's revisit trigger met** | `ProviderKind { Hermes, Ollama }` (`bots/mod.rs:69`). Eleven compile-forced arms, four silent sites and three fail-closed decodes (§12.11; D3 inventory A). |
| BMAD's licence | **established in this pass** | BMAD-METHOD is MIT at tag `v6.12.0` (commit `05bfbd46d00766ec88eb9b42e76be2c575d64d7b`), "Copyright (c) 2025 BMad Code, LLC", with a trademark notice (`TRADEMARK.md`). It settles research §14's open item and the architecture's Ambiguity 9. |
| BMAD's merge rule | **read at the pinned tag** | `src/scripts/config_utils.py` at `v6.12.0` is byte-equivalent to the installed `_bmad/scripts/config_utils.py:79-119`. `structural_merge` is identical on `main@4f61d4e7`, where the scripts have since moved to `skills/bmad/scripts/` and `load_central_config` dropped `config.user.toml`. |
| agentskills' validator | **read** | `agentskills/agentskills@69ef37e9424c0a7ea9dd2293b559e43ec8176379`, `skills-ref/src/skills_ref/validator.py`, licensed Apache-2.0 (`skills-ref/LICENSE`). |

## The one sentence

**keeper can hold a person's files and talk to a model, but nothing in a drive says who an agent is, who may read what it reads, what it was told or what it did — so an agent cannot exist, move between machines or be checked.** The fix is files with grammars:
- **a zone and its readers**, `80-agents/_drive.toml`;
- **a home**, `agent.toml`, `SOUL.md` and capped memory;
- **a prompt** composed in one order, with its digest kept;
- **a label** on everything read;
- **a session log** that replays the model's exact context;
- **a provider kind** that speaks to CLIProxyAPI.

## The reviews' amendments

Two reviews of this plan, of 2026-10-02, were accepted in full (rulings R28 and R29). Epic 89 was already being built when they landed, so four of them are code-level amendments, A1–A4, which the stories below now state. Where each finding lands in this epic:

| finding | what changes | where |
| --- | --- | --- |
| S-02 (A1) | `_drive.toml` gains `[integrity] untrusted = [globs]`, default `["00-inbox/**", "70-comms/**", "recordings/**"]`; a present table replaces the default. A read in an untrusted zone, or of a file whose OKF `sources` cite the web, is `untrusted` whoever last wrote it. | 89.2, 89.4 |
| S-04 (A2) | `Label.local_only`: the join is OR, and the key is written only when true. `Label::may_use_model(local)` answers whether a model may see the content. The per-call check is 90.5's; the `Sink::Model` row is 92.1's and 92.6's. | 89.4 |
| S-05 (A3) | Every log line carries `claim` (the claim event id) beside `epoch`. Two `acquired` claims at one epoch with different claim ids mark the session conflicted, and replay refuses it. | 89.5 |
| S-17 (A4) | `keeper_core::agents::redact::redact_secrets`, applied by `ChunkWriter::append` before a line is written. What it cannot see is DW-430. | 89.5 |
| S-15 | `_drive.toml` stays the drive's declaration; the readers and owner it must match are pinned by the host (90.3, 90.6). | 89.2 (cross-reference) |
| S-20 | Tests and fixtures name the provider endpoint from `KEEPER_OPENAI_SMOKE_BASE_URL` or an `example` host, never the tailnet host. | 89.6 |
| S-31 | The docs say that `owner` integrity means "committed by a reader's keeper", not "written by that person". | 89.4 |
| S-12 | A forward note: 95.2 keeps a skill carrying `metadata.keeper_proposal` out of 89.3's index until a person adopts it. | *Names other epics cite* |
| F6 (R24) | 89.3's tool vocabulary is AD-397's as R24 completed it, `kvm_snapshot` and `kvm_act` included. | 89.3 |
| F7 (R25) | The session `agent.toml`'s `kind` gains `conversation`, a proxy conversation the person started. | 89.5 |
| F16 | Drifted code citations corrected against this worktree. | triage, C1, 89.2, 89.3, 89.6, DW-357 |
| F17 | The architecture's `bot:openai:` Nixi example stands; 89.3's `ollama` fixture is a named variant of it. | 89.3 |
| F18 | A steward's default set is the specialist's plus `delegate`. | C8, *Kind defaults* |
| F22 | Committed records are cited by path; the header lists the deferred items. | header, *What stays out*, *Sprint-status entry* |
| F23 | The drive-cost measurement is kept as an `#[ignore]` test. | 89.5, DW-359 |

Every other finding of the two reviews lands outside this epic.

## Requirements

Copied from the architecture's table, as amended after the reviews (2026-10-02). This epic binds these rows and allocates none.

| id | statement | story | AD |
| --- | --- | --- | --- |
| FR-767 | keeper carries ported upstream code in one crate, `keeper-ported`, one module per upstream, each with an `UPSTREAM.md` naming the repository, commit, licence and what was changed; the crate has no keeper dependency, opens no socket and links no tauri. | 89.1 | AD-396 |
| FR-768 | A person can make an agent from a BMAD agent: its persona layers are merged by BMAD's own rules into a `SOUL.md`, and the drive's skills are validated by agentskills rules, a refused skill listed with its reason and never offered. | 89.1, 89.3 | AD-362, AD-396 |
| FR-769 | A synced folder flagged `[folder.agents]` keeps agents under `80-agents/` (or the subfolder it names), beside its sessions zone; its `_drive.toml` names the drive, its principal, its owner and its readers by Matrix id; a zone without a valid one hosts no agent and says why. | 89.2 | AD-361 |
| FR-770 | An agent is a folder with `agent.toml` and `SOUL.md`; every machine key is validated and an unknown one is named; the same name in two drives is two agents; no tool can change an agent's soul, tools or core memory. | 89.3 | AD-360, AD-362 |
| FR-771 | An agent's instructions are composed in one fixed order — soul, core memory, skills, menu, the session's frame, context files as data — and the person can read exactly what the agent was told. | 89.3 | AD-363 |
| FR-772 | `USER.md` holds at most 1375 characters and `MEMORY.md` at most 2200, as `§`-separated entries; a change that would exceed them is refused with the current entries; a session sees the memory as it was when it opened. | 89.3, 95.1 | AD-364 |
| FR-773 | Everything an agent reads carries who may read it and how far it can be trusted; a session's label is the join of all of it and the person sees it as a chip. | 89.4 | AD-390 |
| FR-774 | An agent's session is a session folder in its home drive with `agent.toml`, its log and its approvals; any host with the folder continues it from the files alone, tool calls and results included. | 89.5 | AD-365 |
| FR-775 | A session's log is append-only JSONL in dated, per-host, size-bounded chunks with large bodies as blobs, written by one host; the board and the session list answer from a rebuildable index, never by re-reading logs. | 89.5 | AD-365, AD-366 |
| FR-776 | A provider of kind `openai` — any OpenAI-compatible endpoint, such as CLIProxyAPI — is added, tested and used like the other kinds; its models are listed from `/v1/models` and keeper runs its tools. | 89.6 | AD-369 |
| NFR-115 | **No byte crosses principals.** No content from a drive reaches a process, room, drive, memory file, MCP server, KVM or command whose audience is not within that drive's readers, except by a recorded declassification, and none reaches a model that is not local while the session's label is `local_only`. A model provider is a processor the person chose for the agent, not an audience (D-34). Proved per sink (send, invite, status and scope edits, delegate, write, propose, promote, MCP, KVM, `run`, model calls including embeddings and review-layer helpers) by tests that try. | 89.4, 90.3, 92.6 | AD-377, AD-390, AD-391 |
| NFR-116 | **A log never becomes a large file, and is never read on the hot path.** No chunk reaches `min(192 KiB, 3/4 × lfs_threshold_bytes)`; no line exceeds 64 KiB; a body over 16 KiB is a blob; the board, the list and a turn read the index and the in-memory context only: the second turn of a served session opens no file under `log/` (AD-366's `SessionContext`). | 89.5, 90.5 | AD-366 |
| NFR-117 | **A crash loses nothing and repeats nothing.** A torn last line is truncated on open and the rest reads; a turn's lines are `fsync`ed at its end; an interrupted session plan resumes at start; an approval is consumed at most once across crashes, restarts and takeovers: the owning host acts only after the homeserver has accepted its `dev.keeper.agent.approval.consumed` event and that event is the first for the approval in the room, and a host resuming a session reads the room before its own log. | 89.5, 90.2, 93.2 | AD-366, AD-368, AD-394 |
| NFR-118 | **Memory stays bounded.** Core memory never exceeds 1375 and 2200 characters; a consolidation that would lose more than 25 % is rejected; a session's memory snapshot never changes while it runs. | 89.3, 95.1, 95.2 | AD-364, AD-401 |
| NFR-119 | **The licence firewall holds.** Every ported module names a permissive licence in its `UPSTREAM.md`; every new crate passes `cargo deny`; what cargo cannot see — `ort`'s prebuilt ONNX Runtime library, the Android build's Gradle dependencies — is listed with its licence where it is added and checked by hand in its story (97.1, 98.3); AGPL and GPL software (Sygnal, ntfy's GPL option, NanoKVM firmware, Element Call) is run as a separate service or read as a protocol, never linked; every model in `_models/` names its licence. | 89.1, 94.1, 96.5, 97.1, 98.3 | AD-396, AD-410 |
| NFR-120 | **One writer per session.** At most one host writes a session per epoch; a holder that cannot renew stops writing at least 60 s before another host may take over; every line carries the epoch and the claim event it was written under, and a late line from a superseded epoch is dropped by every reader; two hosts that acquired one epoch with different claim events mark the session conflicted, and nothing replays it until a person resolves it. | 89.5, 90.6 | AD-378 |
| NFR-121 | **No destination the person did not configure.** The agents add only the homeserver, provider base URLs, MCP servers, KVMs and the push gateway the person configured, each derived into the egress list (AD-53) and diffed at release; a networked `run` reaches only what its approval shows, and a `docs/egress.md` row says so. `keeper-agent` and `keeper-agentd` register no observability sink; the desktop's export never reads a span or event under the targets `keeper_agent` and `keeper_core::agents`, pinned by a test; `check:agentd-lean` forbids `opentelemetry*` and `posthog*` crates. | 89.6, 90.5, 90.6, 96.1, 96.2, 96.5, 98.1 | AD-369, AD-375, AD-405, AD-406, AD-409, AD-412 |

**This epic's share of a shared NFR.**
- NFR-115: 89.4 proves the lattice never widens a label, and that `local_only` travels with it. Every sink is 92.6's; the model check is 90.5's first.
- NFR-116: 89.5 proves the chunks, the blobs and the index; the in-memory context is 90.5's.
- NFR-117: 89.5 proves the torn tail and the turn-end `fsync`. Plans are 90.2's, approvals 93.2's.
- NFR-120: 89.5 proves the line-level fence on the epoch and the claim event, and the conflicted log (A3). Claims are 90.6's.
- NFR-118: 89.3 proves the caps and the frozen snapshot. Consolidation is Epic 95's.
- NFR-119: 89.1 records two licences.
- NFR-121: 89.6 proves the new kind adds only its base URL.

**Held, not restated:** AD-65 (one containment rule), AD-151 (an unread capability is unknown), AD-158 (a grant is re-checked per call, and the audit row precedes the effect), AD-159 (file content is data, and every bound is disclosed), D-21 (a derived index is disposable).

## Choices this plan makes where the architecture left room

Each is a plan reading the coordinator can overturn; none is silent.

- **C1 — the chunk writer lives in `keeper-core`.**
  - **What the architecture says.** AD-366 calls the writer "`keeper-agent`'s `SessionWriter`", but `keeper-agent` is created by 90.1, after this epic.
  - **The plan.** 89.5 puts the file-level primitive, `keeper_core::agents::log::ChunkWriter`, in core. It covers `O_APPEND`, the torn tail, rotation, blobs, secret redaction (A4) and `fsync`. 90.5 wraps it as `keeper_agent::writer::SessionWriter`, which adds the claim's epoch and event id (A3), `rotate_at` from the profile, and the index update.
  - **Why core.** The primitive needs nothing from `keeper-sync`, and AD-6 sends such code to core: the same argument R6 used for `agents::matrix`. Core already writes files with `fsync` (`org_account/descriptor.rs:1251-1255`).
  - **Its input.** The writer takes a session directory its caller resolved through `browse::resolve`, composes every file name itself, and refuses a symlinked `log/`.
- **C2 — core parses text; the host walks the drive.**
  - Every `agents::*` function in this epic takes the text of a file (or a directory listing) and decides; none opens a file in a zone.
  - The walk that finds and reads `_drive.toml`, `agent.toml`, `SOUL.md`, memory and `_skills/*/SKILL.md` lands in `keeper_agent::zone` (90.5), through `browse::resolve`. Core cannot call it (AD-40), and AD-65 makes it the only containment rule.
  - Tests read real fixture files from `keeper-core/tests/fixtures/agents/`.
- **C3 — memory over its cap at read time.** FR-772's refusal is about a *change*, and 89.3 has no writer (Epic 95 has).
  - **When a person's hand-edited `USER.md` or `MEMORY.md` is already over its cap**, the session's snapshot leaves that file out. The problem lists its entries and appears in "what the agent was told". The plan never truncates.
  - **The same applies** to a duplicate entry or an invisible format character.
  - **The alternative**, refusing to open the session, would silence a proxy over a typo. Q2 asks.
- **C4 — the session frame's "date and time".** The frame carries the writing host's local time with its offset (`2026-10-02T10:15:03+02:00`). Log lines stay RFC 3339 UTC, as *Data formats* says.
- **C5 — `tool_call.args` is `arguments_raw`.**
  - The line's `args` is the string the model streamed, verbatim. It is not the parsed object, because the wire replays `arguments_raw` (`chat.rs:348`), and only that keeps replay byte-exact.
  - Replay re-parses it into `ToolCall::arguments`.
- **C6 — `WriteScope::with_agents` is armed in the shell in 89.3.** The epic map lists 89.2 and 89.6 as this epic's shell stories. The fence must be armed where the tool scope is built, which until 90.1 is `keeper/src/bots_tools.rs:424-433`. So 89.3 touches the shell by one call.
- **C7 — BMAD is pinned at the tag the repo runs.** 89.1 ports `v6.12.0` (`05bfbd46…`), which is what `_bmad/` holds. The newer `main` layout is recorded as a revisit note in `bmad/UPSTREAM.md`, and Epic 94 (story 94.1) agreed.
- **C8 — a steward's default tools follow AD-389, not AD-397.** The two disagree.
  - AD-397: a steward gets `proxy`'s set plus `card_update`, `session_write` and `workflow_start`, which includes the surface tools.
  - AD-389: a steward gets "`card_update`, `delegate` and `workflow_start` [added] to a specialist's; nothing else".
  - AD-389 is the steward's own decision, and AD-383 keeps the surface tools for the person's proxy, so 89.3 pins AD-389's reading.
  - Epic 91–93's lane (92.5) builds on the same reading.
  - **In effect** a steward's set is the specialist's plus `delegate`: the specialist's set already holds `card_update` and `workflow_start` (F18). Ruling R27 settled this reading, and the architecture's AD-397 is corrected to match.
- **C9 — writing `[folder.agents]` into a real drive waits for every machine.** tgdrive's folder file warns that an older keeper refuses a `[folder]` table holding a key it does not know, together with the working keys beside it (`/workspace/tgdrive/.keeper/keeper.toml:6-10`).
  - So 89.2 ships the flag and documents its version floor. No drive's folder file gains `[folder.agents]` in this epic.
  - Writing it into a real drive is 91.5's operator action, after every machine that syncs that drive runs a keeper carrying `epic89-home`.

## Open questions for the coordinator

Each has the reading this plan builds to, so no lane is blocked.

- **Q1. Where the chunk writer lives (C1).** **Settled by R27:** the file-level writer lives in `keeper-core`, and 90.5's runtime wraps it.
  - **Plan's reading:** `keeper_core::agents::log::ChunkWriter`, wrapped by `keeper_agent::writer::SessionWriter` in 90.5.
  - **The alternative:** create `keeper-agent` in 89.5 for the writer alone. That breaks AD-367's "extracted first with the shell as its only consumer".
- **Q2. A core-memory file already over its cap when a session opens (C3).**
  - **Plan's reading:** the snapshot leaves it out and says so. The alternative refuses to open the session.
- **Q3. Steward default tools: AD-389 or AD-397 (C8).** **Settled by R27:** AD-389. The architecture's AD-397 sentence is corrected to match (F18).
- **Q4. The epic map lists 89.2 and 89.6 as this epic's shell stories.** **Settled by R27:** 89.3 touches the shell by one call (C6), and the epic map's row names 89.3 too.

## Stories

Every story names its rung (*Stack rungs*, below).
- **The shell is by inspection.** Every story that touches `src-tauri/crates/keeper/**` is named in its PR as awaiting CI's macOS job, and is gated by `bun run check:rust:macos` on hesperia (`scripts/check-macos.sh`; scope guard, research §13 #25).
- **Mutation-proved.** Every new behaviour test is mutated, run and restored, and the restore is confirmed by reading the diff.
- **Generated bindings** (`src/lib/ipc/gen/*.ts`) are regenerated, never hand-edited, and `bindings:check` is green on each rung.
- **Names** below are the plan's. Lanes may rename them, and the behaviour may not change. Names cited by sibling epics (90–95) are listed in *Names other epics cite* and must not change without telling them.

### 89.1 — keeper-ported crate: agentskills + BMAD merge

**Intent:** "rewriting to rusr recommended parts (add separate source module)"; "i want bmad style personalities". **Rung:** **epic89-home** (lands with 89.3, its first consumer: P14's rule). AD-396; FR-767, FR-768 (its port half); NFR-119 (its share).

**Files:**
- `src-tauri/crates/keeper-ported/Cargo.toml` (new):
  - `[lints] workspace = true`;
  - dependencies `toml` and `unicode-normalization`, both workspace dependencies already in `Cargo.lock` (`src-tauri/Cargo.toml:296`; `keeper-core/Cargo.toml`'s `unicode-normalization`);
  - nothing else.
- `src-tauri/Cargo.toml:3`: `"crates/keeper-ported"` in `members`.
- `keeper-ported/src/lib.rs`. Its crate doc states the rules:
  - no keeper dependency, no network, no async runtime, no tauri;
  - one module per upstream, each with `UPSTREAM.md`;
  - a module lands with its first consumer;
  - and it declares `pub mod agentskills; pub mod bmad;`.
- `keeper-ported/src/agentskills/mod.rs`, ported from `skills-ref/src/skills_ref/validator.py` at `69ef37e9`.
  - `MAX_SKILL_NAME_LENGTH = 64`, `MAX_DESCRIPTION_LENGTH = 1024`, `MAX_COMPATIBILITY_LENGTH = 500`, `ALLOWED_FIELDS = ["name", "description", "license", "allowed-tools", "metadata", "compatibility"]`.
  - `pub enum MetaValue<'a> { Str(&'a str), Other }`.
  - `pub fn validate_metadata(fields: &[(&str, MetaValue<'_>)], dir_name: Option<&str>) -> Vec<String>`, returning upstream's error sentences verbatim.
  - **Python to Rust.** Length counts `char`s (Python `len`). Case is `to_lowercase` (Python `lower`). NFKC comes from `unicode-normalization`. Python's `isalnum` becomes `char::is_alphanumeric`.
  - **The file header** carries Apache-2.0 §4(b)'s notice: modified from the named file, and how.
  - **Not ported:** upstream's directory and YAML reading (`validate`, `parser.py`). keeper parses `SKILL.md` with its own frontmatter subset (89.3), and the three directory sentences (`Path does not exist`, `Not a directory`, `Missing required file: SKILL.md`) are reproduced there.
- `keeper-ported/src/bmad/mod.rs` and `keeper-ported/src/bmad/config.rs`, ported from `src/scripts/config_utils.py` at `v6.12.0`:
  - `pub struct ConfigError`, whose `Display` is upstream's message;
  - `pub fn load_toml(path: &Path, required: bool) -> Result<toml::Table, ConfigError>`;
  - `pub fn structural_merge(base: toml::Value, over: toml::Value) -> Result<toml::Value, ConfigError>`, with `_detect_keyed_merge_field` and `_merge_arrays` as private functions, keyed fields in upstream order (`code`, then `id`);
  - `pub fn merge_layers(layers: impl IntoIterator<Item = toml::Table>) -> Result<toml::Table, ConfigError>`;
  - `pub fn load_customization(project_root: Option<&Path>, skill_dir: &Path) -> Result<toml::Table, ConfigError>`, over three layers: `customize.toml`, `_bmad/custom/<skill>.toml`, `_bmad/custom/<skill>.user.toml`.
  - **Not ported:** `load_central_config` belongs to 94.1, which owns the four central layers.
- `keeper-ported/src/agentskills/UPSTREAM.md` and `keeper-ported/src/bmad/UPSTREAM.md`. The record format is these key lines first, then prose: `repository:`, `commit:`, `licence:`, `copyright:`, `files read:`, `ported:`, `not ported:`, `changed:`, `revisit:`.
- `keeper-ported/tests/fixtures/bmad/bmad-agent-architect/customize.toml`: a copy of the installed `~/.claude/plugins/cache/bmad-method/bmad/6.12.0.0/skills/bmad-agent-architect/customize.toml`, MIT, attributed in `UPSTREAM.md`.
  - Beside it, a team override `_bmad/custom/bmad-agent-architect.toml` and the golden `winston-resolved.json`.
  - The golden was captured once with `python3 _bmad/scripts/resolve_customization.py --skill <fixture dir> --project-root <fixture root> --key agent`. The capture command is recorded in the fixture's README, and the script is not kept.
- `keeper-ported/tests/upstream.rs`, the record test (acceptance 6).
- `package.json`:
  - `check:ported-pure`, modelled on `:25-27`: `tree=$(cargo tree --manifest-path src-tauri/Cargo.toml -p keeper-ported -e normal,build --prefix none) && ! printf '%s' "$tree" | grep -qE '(^|[[:space:]])(keeper-core|keeper-sync|keeper-agent|tauri(-[a-z]+)*|reqwest|hyper|tokio|matrix-sdk(-[a-z]+)*|gix(-[a-z-]+)*) v'`;
  - appended to `check` (`:31`).
- `lefthook.yml:47`: `-p keeper-ported` in the clippy fallback.

**Acceptance:**
1. **BMAD's own tests pass in Rust.** The risk is a drifted port; the cases are upstream's own (`src/scripts/tests/test_config_utils.py` at `v6.12.0`). In `bmad::config::tests`:
   - `structural_merge_recurses_appends_and_replaces_keyed_tables`, with upstream's `base`/`override` (`nested` keeps `keep` and takes `replace`, `plain` appends, `items` replaces `one` and appends `two`);
   - `non_string_keyed_identifier_is_rejected` ("identifier `id` must be a string");
   - `present_malformed_optional_layer_is_rejected` ("failed to parse");
   - `missing_optional_layer_is_empty`;
   - `customization_layer_precedence`: the `load_customization` half of `test_filesystem_layer_precedence`, where `default` < `team` < `user` gives `"user"`.
2. **Keyed merge order.** `keyed_merge_prefers_code_then_id_and_appends_when_any_item_lacks_it`:
   - tables carrying both `code` and `id` merge by `code`;
   - if any table lacks the key, the arrays append;
   - an empty identifier is refused with "must not be empty".
3. **A real BMAD agent merges as BMAD merges it.** `merges_winston_like_resolve_customization`. The risk is the gap between a port and the real helper, so the result is compared to the golden that BMAD's own `resolve_customization.py` printed:
   - Winston's `customize.toml`, with an override that appends a principle and replaces menu `CA`, merged by `load_customization`, equals `winston-resolved.json`'s `agent` table.
   - Comparison is semantic, over the JSON values.
4. **agentskills' own cases pass.** In `agentskills::tests`, upstream's metadata cases (`skills-ref/tests/test_validator.py` at `69ef37e9`), each asserting the upstream substring:
   - `valid_skill`, `invalid_name_uppercase` ("lowercase"), `name_too_long` (70 characters, "exceeds" and "character limit"), `name_leading_hyphen`, `name_consecutive_hyphens`, `name_invalid_characters` (`my_skill`), `name_directory_mismatch` ("must match skill name"), `unexpected_fields`;
   - `valid_with_all_fields`, `allowed_tools_accepted`, `i18n_chinese_name` (`技能`), `i18n_russian_name_with_hyphens`, `i18n_russian_lowercase_valid`, `i18n_russian_uppercase_rejected`;
   - `description_too_long` (1100), `valid_compatibility`, `compatibility_too_long` (550), `nfkc_normalization` (`cafe\u{301}` against the directory `café`).
5. **The crate stays pure, and the guard bites.** `bun run check:ported-pure` passes. Mutation: adding `tokio = { workspace = true }` to `keeper-ported/Cargo.toml` makes it fail; restoring makes it pass.
6. **Every module carries its record.** `every_module_has_an_upstream_record_whose_licence_is_allowed` (`tests/upstream.rs`):
   - every directory under `src/` has `UPSTREAM.md`;
   - each has the key lines of the format above;
   - its `licence:` value is on `src-tauri/deny.toml`'s `[licenses] allow` (read from the file, `deny.toml:7-22`), or is the exact sentence "written from documentation; no upstream code is copied".
   - Mutation: `licence: GPL-3.0` fails it.
7. **`bmad/UPSTREAM.md` records the licence** (closing research §14's row and Ambiguity 9):
   - `repository: https://github.com/bmad-code-org/BMAD-METHOD`;
   - `commit: 05bfbd46d00766ec88eb9b42e76be2c575d64d7b (tag v6.12.0)`;
   - `licence: MIT`, `copyright: Copyright (c) 2025 BMad Code, LLC`;
   - `files read: src/scripts/config_utils.py, src/scripts/tests/test_config_utils.py, LICENSE, TRADEMARK.md`;
   - `ported:` the six functions, and `not ported: load_central_config (94.1)`;
   - `changed:` Python dict/list to `toml::Value`, and errors as `ConfigError`;
   - `revisit:` `main@4f61d4e769e50bc11d0d5d724f48942aac699679` moved the scripts to `skills/bmad/scripts/` and dropped `_bmad/config.user.toml` from the central layers. `structural_merge` is unchanged there.
   - **The trademark paragraph:** "BMad™ is a trademark of BMad Code, LLC, not licensed under MIT. keeper uses the name only to describe compatibility ('compatible with BMad Method'), never as a product, feature or UI name."
8. **`agentskills/UPSTREAM.md`** records:
   - `repository: https://github.com/agentskills/agentskills`, `commit: 69ef37e9424c0a7ea9dd2293b559e43ec8176379`, `licence: Apache-2.0`;
   - `files read: skills-ref/src/skills_ref/validator.py, skills-ref/tests/test_validator.py, skills-ref/LICENSE`;
   - the §4(b) modification note.
9. **The firewall.** `cargo deny check` passes. `Cargo.lock` gains the path crate `keeper-ported` and no package from a registry. `check:core-tauri-free` and `check:core-sync-free` stay green.

**Shell crate:** no. Linux-gated by lefthook and the existing gates.

**binds:** FR-767, FR-768, NFR-119, AD-396

### 89.2 — The agents zone

**Intent:** "memory, skills, soul, etc bot data find a right place in the drives fro this files (tgdrive i neuradrive)"; "tgrive is only for tgorka". **Rung:** **epic89-home**. AD-361; FR-769; UX-DR127.

**Files:**
- `keeper-sync/src/profile/mod.rs`, mirroring the voices recipe line for line:
  - `pub const DEFAULT_AGENTS_SUBFOLDER: &str = "80-agents";`, beside `:224`/`:247`;
  - `pub struct AgentsConfig { pub subfolder: String }`, `#[serde(rename_all = "camelCase")]`, with `default_agents_subfolder()` beside `:1337`;
  - `impl AgentsConfig { pub fn validate(&self, notes: Option<&NotesConfig>, recordings: Option<&RecordingsConfig>, sessions: Option<&SessionsConfig>, tasks: Option<&TasksConfig>, voices: Option<&VoicesConfig>) -> Result<()> }`;
  - `#[serde(default)] pub agents: Option<AgentsConfig>` after `voices` (`:1307`);
  - `agents: None` in `new` (`:1386`);
  - `pub fn agents_root(&self) -> Option<PathBuf>` beside `voices_root` (`:1525`);
  - the validation call last in `SyncProfile::validate` (after `:1668-1670`).
- `keeper-sync/src/profile/folder.rs`: `("agents", FolderFieldRule::Allowed)` after `:277`, with the comment the other zone rows carry.
- `keeper-core/src/agents/mod.rs` (new module, `pub mod agents;` in `keeper-core/src/lib.rs`).
- `keeper-core/src/agents/drive.rs`, the `_drive.toml` grammar of *Data formats*:
  - `DriveDecl { id, title, principal, owner: OwnedUserId, readers: BTreeSet<OwnedUserId>, local_only, untrusted: Vec<String> }`;
  - **`[integrity] untrusted = [globs]`** (A1, S-02): drive-relative glob patterns whose files are `untrusted` whoever last wrote them, because they hold what came from outside the readers (an inbox, messages, recordings of other people). Without the table, `untrusted` is `DEFAULT_UNTRUSTED = ["00-inbox/**", "70-comms/**", "recordings/**"]`. A present table replaces the default whole, so `[integrity]` alone or `untrusted = []` means none. Each pattern is compiled with `globset` (already a core dependency), and one that does not compile is refused naming it;
  - the declaration is the drive's own; the readers and owner a host serves are pinned by that host (S-15: `agentd.toml`'s `[[drives]]` in 90.3, the desktop's pin in 90.6), and a declaration that differs from the pin hosts nothing there;
  - `pub fn parse(text: &str) -> Result<DriveDecl, DriveDeclRefusal>`;
  - `DriveDeclRefusal::sentence()`;
  - Matrix ids parsed with `matrix_sdk::ruma::UserId::parse` (already a core dependency).
- `keeper-core/src/agents/zone.rs`:
  - `pub fn assess(listing: &[ZoneEntry], drive_toml: Option<&str>) -> ZoneAssessment`;
  - `ZoneEntry { name, is_dir }`;
  - `ZoneAssessment { drive: Result<DriveDecl, String>, homes: Vec<String>, hosts_nothing_because: Option<String> }`;
  - names beginning `_`, and `README.md` and `AGENTS.md`, are the zone's own and never homes.
- `keeper-core/src/vm.rs`: `FilesFolderRoles.agents_subfolder` (in the struct at `:4194`) and `FilesFolderRoleVm::Agents` (in the enum at `:4170`), so Files labels the zone.
- `keeper-core/src/org_account/manifest.rs`: `DriveRecord.agents: Option<String>` with `skip_serializing_if` (beside `:81`), and its mapping (beside `:492`). `keeper-core/src/org_account/state.rs`: `DriveOfferVm.agents` (beside `:111`).
- Shell, by inspection:
  - `keeper/src/sync_ipc.rs`: `SyncProfileVm.agents`/`agents_subfolder` (`:228-235`, `:301-305`'s rule), `SyncProfileReq.agents`/`agents_subfolder` (`:812-818`), the apply arm (`:1233-1250`'s rule), `agents_subfolder(req)` (`:1300-1306`), the browse roles (`:3711-3715`), and the test fixtures and the EXPRESSED list (`:5637-5639`, `:5678`, `:5901-5903`);
  - `keeper/src/forge_ipc.rs:516-517`;
  - `keeper/src/account_settings.rs:376`.
- Front:
  - `add-folder-form.tsx` (`:768-770`, `:817-819`, `:876-878`, `:946-948`, `:1861-1868`, and a switch beside `:2299-2307`);
  - `src/lib/stores/sync.ts` (`:483-541`, the role helper gains `"agents"`);
  - every `SyncProfileVm` fixture (e.g. `sync-pane.test.tsx:249`);
  - `dev/mock-shell.ts`'s profiles.
- `docs/agents.md` (new), chapter *The agents zone*: the layout, `_drive.toml`'s keys (`[integrity]` and its default among them, with the reason: an inbox's last author is the reader whose keeper synced it, not whoever wrote the words), the flag and why it needs `[folder.sessions]`. `docs/sync.md`'s list of folder-file keys gains `agents`.

**Acceptance:**
1. **The default and the empty table.** `agents_default_subfolder_is_80_agents_and_an_empty_table_means_on` (`profile/mod.rs`, modelled on `:2872-2891`): `{}` deserialises to `80-agents`, and the subfolder is trimmed.
2. **A subfolder that escapes is refused.** `an_agents_subfolder_that_leaves_the_profile_folder_is_refused`, modelled on `:2899`: `""`, `"   "`, an absolute path, `..`, `a/../..`, each a typed `SyncError::Config` naming the field.
3. **No zone overlaps another, in either direction.** `an_agents_zone_overlapping_another_zone_is_refused_both_ways`: inside and around the notes vault, the recordings root, the sessions zone, the tasks ledger and the voices bank, each refusal naming the other zone. `80-agents` beside `60-sessions` is accepted, which is tgdrive's layout (`/workspace/tgdrive/README.md:9-24`).
4. **Agents need sessions.** `an_agents_zone_without_a_sessions_zone_is_refused_naming_folder_sessions`. The sentence is "This folder keeps agents, so it needs a sessions zone: an agent's sessions live in this folder's sessions zone. Add [folder.sessions]."
   - The converse is proved too: removing `sessions` from a profile that keeps agents is refused with the same sentence. That is the rule's real risk: a save from the app that turns sessions off.
5. **Old rows load.** `a_profile_row_from_before_agents_loads_without_the_flag`: serde default, then round trip.
6. **The folder file can say it.**
   - `folder_field_rules_cover_every_profile_field` (`folder.rs:1061`) is green only with the new row; mutation: delete the row and it fails.
   - `an_empty_agents_table_turns_the_flag_on_with_its_default_subfolder` and `a_bad_agents_subfolder_is_refused_and_not_stored` are modelled on `:1431-1489`.
7. **The account round trip, on a real record.**
   - `account_settings.rs`'s record test (`:723-736`) gains `"agents": {"subfolder": "80-agents"}` and asserts `record.agents == Some("80-agents")`.
   - A device file carrying `agents = { subfolder = "80-agents" }` restores the flag through `account_restore`'s `drive_table`/`profile_of`, with no code change (D2 §3).
   - Shell, by inspection.
8. **`_drive.toml` is a contract.** `agents::drive::tests`:
   - the architecture's neuradrive example parses;
   - a typo `reader = [...]` is refused naming `reader`;
   - `version = 2` is refused: "written by a newer keeper";
   - `id = "TG_Drive"` is refused with the pattern;
   - `owner` not among the readers is refused;
   - empty readers, a duplicate, and `"marta"` (not a Matrix id) are refused;
   - `local_only = "yes"` is refused naming its type;
   - an unsorted `readers` is accepted, and `readers` is reported sorted;
   - **the untrusted zones** (A1): `an_integrity_table_replaces_the_default_untrusted_globs` — no `[integrity]` table gives exactly `["00-inbox/**", "70-comms/**", "recordings/**"]`; `untrusted = ["99-temp/**", "00-inbox/**"]` gives exactly those two, in the file's order; `untrusted = []` and a bare `[integrity]` give none. Mutation: merging a present table with the default fails it;
   - `integrity_refuses_unknown_keys_and_bad_globs`: `[integrity] trusted = []` is refused naming `trusted`; `untrusted = ["a/[b"]` is refused naming the pattern; `untrusted = "00-inbox/**"` and `integrity = 1` are refused naming their types.
9. **A zone without a valid declaration hosts nothing.** `assess` over the fixture listings `keeper-core/tests/fixtures/agents/zone-ok/` and `zone-no-drive/`:
   - `zone-ok` yields homes `["nixi", "tola-grey"]`, skipping `_skills`, `_template`, `_workflows`, `README.md` and `AGENTS.md`;
   - `zone-no-drive` gives `hosts_nothing_because = "This agents zone has no _drive.toml, so it hosts no agent. Write one naming the drive's readers."`, and the same for a refused declaration, with its sentence appended.
10. **Files names the zone.** The role test `the_vault_and_the_recordings_folder_come_from_configuration_not_from_a_name` (`vm.rs:10285`) gains `agents_subfolder: Some("80-agents")` and asserts `FilesFolderRoleVm::Agents`; `the_role_normalises_the_configured_subfolder_and_matches_only_the_folder_itself` (`:10397`) carries `agents_subfolder: None`.
11. **The form (UX-DR127).** In `add-folder-form.test.tsx`:
    - "This folder keeps agents" is present only while "This folder holds sessions" is on (AD-27: absent, not disabled);
    - the request carries `agents` and `agentsSubfolder` under the voices rules (`:1861-1868`);
    - a folder file that owns `agents` locks the switch with the folder-file sentence;
    - Rust's sentence from acceptance 4 is shown when the person turns sessions off while agents are on.
12. **Gates.** `bindings:check` is green on the rung. `bun run check:rust:macos` is green on hesperia.
13. **Smoke on hesperia.** A scratch folder synced by the installed build (never the owner's drives, C9) gets `[folder.agents]` in its `.keeper/keeper.toml`. Its Advanced settings show the flag as the folder file's. The Files view labels `80-agents`. Removing `[folder.sessions]` shows acceptance 4's sentence.
14. **The version floor is written down.** `docs/agents.md` § *The agents zone* states: "Requires keeper ≥ <the release carrying this rung> on every machine that syncs the drive. An older keeper refuses the whole `[folder]` table (`/workspace/tgdrive/.keeper/keeper.toml:6-10`)." The release notes of that version repeat it.

**Shell crate:** yes: `sync_ipc.rs`, `forge_ipc.rs`, `account_settings.rs`. `SyncProfileVm.ts`/`SyncProfileReq.ts` are generated from the shell, so they regenerate on the Mac only (research §14).

**binds:** FR-769, AD-361, UX-DR127

### 89.3 — An agent's home: `agent.toml`, `SOUL.md`, core memory caps, the system prompt

**Intent:** "i want bmad style personalities and different purposes (coding, exploring, designing, marketing, hr, psychologist etc)"; "i like hermes self improvement mehanism and continues memory". **Rung:** **epic89-home**. AD-360, AD-362, AD-363, AD-364, AD-396 (the consumer); FR-768 (its validation half), FR-770, FR-771, FR-772; NFR-118 (its share).

**Files:**
- `keeper-core/Cargo.toml`: `keeper-ported = { path = "../keeper-ported" }`. `sha2`, `hex`, `toml` and `unicode-normalization` are already core dependencies, so nothing else is added.
- `keeper-core/src/agents/home.rs`, the `agent.toml` grammar of *Data formats*:
  - `pub fn parse_agent_toml(text: &str, folder: &str, drive: &DriveDecl) -> Result<AgentConfig, HomeRefusal>`;
  - `AgentKind { Proxy, Steward, Specialist, Gate }`;
  - `pub const TOOL_VOCABULARY: &[&str]`: AD-397's closed list without the `mcp:` form, as R24 completed it, so `kvm_snapshot` and `kvm_act` are in it (F6) and an `agent.toml` naming them parses before epic 96 implements them (listed "not offered on this host");
  - `pub fn default_allow(kind) -> &'static [&'static str]`, with AD-397's kind defaults (below);
  - `BotRef { kind: ProviderKind, base: String, target: String }`, parsed from `bot:{kind}:{base}#{target}`, where `{base}` goes through `bots::url::parse_base_url` (`url.rs:120`: no userinfo) and is normalised as `settings_sync::normalize_base_url` (`:481`) does.
- `keeper-core/src/agents/soul.rs`:
  - `pub fn parse_soul(text: &str, agent_name: &str) -> Result<Soul, SoulRefusal>`, read with `notes::frontmatter::Frontmatter::parse`;
  - `Soul { name, title, icon, role, identity, communication_style, principles, persistent_facts: Vec<Fact>, body, ignored_keys }`, where `Fact` is `Text(String)` or `File(String)`;
  - `pub fn soul_from_bmad(merged: &toml::Table) -> Result<SoulImport, SoulRefusal>`, where `SoulImport { text, not_imported: Vec<String> }`, written with `Frontmatter::serialise_new` (`frontmatter.rs:258`).
- `keeper-core/src/agents/memory.rs`:
  - `pub const USER_CAP: usize = 1375; pub const MEMORY_CAP: usize = 2200;`;
  - `pub fn entries(body: &str) -> Vec<&str>` (split on a line holding only `§`);
  - `pub fn count(body: &str) -> usize` (Unicode scalar values of the body, frontmatter excluded);
  - `pub fn snapshot(user: Option<&str>, memory: Option<&str>) -> MemorySnapshot { user: Vec<String>, memory: Vec<String>, problems: Vec<MemoryProblem>, sha256: String }`.
- `keeper-core/src/agents/skills.rs`:
  - `pub fn index(found: &[(String /* dir */, String /* SKILL.md text */)], wanted: &SkillFilter) -> SkillsIndex { offered: Vec<SkillEntry { name, description }>, refused: Vec<(String, Vec<String>)>, warnings: Vec<String> }`;
  - frontmatter is turned into `keeper_ported::agentskills::MetaValue`s.
- `keeper-core/src/agents/prompt.rs`:
  - `pub fn compose(input: &PromptInput<'_>) -> ComposedPrompt`;
  - `PromptInput { soul, facts: &[RenderedFact], memory: &MemorySnapshot, skills: &SkillsIndex, menu: &[MenuItem], frame: &SessionFrame, context: Option<&ContextBundle> }`;
  - `SessionFrame { agent, host, session_path, session_kind, drives: Vec<(String, String)>, audience: Vec<String>, now: DateTime<FixedOffset> }`;
  - `ComposedPrompt { text, sections: Vec<PromptSection { slot: u8, title: &'static str, range: Range<usize> }>, prompt_sha256, memory_sha256 }`;
  - `pub fn told(&ComposedPrompt) -> AgentToldVm` (ts-rs exported): "what the agent was told", consumed by `keeper-agentd status --session` (90.5) and the agent room (91.1).
- `keeper-sync/src/files_write.rs`:
  - `pub fn with_agents(mut self, agents_subfolder: Option<&str>) -> Self`, beside `with_sessions` (`:408`);
  - a new `WriteRefusal::AgentHome` whose sentence is "That is an agent's home file. Only a person edits it, in the drive itself."
- Shell, by inspection: `keeper/src/bots_tools.rs:424-433` arms `.with_agents(profile.agents.as_ref().map(|a| a.subfolder.as_str()))` after `.with_sessions(…)` (C6).
- Fixtures: `keeper-core/tests/fixtures/agents/zone-ok/` holds `_drive.toml` (tgdrive, readers `@tgorka`), `nixi/agent.toml`, `nixi/SOUL.md`, `nixi/USER.md` (exactly 1375 scalars), `nixi/MEMORY.md` (exactly 2200), and `_skills/` with three valid skills and two refused ones (`My_Skill/`, `mismatch/`). The golden is `tests/fixtures/agents/nixi-told.md`.
- `docs/agents.md`, chapters *An agent's home* and *What an agent is told*.

**Kind defaults** (AD-397's rule, except stewards, which follow AD-389 (C8), pinned as a table):
- `proxy`: the five drive reads, `drive_search`, `delegate`, `reply`, the five `surface_*`, `journal_append`, `memory_propose`, `skill_propose`, `skills_list`, `skill_view`.
- `steward` (C8, AD-389): `specialist`'s, plus `delegate`. AD-389 also names `card_update` and `workflow_start`, which the specialist's set already holds (F18). No surface tools.
- `specialist`: the drive reads, `drive_search`, `session_write`, `card_update`, `workflow_start`, the four `bmad_*`, `helper`, and the five memory tools.
- `gate`: `reply`, `delegate`, `journal_append`.
- A name in the vocabulary that this build does not implement is accepted and not offered. The host lists it as "not offered on this host" (90.5).

**Acceptance:**
1. **`agent.toml` is a contract** (`agents::home::tests`, mutation-proved).
   - **The architecture's Nixi example, and its variant** (F17). The architecture's example names `bot:openai:…`, and the `openai` kind lands in 89.6, a later rung of this epic. So 89.3's fixture `nixi/agent.toml` is that example with one change, named in the fixture's first comment line: `[model].bot` is `bot:ollama:…`. It parses; `bot:openai:…` is refused naming the kind until 89.6. From 89.6 on the architecture's example parses verbatim (89.6, acceptance 8), so at the end of this epic "the architecture's example parses" is literally true.
   - An unknown key is refused naming it (`tool = […]`).
   - `id` must equal the folder name (`id = "nixie"` inside `nixi/` is refused, naming both), and `_nixi` is reserved for the zone.
   - `name` ≠ `SOUL.md`'s `name` is refused.
   - `human` is required for `proxy`, refused for `steward`, and refused when not among the drive's readers.
   - `[model].bot` with userinfo (`https://u:p@host`), or with an empty target, is refused.
   - `local_only = true` with kind `hermes` is refused.
   - A drive with `local_only = true` makes every agent's `local_only` effective, and refuses a non-`ollama` bot naming the drive's key.
   - `[tools].allow` holding `mcp:paseo/x` is refused ("MCP tools are named by [tools].mcp"), and so is `shell`, naming the vocabulary.
   - `[[menu]]`: a duplicate `code`, `code = "x"`, both `workflow` and `prompt`, or neither, are each refused.
   - Every `[limits]` and `[memory]` bound is refused one past the edge and accepted on it: `rounds_per_turn` 0 and 9; `hop_limit` 4; `nudge_user_turns` 4 and 51, with 0 accepted.
   - Two homes in one zone with one `matrix_user` are refused, naming both.
2. **A kind chooses defaults, never powers.** `a_kind_only_chooses_defaults`:
   - each kind without `[tools].allow` gets exactly its default set;
   - a `gate` with an explicit `allow` of the drive reads gets exactly those;
   - nothing in `AgentConfig` depends on `kind` beyond the defaults and `human`.
3. **The same name in two drives is two agents.** `amelia_in_two_drives_is_two_agents`: the same `amelia/` home parsed against the tgdrive and neuradrive declarations yields two configs, keyed `(drive id, agent id)`, with different audiences.
4. **`SOUL.md`** (`agents::soul::tests`).
   - The architecture's Dr Tola Grey example parses.
   - An unknown frontmatter key is kept and listed in `ignored_keys`.
   - A missing `role` is refused naming it.
   - Bounds: `title` of 65 characters, `principles` of 17 items, and a 281-character principle are refused. The file is capped at 16 KiB (16 385 bytes is refused, with the size in the sentence).
   - An `identity: |` block scalar is refused, naming `identity` and the parser's `Unparsed` reason, with "write it as a double-quoted string; `\n` starts a new line" (DW-357). A double-quoted `identity` holding `\n` escapes reads back with its line breaks (`unescape_double`, `frontmatter.rs:725`), and `soul_from_bmad` writes it that way through `quote_double` (`:1011`). A scalar is single-line by construction (`:54-55`), so the escapes are the only multi-line form.
   - `persistent_facts: ["file:../tgdrive/secret.md"]` and `"file:{project-root}/x.md"` are refused: a `file:` fact names a path inside the agent's own home only. Literal text is accepted.
5. **A BMAD agent becomes a soul** (89.1's first consumer). `winston_imports_as_a_soul`, over 89.1's merged Winston:
   - the text is golden (`tests/fixtures/agents/winston-SOUL.md`) and parses back through `parse_soul` to the merged `name`, `title`, `icon`, `role`, `identity`, `communication_style` and `principles`;
   - `not_imported` names `activation_steps_prepend`, `activation_steps_append` and each `[[agent.menu]]` item (`CA → skill bmad-architecture`, `IR → skill bmad-sprint-planning`), because menus land with workflows (DW-356);
   - a `file:{project-root}/…` persistent fact is listed as not imported.
6. **Core memory caps** (`agents::memory::tests`, mutation-proved).
   - The fixture `USER.md` (1375) and `MEMORY.md` (2200) are within the caps.
   - One more scalar puts the file over its cap: the snapshot leaves that file out, and the problem lists its entries with "USER.md is 1376 characters; the cap is 1375. Shorten it; keeper does not cut it for you." (C3).
   - The cap counts scalars, not bytes: 1375 `ł`s (2750 bytes) are within it.
   - Frontmatter is not counted.
   - `§` inside a line is text, not a separator.
   - A duplicate entry and an entry holding U+202E, U+200B, U+2066 or U+FEFF are refused, naming the entry's number and the code point.
7. **The snapshot is frozen.** `memory_sha256` is the SHA-256 of the snapshot's canonical text (entries joined by `\n§\n`, `USER.md` first). The same files always give the same digest; one changed entry gives another. The freeze across a running session is 90.5's (acceptance 8 there).
8. **Skills are validated, refused ones listed and never offered.** `agents::skills::tests` over the fixture `_skills/`:
   - three offered;
   - `My_Skill` refused with agentskills' "must be lowercase" and "invalid characters";
   - `mismatch` refused with "must match skill name";
   - a `SKILL.md` without frontmatter refused with "Missing required field in frontmatter: name";
   - a body over 500 lines warned, not refused;
   - `[tools].skills = ["web"]`, when `_skills/web` is absent, is listed "named in agent.toml, not in _skills/";
   - `"*"` offers all valid skills;
   - a `SKILL.md` over 256 KiB is refused with its size.
9. **The prompt's order is fixed.** `a_home_composes_to_the_golden_prompt`. The fixture home, frame and context compose byte for byte to `nixi-told.md`:
   - slot 1 is the soul's fields in AD-363's order, then its body and its sentence facts; a `file:` fact's file is not in slot 1 but follows `FILE_CONTENT_IS_DATA` in slot 5, headed `--- home file: <path> ---` (review R-2);
   - slot 2 is `USER.md`, then `MEMORY.md`;
   - slot 3 is the skills, by name and description only, with no body;
   - slot 4 is the menu;
   - slot 5 is the frame: `nixi@electra`, the session path and kind, the drives, "What you read here may be shown only to: tgorka.", the time, and `FILE_CONTENT_IS_DATA` (`bots/tools.rs:153-155`);
   - slot 6 is the context files under `UNTRUSTED_PREAMBLE` (`context_files.rs:97-102`), through `ContextBundle::system_prompt` (`:199`).
   - Mutation: swapping two slots fails the test. So does dropping `FILE_CONTENT_IS_DATA`.
10. **The digest is of what was sent.** `prompt_sha256` is the SHA-256 of `text`'s UTF-8. `told()`'s sections, concatenated, equal `text`, so "what the agent was told" and what the model received cannot differ.
11. **Agents' home files are fenced from every tool writer.** `an_agents_fence_refuses_every_home_file_and_leaves_the_rest_alone` (`files_write.rs`):
    - `drive_write`/`drive_edit` routes to `_drive.toml`, `_skills/x/SKILL.md`, `_workflows/w/workflow.toml`, `_template/agent.toml`, `nixi/agent.toml`, `nixi/SOUL.md`, `nixi/USER.md`, `nixi/MEMORY.md`, `nixi/journal/2026-10-02.electra.md` and `nixi/proposals/01J….md` are refused with `AgentHome`, case-folded as the workspace fence is (`:599-633`);
    - `80-agents/README.md` and `80-agents/AGENTS.md` are routed as before; every path inside a home (`nixi/notes.md`, a `file:` fact's target, `nixi/drafts/SOUL.md`) is refused with `AgentHome`, and so is a link that lands inside a home (review R-2, R-19);
    - a scope built without `.with_agents` refuses nothing new.
12. **Gates.** `bun run check:rust:macos` is green on hesperia, with the one-call shell arm (C6). Smoke there: a ⌘9 bot with a write grant on a scratch drive asked to edit `80-agents/nixi/SOUL.md` gets the `AgentHome` sentence as its tool result.

**Shell crate:** yes, one call (`bots_tools.rs:424-433`; C6). Everything else is keeper-core, keeper-sync and keeper-ported.

**binds:** FR-768, FR-770, FR-771, FR-772, NFR-118, AD-360, AD-362, AD-363, AD-364, AD-396

### 89.4 — Labels: readers and integrity

**Intent:** "make sure the sensitive part goes only to the private bots/drives (nixi needs to be told to use what drive context - but can multiple)". **Rung:** **epic89-home**. AD-390; FR-773; NFR-115 (its share). Enforcement at every sink (`check_sink`, declassification) is 92.1/92.6's (AD-391), not this story's.

**Files:**
- `keeper-core/src/agents/label.rs`:
  - `pub enum Integrity { Untrusted, Agent, Peer, Owner }`, with `Ord` derived in that order;
  - `pub enum Readers { Anyone, Only(BTreeSet<OwnedUserId>) }`;
  - `pub struct Label { pub readers: Readers, pub integrity: Integrity, pub local_only: bool }`. `local_only` (A2, S-04) is set by reading a `local_only` drive or agent home; serde writes the key only when it is true, and a missing key reads `false`;
  - `pub fn join(&self, other: &Label) -> Label`: readers ∩ (`Anyone` is the identity), integrity min, `local_only` OR;
  - `pub fn may_reach(&self, audience: &Readers) -> bool`: audience ⊆ readers, the primitive `check_sink` will use;
  - `pub fn may_use_model(&self, local: bool) -> bool`: `!local_only || local`. A model provider is a processor the person chose, not an audience (S-04, D-34), so it is not checked against `readers`; while `local_only` holds, only a model that runs locally may see the content. 90.5 asks it before every model request, and 92.1/92.6 make it the `Sink::Model { local }` row of `check_sink`;
  - `Label::opening(home: &DriveDecl, requester: Integrity) -> Label`, carrying the home drive's `local_only`;
  - `pub fn sentence(&self, name: &dyn Fn(&UserId) -> String) -> String`, which feeds 89.3's frame (`SessionFrame.audience`) and adds "It may be sent only to a model that runs locally." while `local_only` holds.
- The input labellers, pure:
  - `label_drive_read(drive: &DriveDecl, facts: &ReadFacts) -> Label`, where `ReadFacts { path, last_author: Author, okf_human_reviewed: Option<bool>, okf_external_source: bool }` and `Author` is `Reader(OwnedUserId)`, `Agent` or `Unknown`. **The integrity rule** (A1, S-02): `untrusted` when `path` matches one of the drive's `untrusted` globs (89.2; a pattern that does not compile counts as matching, so an unreadable zone fails low), when the file's OKF `sources` cite an `http(s)` URL, or when the last author is a person outside the readers; otherwise `owner` for a reader, `agent` for an agent or an author keeper cannot name; an OKF `human_reviewed: false` lowers it to `agent`. Readers and `local_only` are the drive's;
  - `okf_label_facts(text) -> OkfLabelFacts { human_reviewed, external_source }`, read with keeper's own OKF reader (`notes/okf.rs`);
  - `label_person_message(sender, session_person, room_readers) -> Label`;
  - `label_agent_message(session_label) -> Label`;
  - `label_outside() -> Label`.
- The `label` line body: `LabelBody { readers, integrity, cause: LabelCause { kind, reference } }`, used by 89.5.
- `LabelVm { readers: Vec<String>, integrity: String, sentence }` (ts-rs exported) for the chip (91.1).
- `docs/agents.md`, chapter *Who may read what an agent read*. It says that `owner` integrity means "committed by a reader's keeper", not "written by that person" (S-31): a file a reader's keeper synced may hold anyone's words, which is why the untrusted zones exist.

**Acceptance:**
1. **The lattice laws, exhaustively.** `the_label_lattice_holds_over_every_label`. Over the universe `{@tgorka, @marta, @x}`, the readers are `Anyone` and the 8 subsets; with 4 integrities and both values of `local_only` that gives 9 × 4 × 2 = 72 labels. Every pair and triple is checked (no property-testing crate):
   - join is commutative, associative and idempotent;
   - `Anyone` with `Owner` and `local_only` false is the identity;
   - the empty set with `Untrusted` and `local_only` true is absorbing.
2. **A join never widens** (NFR-115's share; the risk is a summary that carries a private byte wider). `a_join_never_widens_readers_or_raises_integrity`: for all 72², the join's readers ⊆ each side's, its integrity ≤ each side's, and its `local_only` ≥ each side's; whatever the join may reach, each side could reach. Mutation: a union instead of an intersection fails it, and so does an AND for `local_only`.
3. **`may_reach`.** neuradrive's opening label `{marta, tgorka}` may reach `{tgorka}` and `{marta, tgorka}`, and may not reach `{tgorka, x}` or `Anyone`. tgdrive's `{tgorka}` may not reach `{marta, tgorka}`.
4. **The input labellers, over real OKF files.** `labels_follow_who_wrote_a_file`:
   - a tgdrive file whose last author is `@tgorka` gets tgdrive's readers and `Owner`;
   - a fixture note whose OKF frontmatter says `human_reviewed: false`, read with keeper's own OKF reader (`notes/okf.rs`), is lowered to `Agent`;
   - a file whose last author is unknown, or an agent, gets `Agent`, never `Owner` (fail low); one whose last author is a person outside the readers gets `Untrusted`.
   - **The untrusted zones** (A1, S-02): `integrity_zones_and_web_sources_are_untrusted_whoever_wrote_them` — `00-inbox/forwarded.md` and `70-comms/thread.md`, last written by `@tgorka`, are `Untrusted` with tgdrive's readers; a note whose OKF `sources` name an `https://` URL (`notes/clipping.md`) is `Untrusted`; with the drive's `untrusted` emptied, the inbox file is `Owner` again; a pattern that does not compile makes every read `Untrusted`. Mutation: dropping the zone check fails it.
5. **`local_only` travels with the label** (A2, S-04). `local_only_travels_with_the_label_and_gates_the_model`: a session opened on a `local_only` drive may use a local model and not a remote one; a read from that drive carries `local_only`; a tgdrive session that joins that read may no longer use a remote model.
6. **Messages.** A message from the session's own person gets `{sender ∪ room readers}` and `Owner`. One from another reader gets `Peer`, and one from anyone else `Untrusted`. An agent's message carries its session's label. Outside content gets `Anyone` and `Untrusted`.
7. **Serde shapes are stable, because a log is read by other hosts.**
   - `{"readers":["@marta:h","@tgorka:h"],"integrity":"owner"}`, with readers written sorted, and no `local_only` key while it is false.
   - `{"readers":"*","integrity":"untrusted"}`, and `{"readers":"*","integrity":"untrusted","local_only":true}` when it holds.
   - Unknown integrity words, `"readers":"all"`, a reader that is not a Matrix id and a duplicate reader are refused.
8. **The prompt says it.** The sentence is "What you read here may be shown only to: Marta, tgorka." for neuradrive and "…may be shown to anyone." for `Anyone`, with " It may be sent only to a model that runs locally." appended while `local_only` holds. 89.3's golden is regenerated through `Label::sentence`, not a literal.
9. **Control metadata is not labelled** (the architecture's Ambiguity 7, accepted). `docs/agents.md` says that run state, claims, presence and manifests carry no content and are not labelled. The schemas that guarantee "no content" are in 90.6 (acceptance 8 there).

**Shell crate:** no.

**binds:** FR-773, NFR-115, AD-390

### 89.5 — A session an agent works in: `agent.toml`, the chunked log, replay with tool steps, the `.keeper/` index

**Intent:** "Use sessions in the drives as a working place of the agent with all the data he needs … the message history and actions taken - so the session can be used after. - also after sync by drive the work can be continued on other device … (data is all he needs) - but make sure its fast to operate." **Rung:** **epic89-session**. AD-365, AD-366; FR-774, FR-775; NFR-116, NFR-117 (its share).

**Files:**
- `keeper-core/src/agents/session.rs`, the session `agent.toml` grammar of *Data formats*:
  - `pub fn parse_session_agent_toml(text) -> Result<SessionAgent, SessionRefusal>`;
  - `pub fn compose_session_agent_toml(&SessionAgent) -> String`, written once by the session runtime (90.2's create, 90.5's first use);
  - room ids parsed with `ruma::RoomId::parse`, and `label` as 89.4's `Label`; `kind` is `main | conversation | delegated | scheduled | workflow | gate` (R25 added `conversation`, a proxy conversation the person started; `main` is the proxy's DM only).
- `keeper-core/src/agents/log/mod.rs`:
  - `LogLine { v, id: Ulid, parent: Option<Ulid>, ts: DateTime<Utc>, host: HostSlug, epoch: u64, claim: Option<String>, kind, matrix_event: Option<OwnedEventId>, body }`, serialised in the documented key order `v, id, parent, ts, host, epoch, claim, kind, matrix_event, body` (struct order through `serde_json`), `ts` RFC 3339 with milliseconds and `Z`. **`claim`** (A3, S-05) is the claim state event's id the writing host held; with `epoch` it is the fence key, so two hosts that both believe they won epoch N are told apart. Lines written before claims exist (90.5's `epoch: 0`) carry none;
  - `LineBody`, one variant per *Data formats* kind, with exactly its fields; `tool_call.args` is `arguments_raw` (C5);
  - `HostSlug` (`[a-z0-9-]{1,32}`);
  - `ChunkName { date, host, n }`, with `Display` and `FromStr` for `YYYY-MM-DD.<host>.<n>.jsonl`, where `n` is unpadded and compared as a number.
- `keeper-core/src/agents/log/writer.rs`:
  - `pub fn rotate_at(lfs_threshold_bytes: u64) -> u64 { min(192 * 1024, lfs_threshold_bytes * 3 / 4) }`;
  - `ChunkWriter::open(session_dir: &Path, host: &HostSlug, rotate_at: u64, today: NaiveDate) -> Result<ChunkWriter, LogError>`;
  - `append(&mut self, line: &LogLine) -> Result<AppendReceipt { chunk, offset }, LogError>`. **Before it serialises**, `append` passes every free-text field through `redact_secrets` (A4, S-17): an `assistant` line's text, a `tool_result`'s content, and a `user` or `peer` line's text. What the model said, what a person said and what a tool returned reach the file only redacted;
  - `append_line(&mut self, raw: &str)`, the primitive Epic 95's journal writer reuses. It writes what it is given; its caller scans;
  - `sync(&mut self)`.
- `keeper-core/src/agents/redact.rs` (A4, S-17), pure:
  - `pub fn redact_secrets(text: &str) -> Redacted { text, found: Vec<Redaction { kind, sha256 }> }` over a closed, tested pattern set: Matrix access tokens (`syt_…`), Anthropic keys (`sk-ant-…`), OpenAI-style keys (`sk-…`), GitHub tokens (`ghp_…`, `gho_…`, `github_pat_…`), PEM private-key blocks, AWS access key ids (`AKIA…`), Slack tokens (`xoxb-…`, `xoxp-…`), JSON Web Tokens (`eyJ….….…`) and PostHog keys (`phx_…`, `phc_…`);
  - each match becomes `[REDACTED secret-like: sha256:<the first 12 hex digits of the secret's SHA-256>]`, so the line still says something was there and the same secret seen twice is recognisably the same;
  - every token pattern starts at a word boundary, so `task-…` is never an `sk-` key; if the set ever fails to compile, the whole text is withheld behind one marker rather than written unchecked;
  - a secret whose shape is outside the set is written as it came: DW-430.
- `keeper-core/src/agents/log/reader.rs`:
  - `pub fn read_session(session_dir: &Path) -> SessionLog { lines, problems, conflicted }`. Two `acquired` claim lines at one epoch with different claim event ids (A3, S-05) mean two hosts both believed they held the session: `conflicted` is set and a problem names both events. The same claim event logged twice is not a conflict;
  - `pub fn hydrate_blob(session_dir, sha256) -> Result<serde_json::Value, LogError>`.
- `keeper-core/src/agents/log/replay.rs`:
  - `pub fn replay(log: &SessionLog, blobs: &dyn Fn(&str) -> Result<Value, LogError>) -> Result<Replay { messages: Vec<ChatMessage>, last_open: Option<OpenBody> }, ReplayRefusal>`, refusing a conflicted log until a person resolves it (90.6 says how the session shows it);
  - `pub fn message_for(line: &LogLine) -> Option<ChatMessage>`: the one function the turn loop (90.5) and replay both use to turn a line into a message, so replay equals what was sent by construction, except where a secret was redacted.
- `keeper-core/src/agents/index.rs`:
  - `Index::open(zone_root)` opens `<zone>/.keeper/agents.db` (rusqlite, WAL, `PRAGMA user_version`; a mismatch drops and rebuilds, D-21);
  - `rebuild(...)`, `apply(session, &LogLine, &AppendReceipt)`;
  - queries `session(path)`, `sessions()`, `cards(session)`, `seen(session, event_id)`.
  - Tables:
    - `sessions` (path, id, agent, drive, kind, title, room, label, scope, run, claim_host, claim_epoch, lines, last_ts);
    - `chunks` (session, name, host, n, bytes, last_offset);
    - `cards` (session, rel, run, assignee, host, requested_by, schedule, last_run, workflow), read from card frontmatter as `sessions::pool` reads `fields`;
    - `seen_events` (session, event_id). Its writer is 90.5; its reader is the dedupe.
- `keeper-core/src/sessions/template.rs`: the flat session's `AGENTS_MD` gains one paragraph ("`log/` and `approvals/` are keeper's: the session's record and its pending actions. Never edit or delete them by hand."). `the_navigation_file_states_the_load_bearing_rules` (`:1176-1211`) gains `log/` and `approvals/` among its required strings.
- `docs/agents.md`, chapters *A session an agent works in*, *The log*, *What a session costs the drive*. *The log* says that every line's free text is scanned for secret shapes before it is written, which shapes, what the marker looks like, and what is not caught (DW-430): a session folder is as sensitive as the drives it reads (D-31). `docs/sessions.md`'s layout section names the agent files.

**Acceptance:**
1. **Session `agent.toml` is a contract.**
   - The keys and bounds of *Data formats* (`kind` enum, `title` ≤ 120, `hop` 0–3, `parent` table shape, a room id) are each refused one past the edge, naming the key.
   - An unknown key is refused.
   - `compose_session_agent_toml` followed by `parse` round-trips.
2. **One write per line, and the torn tail is the host's own only.** `a_torn_tail_of_the_hosts_own_chunk_is_truncated_on_open_and_the_rest_reads`: a real file ending in half a line, reopened by its own host, is truncated to the last `\n`; the next append reads cleanly. `another_hosts_torn_tail_is_left_alone_and_skipped`: hesperia's half line is untouched by electra's writer, and the reader skips it with a problem entry. (Real files, NFR-117.)
3. **Rotation keeps chunks out of LFS** (NFR-116).
   - `rotate_at(4 MiB) = 192 KiB`, and `rotate_at(256 KiB) = 192 KiB` (tgdrive's threshold; §12.8), and `rotate_at(128 KiB) = 96 KiB`.
   - `no_chunk_ever_reaches_rotate_at`: 10 000 lines of realistic sizes leave every chunk under `rotate_at`, numbered `1…n` with no gap.
   - `a_utc_date_change_starts_a_new_chunk`: the next day's first line opens `<date>.<host>.1.jsonl`.
4. **Blobs.** `a_body_over_16_kib_becomes_a_blob_written_before_its_line`:
   - the blob `log/blobs/<sha256>.json` exists and is `fsync`ed before the line is written;
   - the line's body is `{"blob":"<sha256>","bytes":n}`;
   - the hash is of the stored bytes;
   - a second identical body reuses the blob.
   - A line still over 64 KiB after blobbing is refused, never written.
5. **One writer, never a symlink.** `the_writer_refuses_a_symlinked_log_directory`: `log` → `/tmp/x` is refused. A chunk name with another host's slug is never opened for append.
6. **Merge, the epoch fence and the claim fence.** `two_hosts_chunks_merge_by_ts_host_id` (two real hosts' chunks interleave in order). `a_superseded_epochs_late_lines_are_dropped`:
   - electra writes epoch 1, and hesperia's `claim acquired` epoch 2 lands at T;
   - electra's epoch-1 line at T + 5 s is dropped with a "newer epoch" problem, and its line at T − 5 s is kept;
   - mutation: removing the fence keeps the late line, and the test fails.
   - **Two winners of one epoch** (A3, S-05): `a_double_acquire_at_one_epoch_is_a_conflict_and_replay_refuses` — electra's `acquired` epoch 2 with claim `$a` and hesperia's `acquired` epoch 2 with claim `$b` mark the log `conflicted`, name both events, and `replay` refuses it; the same claim event logged twice by one host is not a conflict. Mutation: comparing epochs only fails it.
7. **Replay reproduces the wire exactly** (FR-774; the risk is a resumed session that loses its tool trace, `bots_ipc.rs:1275-1284`). `replay_reproduces_the_request_body_byte_for_byte` (`keeper-core/tests/agents_log.rs`), over a turn whose text holds no secret:
   - a real tool loop, `bots::tools::run_tool_loop_reporting` (`tools.rs:1215`), runs against the crate's local stub provider (the server `tests/bots_tools.rs` uses), with two rounds and three drive-tool calls, one of them refused;
   - its `ToolCallReporter` (`:1204`) and round events append lines through `ChunkWriter`;
   - a fresh `read_session` and `replay` of those files, prefixed by the same system message, give `bots::chat::build_body(kind, …)` equal, byte for byte, to the body the stub received for the last request;
   - mutation: dropping `tool_calls` or a `Role::Tool` message from replay fails it.
8. **Every kind round-trips.** One line of each *Data formats* kind serialises in the documented key order (`claim` after `epoch`, absent when there is none), and parses back equal.
   - An unknown `kind` and a `v: 2` line are skipped with a problem entry, never a panic.
   - `compact` replaces the lines through `replaces_through` with one system message headed "Summary of the earlier part of this session".
9. **The index answers without the logs.** `the_index_answers_with_the_logs_gone` (NFR-116's hot-path half):
   - `rebuild` over the fixture zone `keeper-core/tests/fixtures/agents/sessions/` (two hosts' chunks, a blob, a card with `run: blocked`, `assignee: amelia`);
   - then `log/` renamed away;
   - `session()` and `cards()` still answer the label, scope, run, claim host and epoch, line count and last activity, and the card's fields.
   - `apply` after an append changes exactly that session's row. A `user_version` mismatch rebuilds rather than erroring.
10. **Agent files do not disturb the flat contract.** The `AGENTS.md` pinning test passes with the new paragraph. `.jsonl`, `blobs/` and `approvals/*.json` are not markdown and never enter the pool (the scan half of this is proved in 90.2, acceptance 8, after the scan moves out of the shell).
11. **What a session costs the drive** (AD-366's measurement; kept repeatable, F23). `a_ten_thousand_line_session_costs_the_drive` in `keeper-core/tests/agents_log_growth.rs`, `#[ignore = "measurement: writes a 10 000-line session into a scratch git repo; run with --ignored --nocapture"]`: a 10 000-line session is written through `ChunkWriter` with one commit per 20-line turn, in a scratch git repository, using the system `git` and the default threshold.
    - It prints chunk count, largest chunk, largest line, and the repository's size before and after `git gc` (from `git count-objects -vH`), and asserts only the writer's own bounds (every chunk under `rotate_at`, no line over 64 KiB), because the sizes are git's.
    - `docs/agents.md` records the printed numbers and the run command: `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --test agents_log_growth -- --ignored --nocapture`.
    - If growth is quadratic rather than proportional to the log, DW-359's revisit trigger fires and the story says so; DW-359 is re-checked by re-running the same test.
12. **No secret reaches the log** (A4, S-17). `a_secret_never_reaches_the_log_and_replay_differs_exactly_at_it` (`keeper-core/tests/agents_log.rs`): the acceptance-7 turn, with a GitHub token in a drive file the tool returns. The stub provider received the token (the model saw it); no chunk under `log/` contains its bytes; replay equals the sent body with the token replaced by its marker, and differs nowhere else. Mutation: removing the writer's redaction fails it.
13. **The pattern set is closed and tested** (`agents::redact::tests`, mutation-proved): `every_pattern_in_the_set_is_replaced_by_its_marker` (one sample of each kind, the marker's 12 hex digits being the secret's own SHA-256 prefix); `ordinary_text_is_left_byte_for_byte` (`task-abcdefghijklmnopqrstuvwxyz`, `ask-me-anything-…` and other look-alikes are untouched, with nothing found); `two_secrets_are_each_replaced_and_the_same_secret_gets_the_same_marker`.

**Shell crate:** no.

**binds:** FR-774, FR-775, NFR-116, NFR-117, NFR-120 (the line-level fence), AD-365, AD-366

### 89.6 — A generic OpenAI-compatible provider (`openai` kind; CLIProxyAPI)

**Intent:** "it could connect to model provider cliproxy like this omp". **Rung:** **epic89-session**. AD-369; FR-776; NFR-121 (its share). D-4 unchanged: there is no default base URL, and the endpoint is the owner's. AD-146's closed set gains its revisit-trigger member.

**Files (D3 inventory A, every site):**
- **Compile-forced, keeper-core.**
  - `bots/mod.rs:69` gains `OpenAi`, wire `"openai"`, documented as "any OpenAI-compatible endpoint: bearer-authenticated, the model in the body", with arms in `as_registry_str` (`:82-87`) and `from_registry_str` (`:97-103`). `Endpoint::url`'s prefix arm (`:364-367`; `Endpoint::new` is `:337`) gives no prefix (the `_` arm).
  - `quirks.rs:208-237` gains the row `embeddings: Unknown`, `tool_choice: Yes`, `remote_image_url: Unknown`, `image_part: Object`, `context_window_over_v1: No`, `done_sentinel: Yes`, `stream_usage: Unknown`, `reasoning_fields: ["reasoning_content", "reasoning"]` (a gateway in front of other providers, Hermes' reasoning), `keepalive_secs: None`, `named_events: No`, `server_sessions: No`. The smoke may change `done_sentinel`/`stream_usage` from what it observes (acceptance 9).
  - In `discover.rs`:
    - `health_route` (`:117-120`) returns `/v1/models`;
    - `models` (`:198-201`) dispatches to a new `openai_models` that parses `data[].id`, leaving `vision`/`tools`/`reasoning` `None` (AD-151);
    - `probe_bot` (`:220-223`) checks membership in `/v1/models`;
    - `enumerate_bots` (`:252-259`) returns `Enumerated`;
    - `status_sentence` (`:728-737`) gains "The endpoint refused keeper's key ({status}). Check the key this provider was saved with.";
    - a new `pub fn probes_model_capabilities(kind) -> bool`, an exhaustive match (Ollama `true`; Hermes and OpenAi `false`), used by both silent sites below.
  - `grant.rs:544-555`: `(OpenAi, Some(true))` and `(OpenAi, None)` take Ollama's semantics (offered; the warning when unknown), and `(OpenAi, Some(false))` is absent.
  - `commands.rs:380-385`: `Some(OpenAi) => ctx.model_tools != Some(false)`.
  - `voice_target.rs:161-164`: `OpenAi => None`.
- **Silent sites get explicit decisions.**
  - `keeper/src/bot_task.rs:134` and `keeper/src/bots_ipc.rs:1150` (shell, by inspection) replace `== ProviderKind::Hermes` with `!discover::probes_model_capabilities(kind)`. For OpenAi this skips a probe whose answer is always `None` (`/v1/models` carries no capabilities) and reaches the same offer with the unknown-tools warning. Hermes is unchanged.
  - `src/components/bots/bot-grant-bar.tsx:149` becomes a `switch (provider.kind)` with a `satisfies never` default, so the next kind is compile-forced in TypeScript too. OpenAi falls through to `model.tools` (`:153`).
  - `dev/mock-shell.ts:5520-5521` accepts `"openai"`, and gains an `openai` provider fixture beside `:2391`/`:2405`.
- **Fail-closed decodes.**
  - `bots/store.rs:448` (`UnknownProviderRow`), `account_ipc.rs:3559-3564` and `account_restore.rs:721-722` decode through `from_registry_str`, so they learn the word with no code change.
  - `store.rs`'s `a_provider_of_an_unknown_kind_is_surfaced_rather_than_dropped` (`:1144-1175`) keeps `omp` as its unknown kind.
  - The shell's tests gain an `openai` offer and an `openai` device record.
- **Decided as they stand** (D3 inventory A, each checked):
  - `commands.rs:415-418`'s `_ => None` (an OpenAi provider gets no Hermes note);
  - `chat.rs`'s `build_body`, `build_message` and `Reassembler::new` (`:216-217`, `:292-293`, `:582-584`), which read `quirks(kind)`;
  - `embed.rs:149-154`, which reads the embeddings quirk (DW-360);
  - `egress.rs:163-251`, `settings_sync.rs:558-584`, `manifest.rs:124-125` and `device_state.rs:65-66` (opaque strings);
  - `chat.rs:42`'s `CHAT_PATH` and `embed.rs:10`'s `EMBEDDINGS_PATH`, both already OpenAI-shaped;
  - `bot-slash-menu.ts:174`, `:217`, `:224`, `bots-pane.tsx:505` and `bots-phone-pane.tsx:612`, which pass the kind through;
  - the kind-literal test fixtures (`bot-grant-bar.test.tsx`, `bots-pane.test.tsx`, `bots-phone-pane.test.tsx`, `bot-grants-section.test.tsx`, `bot-composer.test.tsx`, `bot-slash-menu.test.ts`, `test/account-fixture.ts:91`), unchanged except where an acceptance below adds an `openai` case.
- **UI.** `src/components/settings/bots-section.tsx:201`'s `KINDS` gains `"openai"`, so the third toggle renders the stored word (`:927-936`), and `:642`'s one-tap test includes it. The kind-naming copy (`bot-empty-state.tsx:52`, `bot-grant-bar.tsx:18-20`, `:71-72`, `bot-paste.ts:57`, `bot-picker.tsx:10`, `:22`, `bot-session-list.tsx:165`) names three kinds.
- **Generated.** `src/lib/ipc/gen/ProviderKind.ts` widens on regeneration (from keeper-core; Linux can regenerate it).
- **Tests and docs.** `tests/bots_discover.rs` gains `openai` stub cases. `docs/egress.md` and every doc listing the two kinds name three. The `ProviderKind` doc at `mod.rs:56-59` records that AD-146's trigger was met by CLIProxyAPI.
- **The live test.** `keeper-core/tests/bots_openai_live.rs`, `#[ignore = "live: needs KEEPER_OPENAI_SMOKE_BASE_URL and KEEPER_OPENAI_SMOKE_TOKEN_FILE (CLIProxyAPI, ruling R13)"]`.

**Acceptance:**
1. **Every compile-forced arm decides.**
   - `a_provider_kind_round_trips_and_an_unknown_kind_is_refused` (`mod.rs:639`) iterates three kinds, and `omp` is still refused (DW-214 keeps it closed).
   - The quirks tests (`quirks.rs:244-294`) cover OpenAi: `tool_choice` permitted, `image_part` `Object`, no context window over v1, and the keepalive under the read timeout.
   - `grant_offer` tests (`grant.rs:891-921`) gain the three OpenAi rows.
   - The `commands` tests (`tests/bots_commands.rs`) gain `/grant` offered for OpenAi with tools unknown.
   - `provider_default(OpenAi) == None` (`voice_target.rs:354-413`'s style).
2. **Discovery against a stub** (`tests/bots_discover.rs`, real HTTP on localhost):
   - health issues `GET /v1/models` and nothing else (the request log asserts `["GET /v1/models"]`);
   - `models` parses a real CLIProxyAPI-shaped `data[]` (`{"id":…,"object":"model","owned_by":…}`) with capabilities `None`;
   - `probe_bot` finds a listed id and reports an unlisted one as absent;
   - `enumerate_bots` is `Enumerated`;
   - a 401 gives `Unauthorized` and the OpenAi sentence;
   - **no path gains a `/p/` prefix.**
3. **The wire body.** `build_body(OpenAi, …)` keeps `tool_choice` (`"required"`) and sends an image as `{"image_url":{"url":…}}` (`chat.rs:1540-1593`'s style).
4. **The silent sites decide, by a test.** `probes_model_capabilities` is exhaustive. A shell test (by inspection) shows `arm_turn` (`bots_ipc.rs:1150`) sends no discovery request for an OpenAi bot with a grant, and offers the drive tools with `TOOLS_CAPABILITY_UNKNOWN`. In `bot-grant-bar.test.tsx`, an `openai` provider with `tools: null` shows the grant affordance with the unknown-tools warning, and Hermes still shows its refusal.
5. **The UI selects it.** In `bots-section.test.tsx`, three kind toggles render the stored words, and saving `openai` sends `kind: "openai"`. An account offer of kind `openai` is one-tap where the account is usable. An `omp` offer still shows "This version of keeper cannot talk to a omp provider." (`:585`).
6. **The account round trip.** `openai` provider records survive `manifest.rs`/`device_state.rs` (opaque strings) and are restored by `account_restore.rs:721` (shell test, by inspection).
7. **No new destination** (NFR-121). An egress test (`egress.rs`) with an `openai` provider at `https://provider.example:8452` gives exactly one "AI provider" row for that host, and nothing else.
8. **The agent reference.** `bot:openai:https://provider.example:8452#<model>` parses in 89.3's `agent.toml` (acceptance 1 there), and so does the architecture's Nixi example verbatim (F17).
   - **No test names the real endpoint** (S-20): unit tests and fixtures use the reserved `provider.example` host, and the live test reads the endpoint from `KEEPER_OPENAI_SMOKE_BASE_URL`. A grep of `src-tauri/crates`, `src` and `dev` for CLIProxyAPI's URL (its tailnet host with port `8452`) finds nothing; today `egress.rs`'s test is the one site that must change.
9. **Smoke: CLIProxyAPI, live** (ruling R13; the risk is protocol conversion behind the proxy, §11.1 and §13 #28). Run from this container on the tailnet, with the base URL R13 names:
   `KEEPER_OPENAI_SMOKE_BASE_URL=<CLIProxyAPI's base URL> KEEPER_OPENAI_SMOKE_TOKEN_FILE=$HOME/.omp/cliproxyapi.token cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --test bots_openai_live -- --ignored --nocapture`.
   The test proves each of these against the real endpoint:
   - `health` is `Online` through `GET /v1/models`;
   - `models` is non-empty, and the chosen model (the first chattable id, or `KEEPER_OPENAI_SMOKE_MODEL`) is listed;
   - `probe_bot` finds it and does not find `keeper-no-such-model`;
   - a streamed chat with `tool_choice: "required"` through `run_tool_loop` makes at least one `drive_read` call against an in-memory `ToolHost` returning a fixed file, and the final answer quotes it: tool calls survive CLIProxyAPI's conversion;
   - `[DONE]` and `usage` are observed or not, and the run prints which, so the quirks row's `done_sentinel`/`stream_usage` are set from evidence;
   - a wrong token gives `Unauthorized` and the OpenAi sentence;
   - **the token never appears** in captured `tracing` output or in any error (the captured output is searched for the token's text);
   - **Ambiguity 10:** `usage.prompt_tokens` for 89.3's golden prompt (memory at its caps, three skills) is printed and recorded in `docs/agents.md`, against the Ollama LXC's `OLLAMA_CONTEXT_LENGTH=16384` (G5 §6; DW-358).
10. **Smoke: hesperia** (`bun run check:rust:macos` green first). On the installed build:
    - Settings › Bots adds a provider of kind `openai` at the CLIProxyAPI URL with its token;
    - *Test* reads Online;
    - a model is pinned from the listed models;
    - a ⌘9 chat with a read grant on a scratch drive answers with a `drive_read` tool step shown in the pane;
    - the egress list shows the host once.

**Shell crate:** yes: `bot_task.rs:134`, `bots_ipc.rs:1150`, and the tests of `account_ipc.rs`/`account_restore.rs`. Both silent sites move to `keeper-agent` in 90.1 with their decision intact.

**binds:** FR-776, NFR-121, AD-369

## Names other epics cite

Settled with Epic 91–93 (E2) and Epic 94–95 (E3) on 2026-10-02:
- `keeper_core::agents::{drive, zone, home, soul, memory, skills, prompt, label, session, log, index}`;
- `ChunkWriter::append_line`, which Epic 95's journal writer reuses;
- `AgentToldVm`, `LabelVm`, `Label::may_reach` (92.6's `check_sink` builds on it), `Label::may_use_model` (90.5's per-request check; 92.1's `Sink::Model`);
- `keeper_core::agents::redact::redact_secrets` (89.5's writer applies it; Epic 95's secret scan reuses its pattern set);
- `LogLine.claim` and `SessionLog.conflicted` (90.6's claims write and honour them);
- `keeper_ported::bmad::config::{structural_merge, merge_layers, load_toml, load_customization}`. 94.1 adds `load_central_config` and the rest.

Not this epic's:
- `skills_list`/`skill_view` handlers (94.2);
- `keeper-ported::hermes` (95.1, which moves 89.3's read-side primitives behind it with 89.3's tests unchanged);
- keeping an agent-proposed skill out of the index (S-12): 95.2 makes `skills::index` skip a skill whose frontmatter carries `metadata.keeper_proposal` until a person adopts it by removing that key;
- `check_sink` (92.1/92.6);
- `agents new`/`agents init` (91.5).

## What stays out

- **Enforcing labels at sinks, declassification** (AD-391): 92.1 and 92.6.
- **Writing memory, journals and proposals**: 95.1. **Consolidation**: 95.2.
- **Workflows and menus that run them**: Epic 94. The menu is parsed and shown in the prompt; nothing runs it.
- **Any host, Matrix, turn or room**: Epic 90.
- **A UI for agents**: Epics 91 and 92. This epic ships the VMs (`AgentToldVm`, `LabelVm`) and the folder switch only.

Deferred, with the ledger entries this epic owns, as committed in `_bmad-output/implementation-artifacts/deferred-work.md` (DW-355…DW-360, DW-430):

```markdown
### DW-355: Naia, the shared agent of round 2, is not seeded.

origin: ARCHITECTURE-AGENTS.md § What stays out (2026-10-02); owner round 3, "naia - omit for now"
location: `_bmad-output/planning-artifacts/epic-89-an-agent-is-a-file-in-your-drive.md` (the zone and home grammars a Naia home would use); story 91.5 (the seeded roster)
reason: The owner named Naia in round 2 and asked to omit it in round 3. makistack knows `naia` only as Marta's storage share (`maia/naia`, digest G5 §6), so there is no persona to seed. Revisit when the owner describes Naia: it is a home under a drive's `80-agents/` like any other, with no new mechanism.
status: open

### DW-356: A BMAD agent's menu and activation steps are not imported into an agent.

origin: epic 89's plan, 2026-10-02 (story 89.3, `soul_from_bmad`)
location: `src-tauri/crates/keeper-core/src/agents/soul.rs` (`soul_from_bmad`, `SoulImport.not_imported`); `src-tauri/crates/keeper-ported/src/bmad/config.rs`
reason: The import writes BMAD's persona fields into `SOUL.md` and lists `activation_steps_prepend`/`_append` and each `[[agent.menu]]` item as not imported. A BMAD menu item names a BMAD skill (`skill = "bmad-architecture"`), and keeper's `[[menu]]` names a folder under `_workflows/`, which exists only from Epic 94 on. Writing a menu item that points at nothing would offer the person a command that cannot run. Revisit in 94.3: map `skill = X` to `workflow = X` when `_workflows/X/` exists, and list the rest.
status: open

### DW-357: A soul written with YAML block scalars (`identity: |`) is refused.

origin: epic 89's plan, 2026-10-02 (story 89.3)
location: `src-tauri/crates/keeper-core/src/notes/frontmatter.rs:16-23` (the subset: no `|` or `>`); `src-tauri/crates/keeper-core/src/agents/soul.rs`
reason: `SOUL.md` is read with keeper's own frontmatter subset, which records a block scalar as unparsed rather than guessing it. A scalar is single-line by construction (`frontmatter.rs:54-55`), so a multi-line `identity` must be a double-quoted string with `\n` escapes, which the subset reads (`unescape_double`, `:725`) and `soul_from_bmad` writes (`quote_double`, `:1011`). A person writing a soul by hand in Obsidian may reach for `|`. The refusal names the key and says how to write it. Revisit if the owner writes souls by hand and finds this a burden: teach the subset literal block scalars (`|`), keeping the byte-preserving write rule.
status: open

### DW-358: A composed prompt is not checked against the model's context window.

origin: epic 89's plan, 2026-10-02 (story 89.3; the architecture's Ambiguity 10; research §13 #48)
location: `src-tauri/crates/keeper-core/src/agents/prompt.rs` (`compose`); `src-tauri/crates/keeper-core/src/bots/discover.rs` (no context-window capability is read)
reason: A sensitive agent pins a local model (AD-377), and the Ollama LXC runs with `OLLAMA_CONTEXT_LENGTH=16384` (digest G5 §6). A soul, capped memory, skills and context files may not fit, and a model given a truncated prompt fails quietly. 89.6's smoke records the golden prompt's `prompt_tokens`, but no story refuses a turn whose prompt cannot fit, because no provider kind reports a context window keeper can trust (`context_window_over_v1: No` for every kind). Revisit when the first `local_only` agent runs: read Ollama's `/api/show` context length and refuse the turn with the numbers, never truncate.
status: open

### DW-359: An archived session's log chunks are kept as they were written.

origin: epic 89's plan, 2026-10-02 (story 89.5; AD-366's measurement)
location: `src-tauri/crates/keeper-core/src/agents/log/writer.rs`; `src-tauri/crates/keeper-core/tests/agents_log_growth.rs` (the measurement, `#[ignore]`); `docs/agents.md` § What a session costs the drive
reason: Git stores every committed version of a growing chunk. The 192 KiB bound and pack deltas are expected to keep the cost proportional to the log [INFERENCE], and 89.5's `#[ignore]` test `a_ten_thousand_line_session_costs_the_drive` measures a 10 000-line session to check; re-running it (`cargo test -p keeper-core --test agents_log_growth -- --ignored --nocapture`) re-checks this entry. Archiving a session moves its folder and changes no chunk, so a long session keeps its many committed versions in history forever. Revisit if the measurement shows growth worse than proportional, or a drive's repository grows past the owner's budget: at archive, write the session's chunks into one immutable `log/archive.jsonl.zst` blob and remove the chunks in the same commit.
status: open

### DW-360: Embeddings through an `openai` provider are not probed.

origin: epic 89's plan, 2026-10-02 (story 89.6; AD-369)
location: `src-tauri/crates/keeper-core/src/bots/quirks.rs` (the OpenAi row: `embeddings: Unknown`); `src-tauri/crates/keeper-core/src/bots/embed.rs:149-154`
reason: CLIProxyAPI's route list has no `/v1/embeddings` (§11.1), and another OpenAI-compatible endpoint may have one. The row is `Unknown`, so the notes index may be pointed at such a provider and fail with the endpoint's 404 rather than a sentence before it starts. A probe would add a request no other kind needs. Revisit when the owner points the notes index at an `openai` provider: probe `POST /v1/embeddings` once at *Test*, and record Yes or No per provider.
status: open

### DW-430: A secret whose shape is outside the closed pattern set is logged as written.

origin: security review S-17, accepted by ruling R28 (2026-10-02); epic 89's plan, story 89.5 (amendment A4)
location: `src-tauri/crates/keeper-core/src/agents/redact.rs` (the closed pattern set); `src-tauri/crates/keeper-core/src/agents/log/writer.rs` (`ChunkWriter::append`, which applies it)
reason: The log writer replaces every match of nine secret shapes (Matrix, Anthropic, OpenAI-style, GitHub, Slack and PostHog tokens, AWS access key ids, JSON Web Tokens and PEM private keys) before a line is written. A password in prose, a bearer token of another shape, a database URL with its credentials, or a secret split across two tool results matches none of them, and reaches the chunk, the drive's history and every device that syncs the session. A wider net (entropy, `key=value` guesses) would also redact the ordinary hashes and ids that replay and the person need, so the set stays closed and tested. Revisit when a secret of a new shape is found in a session log: add its pattern to `redact.rs`, with a sample that must be caught and a look-alike that must stay untouched.
status: open
```

## The failure shape this epic must not repeat

**A home an agent can rewrite.** AD-362's whole point is that a soul, tools and memory are written by people (and Epic 95's consolidator), never by a turn an untrusted page steered. A review that finds any of the following is a blocker:
- a tool scope built for a bot or agent without `.with_agents`;
- a path into `80-agents/` composed in TypeScript;
- `SOUL.md` or `agent.toml` written by anything but a person's action (`soul_from_bmad` returns text; it writes nothing).

**A log that lies on replay.** A review that finds any of the following is a blocker:
- a line written through `drive_write` or the session verbs;
- `args` stored parsed instead of raw;
- a message built for the wire anywhere but `message_for`;
- a reader that truncates another host's chunk;
- the index read for anything the log says differently after a rebuild;
- a line whose free text reached the file without `redact_secrets` (A4);
- a fence that compares epochs without the claim event id, or a conflicted log replayed (A3).

**A label that widens.** A review that finds any of the following is a blocker:
- a union where a join is meant;
- a default of `Owner` for a file whose author is unknown;
- a read from an untrusted zone, or of a file citing the web, labelled above `untrusted` (A1);
- a join that drops `local_only` (A2);
- readers unsorted on write.

**A kind decided by accident.** A review that finds any of the following is a blocker:
- a `kind == ProviderKind::X` comparison added anywhere (an exhaustive `match` or `probes_model_capabilities` decides);
- a default base URL for `openai`;
- the CLIProxyAPI token in a log line, a test fixture or a commit, or its URL in a test fixture (S-20).

## Sprint-status entry

The epic's entries are in `_bmad-output/implementation-artifacts/sprint-status.yaml` (the `epic-89` block). Its deferred items, DW-355…DW-360 and DW-430, are in `_bmad-output/implementation-artifacts/deferred-work.md`.

## Stack rungs

Rungs by layer, as in epics 80–88. Each compiles alone (skill: prove each stack rung stands alone).
1. **`epic89-home`**, on top of the plan rung:
   - keeper-ported (89.1);
   - the `[folder.agents]` flag with its shell and front hunks (89.2);
   - `keeper_core::agents::{drive, zone, home, soul, memory, skills, prompt, label}` (89.3, 89.4);
   - `WriteScope::with_agents` and its one shell arm (89.3);
   - `docs/agents.md`'s first chapters.
   - 89.1 rides with 89.3 because a port lands with its first consumer (P14).
   - `bindings:check` and `check:rust:macos` must be green on this rung alone.
2. **`epic89-session`**:
   - `keeper_core::agents::{session, log, index, redact}` and the `AGENTS.md` paragraph (89.5);
   - the `openai` kind at every site, with its shell, front and mock-shell hunks (89.6);
   - the live CLIProxyAPI test.
   - The quirks row and the TS union change together, so the regenerated `ProviderKind.ts` and `KINDS` ride this rung.
