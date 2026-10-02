---
name: 'keeper'
type: research
topic: 'an agents system inside keeper — named agents whose homes are files in the drives, sessions that are folders and continue on another device, agents that hand work to each other, actions that wait for a person on any device, BMAD-format workflows, memory that improves itself under privacy labels, tools and computer use up to a KVM, on-device turn-taking voice, the iPhone and an Android tablet'
decision: 'what an agent is and where its files live; what an agent session is and who writes it; how agents, hosts and people talk (Matrix only, or Matrix plus a hub); where the turn loop runs (in the desktop app, and in a headless Linux daemon per principal); how a dangerous action waits for a person and resumes; how workflows, memory, knowledge harvest and privacy labels work; which upstream code is ported to Rust and where it lives; which provider kind CLIProxyAPI needs; which voice option keeps D-5'
status: final
created: '2026-10-02'
run_folder_note: 'the digests below were read as files of the coordinating session (local://digest-*.md) and stay outside the repository; the coordinator''s pins and rulings are committed at _bmad-output/planning-artifacts/agents-coordinator-decisions-2026-10-02.md and the epic map at _bmad-output/planning-artifacts/agents-program-map-2026-10-02.md'
digests:
  - R1 — agent products and harnesses — Hermes Agent, OpenClaw, Grok Bot, Grok Build and grok-cli, DeepSeek Harness (dsh), pi and oh-my-pi (omp), LangChain deepagents and LangGraph, OpenAI Codex CLI, goose, Violoop — their documentation, repositories and incident write-ups, plus the crates.io API for the codex and goose crates, read 2026-10-01 (`local://digest-R1AgentProducts.md`)
  - R2 — the Rust in-process agent stack for Tauri 2 on macOS, iOS and Linux — crates.io API (`https://crates.io/api/v1/crates/<name>`), GitHub API (`https://api.github.com/repos/<owner>/<repo>`), each crate's own Cargo.toml, README and source, and web search for iOS platform rules only; nothing compiled; read 2026-10-01 (`local://digest-R2RustBuildingBlocks.md`)
  - R3 — computer-use model APIs (Anthropic, OpenAI, Gemini, UI-TARS), macOS, Linux and iOS host automation, IP-KVMs (NanoKVM-Go, NanoKVM, PiKVM, JetKVM, GL.iNet Comet), Violoop, approval patterns in agent frameworks, safety research and incidents, read 2026-10-01 (`local://digest-R3ComputerUseKvmApprovals.md`)
  - R4 — CLIProxyAPI (repository, v8.0.5–v8.0.7 release notes, `config.example.yaml`, `internal/api/server_routes.go`, issue tracker) and similar proxies, the providers' terms, launcher UX (macOS 26 Spotlight and App Intents, Raycast, Alfred, iOS entry points), iOS background limits, file-based multi-agent coordination (beads, Claude Code agent teams, Backlog.md, Taskmaster, git-bug, CRDT libraries), read 2026-10-01 (`local://digest-R4ProxyUxDevices.md`)
  - R5 — realtime voice — hosted speech-to-speech APIs, open and self-hostable models, cascaded-pipeline practice, Rust voice crates (crates.io, 2026-10-01), on-device recognition per platform, MatrixRTC and Element Call, read 2026-10-01 (`local://digest-R5RealtimeVoice.md`)
  - R6 — self-improving memory (Hermes Agent source at `main@663362680b`, OpenClaw memory-core, Letta, mem0, Zep/Graphiti, Claude Code, Codex, the Anthropic memory tool, omp, agentskills.io), memory attacks, information-flow control (Design Patterns, CaMeL, FIDES, SPA), multi-principal isolation guidance, read 2026-10-01 (`local://digest-R6MemoryAndPrivacy.md`; five table rows of the digest file are cut at 768 characters and are quoted only as far as they run)
  - R7 — Matrix as the agent bus — matrix-rust-sdk, the Matrix spec, tuwunel and Synapse configuration, MSC4471 and MSC2477, prior-art Matrix bots, push gateways, device-tool protocols (ACP, AG-UI, MCP 2026-07-28, OpenAI Realtime), Tauri 2 on Android, read 2026-10-01 (`local://digest-R7MatrixAgentBus.md`)
  - G1 — keeper's bots subsystem as built — providers, the wire, discovery, timeouts, conversations, the tool loop, grants and audit, bot tasks, the voice target, embeddings, a gap table and the ADs a new system must obey (path:line, 2026-10-01) (`local://digest-G1KeeperBots.md`)
  - G2 — the sessions zone and its board, tasks and schedules, sync latency, notes search, OKF, device identity (path:line, 2026-10-01) (`local://digest-G2KeeperSessionsTasks.md`)
  - G3 — prior art in the operator's infrastructure repository (makistack) — Nixie on OpenClaw and its move to Hermes (epic 22, stories 22-2…22-8), the OpenClaw security findings, the Paseo broker, the observability and evals research, keeper's own research-ai-chat, OKF on the drives, the lessons paid for (`local://digest-G3PriorArtMakistack.md`)
  - G4 — the BMAD method 6.12 as installed — persona and workflow formats, the execution model, orchestration, what a runtime must provide, read from the plugin cache and `_bmad/`, with `resolve_customization.py`, `resolve_party.py` and `_resolve_replacements` run in-process (`local://digest-G4BmadWorkflowFormat.md`)
  - G5 — sessions promote, voice duplex, echo and barge-in, Android, the notes view, Matrix in keeper, the makistack hosts and homeservers (`local://digest-G5KeeperFacts.md`)
  - D1 — the bots runtime and its extraction into `keeper-agent` — today's call graphs, the two platform ports, matrix-sdk 0.18 in keeper-core, the extraction design, guard scripts, risks, with `cargo tree --offline --locked` run for Linux (`local://digest-D1RuntimeExtraction.md`)
  - D2 — the sessions runtime for agents — create and archive end to end, the board vocabulary, the `[folder.sessions]` flag recipe, writes from an agent, sync wake and the pull gap, adding a `TaskKind` (`local://digest-D2SessionsForAgents.md`)
  - D3 — every `ProviderKind` / `"hermes"` / `"ollama"` site in Rust and TypeScript, and on-device model inference today (`local://digest-D3ProviderAndVoiceSites.md`)
  - C1 — house formats and numbering ceilings (epic 88, AD-359, FR-766, NFR-111, UX-DR126, DW-354, D-30) and existing names a new plan must not collide with (`local://digest-C1Conventions.md`)
context:
  - the coordinator's pins and rulings, `_bmad-output/planning-artifacts/agents-coordinator-decisions-2026-10-02.md` — the owner's three rounds verbatim, pinned decisions P1–P15, rulings R1–R13 when this was written and R14–R29 since (a ruling supersedes a pin where they differ)
  - the program map, `_bmad-output/planning-artifacts/agents-program-map-2026-10-02.md` — epics 89–99, their story ids and order, the numbering ceilings
  - house examples followed — `research-transcription-2026-09-28.md` (this format), `architecture/architecture-keeper-2026-07-03/ARCHITECTURE-BOTS.md` (the AD format), `docs/decisions.md` D-27…D-30 (the D-entry format)
---

# Research — Agents in keeper: one door, homes in your drives, sessions that move between machines

**Evidence grades.** `[SOURCE]` = an external primary source, read on 2026-10-01 or 2026-10-02 by a research lane
(R1–R7) and cited with its publisher and URL. `[REPO]` = read in this worktree (branch `agents-plan`); line numbers
are as the grounding lanes (G1–G5, D1–D3, C1) recorded them on 2026-10-01/02, except the lines §1.7 lists as re-read
in this pass. A `[REPO]` fact about the owner's drive is read in the `/workspace/tgdrive` mirror, and a fact about
the operator's infrastructure is G3's or G5's reading of makistack. `[INFERENCE]` = reasoning over cited facts, with
no source of its own; a lane's inference keeps the lane's name (`[INFERENCE, R2]`). `[UNVERIFIED]` = looked for and not
established; never to be repeated as fact. §14 lists every one of those and what was tried.

**How to cite this document.** Sections are numbered `§N.M` and are stable. Cite as
`research-agents-2026-10-02.md §6.7`. Inside this document a research digest is always cited with a section or a
heading (`R7 §2`, `R1 Hermes`); the coordinator's rulings are written `ruling R1`…`ruling R13`, and the pinned
decisions `P1`…`P15`, so the two R-series never meet.

**What this document is not.** It does not design the program. `ARCHITECTURE-AGENTS.md` and the epics 89–99 do, and
the coordinator pinned P1–P15 and rulings R1–R13 before this was written. This document is the evidence under them. Rulings R14–R29 came after — L1's contradictions, the epics' open questions, and the security and consistency reviews — and every row or point here that one of them settled or superseded says so in place (**Settled by ruling Rn** or **Superseded by ruling Rn**), so a reader of this record alone is not misled.
Where the evidence pulls against a pin, or a pin changes something keeper already decided, it says so in place as a
`> Coordinator note` (§3.10, §5.10, §6.7, §7.9, §8.8, §11.6); §1.6 lists every open point and points at them.

---

## 0. Reading guide

| Pin or ruling | What it rests on | Sections |
| --- | --- | --- |
| **P1** — names and roles: Nixi the only door, Dixi, Dr Tola Grey, Dr Lucyna Novak, BMAD specialists one per home drive; Naia omitted; heavy and light is a property of tools | the owner's three rounds; Hermes profiles and story 22-8's roster; OpenClaw's and Grok Bot's isolation statements | §1, §3.4, §9.6 |
| **P2 + ruling R1** — an agent is a home in zone `80-agents/`; "agent", not "bot" | the drive zone layout; the `bots` name already taken; the voices flag recipe; Hermes and OpenClaw homes; R6's file layout | §1.6, §2.5, §4.4 (pattern 8), §9.4, §12.7, §12.13 |
| **P3 + rulings R3, R4** — a session is the `60-sessions` flat contract plus agent files; the chunked log | dsh's event log; Grok Build's per-session directory; pi's JSONL; no append and no `.jsonl` through the drive writers; the LFS threshold; the unsafe executor | §4.4, §11.5, §12.5, §12.8 |
| **P4** — Matrix only; no hub, no listening socket | R7's verdict and its hybrid; tuwunel's and Synapse's limits; MSC4471's state; DW-215 | §6 |
| **P5 + rulings R6, R7, R8, R10** — hosts: the app in process, `keeper-agentd` per principal, phones as clients | D1's call graphs, ports and risks; D-3's asymmetry; iOS background limits | §11.4, §12.1–§12.4 |
| **P6** — claims and placement | Kleppmann on fencing; Hermes' claims; R4's coordination findings | §4.4, §11.5, §13 |
| **P7** — delegation is a session | the owner's round 2; OS-Blind; story 22-8's room caps; Grok Bot's hand-off; R6's delegation rule | §3.4, §4.2, §7.5, §9.5 |
| **P8** — privacy: process per principal, grants, labels | R6 B4–B5 and its label design; G3's lessons | §3.9, §9.5–§9.7 |
| **P9 + ruling R12** — approvals: tiers, the immutable record, a verified device decides | R3's taxonomy and record; R1 patterns 4, 5, 9, 10; keeper's blocking approver | §7.6, §7.7, §7.9 |
| **P10** — a third provider kind `openai` | D3's inventory A; G1 §1; R4 §1; D-4's revisit trigger | §11.1, §11.2, §12.11 |
| **P11 + ruling R9** — BMAD workflows as written; scheduled work on cards | G4; D2 §6 | §10, §12.10 |
| **P12** — Hermes-style memory with OpenClaw's gates | R6 A1–A3 | §9.1–§9.4 |
| **P13** — voice, the private option (D-5 kept) | R5; D3's inventory B; G5 §2; D-5 | §8 |
| **P14** — Rust ports in `keeper-ported` | R6 (c), G4 §6, R5 (c), R3 §3, the licence firewall | §5.10, §9.4, §10.6 |
| **P15** — platforms: APNs through a self-hosted gateway, the phone's board, Android | R4 §3, R7 §4 and §6, G5 §3 | §11.4, §11.6 |
| **ruling R2** — run state is its own key | D2 §2 | §12.6 |
| **ruling R5** — the doorbell is `Engine::pull_now` | D2 §5, G2 §3 | §12.9 |
| **ruling R11** — matrix-sdk stays at 0.18 | R7 §1, D1 §3 | §6.1, §12.3 |
| **ruling R13** — smoke endpoints | G5 §6, R4 §1, R7 §2 | §6.2, §11.1 |
| **rulings R14–R29** — barge-in pause-first, `run`'s D-entry, epic 22 scoped, neuradrive's own push, adaptive edits, names, D-5's amendment, licences, the KVM raise, the 60 KiB answer; the epics' readings (R24–R27); the two reviews (R28, R29) | the plan's lanes and the reviews `agents-review-security-2026-10-02.md`, `agents-review-consistency-2026-10-02.md` | marked in place in §1.6, §2.1, §2.5, §3.10, §6.7, §7.6, §7.7, §7.9, §8.8, §9.4, §11.1, §11.2, §12.4, §12.6, §13, §14 |

---

## 1. The ask

### 1.1 Round 1 (2026-10-01), excerpts, verbatim

> i want to explore topic of creating an agents system in keeper - similar to grok bot or hermes bot for work with having different sessions (active/archived), different persons will hav diffrent tools to use (and/or access to local drive or notes or use the keeper itself or the whole computer) - it could connect to model provider cliproxy like this omp. there will be one main person (like nixi for me) to talk to on everyday basis

> i want bmad style personalities and different purposes (coding, exploring, designing, marketing, hr, psychologist etc)

> This persons could connect to each other (sent a message, or use the kaban board - look grok bot, hermes)

> Use sessions in the drives as a working place of the agent with all the data he needs, scripts he needs to use etc logs but also the message history and actions taken - so the session can be used after. - also after sync by drive the work can be continued on other device that will sync the data in sessions (data is all he needs) - but make sure its fast to operate.

> I want also the scheduled job on the kaban to work on (tasks on keeper).

> In the future I want to include kvm (like NanoKVM-Go+) to use other devices as a connectors.

> I want to have connectors for dangerous actions before proceed (to review data in sessions to proceed after programaticly). - style like in Violoop

> I dont want sidecar. I want my mac, iphone, sever on linux to use it.

### 1.2 Round 2 (2026-10-01), excerpts, verbatim

> nixi etc are everywhere - because session is sync - they can have tags (like nixi with electra or hesperia tag) to know what materialization of nixi is used. but for example electra (because is always on) can configue work of hesperia once this one is goes off.

> i would prefer to communicate and cooperate and delegate work for different bots instead of sub-agents - to avoid confustion and make one point of true - also want to make suere its fast

> want also some heavy bots (like language server, debugger tools for coding) and other light, some bots can be shared (marta, tgorka - neruadrive - naia) and some might be only for tgorka (nixi) or marta (dixi)

> The data from sessions can be used after to update the knowledge in the main drive (or drives)

> I want support for workflows definitions - i want to support what bmad method have in the workflow ... but also quick free speak model with no workflow that can triger and being proxy between real human and the whole agentic system (the same proxy bot can be between non hyman but other system or service with sync/async matter)

> nixi will alsways be a proxy between tgorka and rest of the system

> i like hermes self improvement mehanism and continues memory

> As a person i want to use my assistant first (nixi) and the note view - nixi can operate and we both can see explain or write over notes view.

> you can assume ios will have paid apple account in the future. also make sure it will be working on my android tablet

> i own the server infrastructure and tgrive is only for tgorka - make sure the sensitive part goes only to the private bots/drives (nixi needs to be told to use what drive context - but can multiple)

### 1.3 Round 3 (2026-10-02), verbatim

> - Dr Lucyna Novak instead of Dr Lucyna Nowak
> - Dr Tola Grey instead of Teo
> - **Private option** (keeps D-5 - yes for the option
> - for now skip drawing - later state will decide
> - memory, skills, soul, etc bot data find a right place in the drives fro this files (tgdrive i neuradrive)
> - rewriting to rusr recommended parts (add separate source module)
> - naia - omit for now
> Rest plan the bmad and proceed with the implementation (bmad research, architectire, epics etc then review and implement one after another one)
> create prs on the stack using gh stack for all the work

### 1.4 What each request became

| The owner's words | Read as | Pinned by | Evidence |
| --- | --- | --- | --- |
| "similar to grok bot or hermes bot" | the products to learn from, not to embed | — | §4 |
| "different sessions (active/archived)" | keeper's sessions zone, whose status already *is* the folder (`active/` or `archive/<year>/`) | P3 | §2.5, §12.5 |
| "diffrent tools … local drive or notes or use the keeper itself or the whole computer" | tools per agent in `agent.toml`; drive grants; surface tools in the notes view; a sandboxed `run`; MCP; the Mac's screen; a KVM | P2, epics 91, 96 | §2.3, §7, §5.5 |
| "connect to model provider cliproxy like this omp" | a third provider kind for a generic OpenAI-compatible endpoint | P10 | §11.1, §12.11 |
| "one main person (like nixi for me)" … "nixi will alsways be a proxy" | Nixi, the only door, with no workflow of its own | P1 | §1.6 item 1 |
| "bmad style personalities and different purposes" | `SOUL.md` carrying BMAD's persona fields; BMB-built marketing, HR and psychologist agents | P1, P2, P11 | §10.1 |
| "connect to each other (sent a message, or use the kaban board …)" | delegation opens a session in the target's home; the existing board gains a run badge | P7, ruling R2 | §4.4, §12.6 |
| "Use sessions in the drives … continued on other device … make sure its fast" | the session folder plus an append-only chunked log, one writer per file; claims; a doorbell for the pull | P3, P6, rulings R3, R5 | §11.5, §12.8, §12.9 |
| "the scheduled job on the kaban" | a card's `schedule:` evaluated on agent hosts | ruling R9 | §12.10 |
| "kvm (like NanoKVM-Go+)" | protocol encoding in `keeper-ported::nanokvm`, behind T4 | P14, story 96.5 | §7.3 |
| "connectors for dangerous actions … style like in Violoop" | prepare/commit approvals with a durable pending record | P9 | §7.4, §7.6, §7.7 |
| "I dont want sidecar. I want my mac, iphone, sever on linux" | the turn loop in process in the desktop app; `keeper-agentd` as its own host on Linux; the phone a client | P5 | §11.4, §12.4 |
| "nixi with electra or hesperia tag … electra … can configue work of hesperia once this one is goes off" | a copy is one Matrix device per (agent, host); claims expire and the always-on host takes over | P5, P6 | §6.1, §11.5 |
| "delegate work for different bots instead of sub-agents … one point of true" | delegation is a session owned by the target, never a sub-agent | P7 | §4.2, §7.5 |
| "heavy bots … and other light … shared … only for tgorka (nixi) or marta (dixi)" | heavy is a property of the tools an agent's work needs; audience is a set of human readers | P1, P8 | §9.7 |
| "update the knowledge in the main drive" | harvest into OKF notes `human_reviewed: false`, then the promote panel | P12, story 95.5 | §2.5, §3.8 |
| "workflows … what bmad method have … also quick free speak model with no workflow … proxy … sync/async" | BMAD skills as written, plus a `workflow.toml` header; a gate agent for other systems | P11, epic 99 | §10 |
| "hermes self improvement mehanism and continues memory" | caps, frozen snapshots, proposals, nudges, nightly consolidation, weekly curator | P12 | §9.1–§9.4 |
| "my assistant first (nixi) and the note view - nixi can operate and we both can see" | Nixi's DM docked beside the notes view, with presence and surface tools | stories 91.2, 91.3 | §2.11, §6.5 |
| "ios will have paid apple account … my android tablet" | D-1 reopened for APNs; an Android platform start | P15 | §11.4, §11.6 |
| "tgrive is only for tgorka … sensitive part goes only to the private bots/drives (nixi needs to be told to use what drive context - but can multiple)" | process per principal; labels of readers and integrity; drives in scope chosen per session | P5, P8 | §9.5–§9.7 |
| Round 3: the names, "Private option", no drawing, "a right place in the drives", "rewriting to rusr recommended parts", no Naia | Dr Lucyna Novak and Dr Tola Grey; on-device voice keeping D-5; no Excalidraw; zone `80-agents/` in tgdrive and neuradrive; a separate crate of ports; Naia omitted | P1, P13, the scope guards, P2 / ruling R1, P14 | §8.1, §9.4, §5.10 |

### 1.5 Method

- **R1–R7**, seven research lanes, read external sources on 2026-10-01 (each digest's scope is in the frontmatter).
- **G1–G5**, five grounding lanes, read this worktree, the installed BMAD method and the operator's makistack.
- **D1–D3**, three deep dives, read the code the build will cut: the runtime extraction, the sessions runtime, the provider-kind and voice sites.
- **C1** read the house formats and the numbering ceilings.
- **The coordinator** pinned P1–P15 from the three rounds, then ruled R1–R13 on the deep dives' pushback, and later R14–R29 (`agents-coordinator-decisions-2026-10-02.md`), which are marked where they land.
- **This pass** wrote the record, re-read the lines in §1.7, and cross-checked the digests against each other. Where two lanes disagree, both are reported and the disagreement is named.

### 1.6 What the rounds leave open, and where a pin moves an earlier decision

These are flagged rather than resolved. Each names where it is argued.

1. **The main agent's name.** The owner writes "nixi". The operator's Hermes default profile is `nixie` (G3 §1, G5 §6), and keeper's wake phrase defaults to `nixie` (`docs/decisions.md:189-190`, D-5) `[REPO]`. P1 pins "Nixi". Whether the agent's Matrix user, its `80-agents/` folder and the wake phrase share one spelling is not decided anywhere. **Settled by ruling R19:** `@nixi:<server>`, home `80-agents/nixi/`, display name "Nixi"; the wake phrase stays the person's own setting (D-5's default unchanged) and the Hermes profile `nixie` is untouched.
2. **"naia".** The owner lists naia among shared bots. In makistack `naia` is Marta's TrueNAS dataset `maia/naia`, storage and not an agent (G5 §6). Round 3 omits it.
3. **neuradrive's server copy is pull-only.** Dr Lucyna Novak is neuradrive's steward and writes sessions there (P1, P3). The server checkout of neuradrive has been pull-only since 2026-09-09 (makistack `README.md:200`, via G5 §6). Ruling R7 gives `agentd-neuraffica` its own checkout, and that checkout has to push. The pull-only posture was an operator decision about the existing checkout. **Settled by ruling R17:** agentd-neuraffica pushes from its own checkout, which is bidirectional by construction; switching the server's existing neuradrive checkout from pull-only is an operator action outside this repository (§13 #49).
4. **Epic 22's refusals and this program.** §3.10. **Superseded by ruling R16:** the refusals were decided for the operator's Hermes gateway and scoped to it; they are not reversed, and makistack's record stands. keeper agents reach drives only through grants, labels, a process per principal and approval tiers, and a first write still asks.
5. **A `run` tool against D-3, DW-213 and AD-159.** §7.9. **Settled by ruling R15:** DW-213's letter is kept, and `run` has its own D-entry (D-33).
6. **"Backchannel before barge-in" against the turn machine's first rule.** §8.8. **Settled by ruling R14:** pause first, and the utterance decides.
7. **D-5's wording.** D-5 says keeper "will **not** ship a model of its own" (`docs/decisions.md:173`). P13's models come from the owner's config repository and are not shipped, the precedent D-29 set for transcription (§8.1). The rule survives; its sentence needs the D-29-style amendment when the D-entry is written. **Settled by ruling R20:** D-36 is that amendment.
8. **Inside the pins.** P3's log name `log/YYYY-MM.<host>.jsonl` is superseded by ruling R3's chunks; P11's `TaskKind::Workflow` is superseded by ruling R9's cards. Both are resolved by the rulings and named here only so a reader of P3 or P11 alone is not misled.
9. **The vocabulary.** Rounds 1–3 say "bot" and "person"; ruling R1 makes the concept "agent" because `keeper-core/src/bots` and `[[provider.bot]]` already exist (D2 §7). The brainstorm stance "bots are instruments, never characters" (C1 §4) is overridden by the owner's explicit ask for named personalities (ruling R1).

### 1.7 Re-read in this pass

The following were re-read on 2026-10-02 and their line numbers are current:

- `pub enum ProviderKind` at `keeper-core/src/bots/mod.rs:69`;
- `pub const STATUSES: [TaskStatus; 4]` at `keeper-core/src/sessions/shape.rs:370`;
- `DEFAULT_SESSIONS_SUBFOLDER = "60-sessions"` at `keeper-sync/src/profile/mod.rs:224`;
- `END_OF_UTTERANCE_PAUSE` (1800 ms) at `keeper-core/src/voice/turn.rs:66`, and the rules "Barge-in stops speech first" at `turn.rs:18-22` and "The port never records its own answer" at `:36-43`;
- `CONFIG_MODELS_DIR = "_models"` at `keeper-core/src/transcription/models.rs:19`;
- `REMOTE_POLL_MS = 300_000` at `keeper-sync/src/engine.rs:570`, `LIVE_WATCH_BACKSTOP_MS = 3_600_000` at `:615`, and the only remote-poll check at `:16815`;
- `matrix-sdk = { version = "0.18", … }` and `matrix-sdk-ui = "0.18"` at `src-tauri/Cargo.toml:61-62` (G5 recorded `:55-56`; the lines have moved);
- `keeper-sync` as a dependency of the shell on every target, iOS included, at `crates/keeper/Cargo.toml:26-33` (Epic 66, AD-198);
- D-3 at `docs/decisions.md:84-114`, D-4 at `:116-167`, D-5 at `:169-252`, D-29 at `:1429`;
- `docs/ios.md:737` ("Nothing is merged on a phone … the Mac merges") and `:739`;
- `docs/sync.md:3490-3495` (the quiet-folder pull gap);
- `docs/decisions.md` D-4's "no keeper-operated proxy" (`:120`) and its revisit trigger "a third provider kind with a real endpoint to read against" (`:161-162`);
- the shell-tool refusal at `prds/prd-keeper-2026-07-03/prd.md:1151` and `epic-61-…:330-332` (DW-213, DW-214);
- in the drive mirror: `/workspace/tgdrive/README.md:9-24` (ten zones, `80` unused), `:32-33` (prose: the profile's LFS threshold is 256 KiB; the value itself is `lfsThresholdBytes = 262144`, `/workspace/tgdrive/.keeper/keeper.toml:33`), `:39-42` and `/workspace/tgdrive/.gitignore:104-105` (`/60-sessions/**/workspace/*` is ignored).

---

## 2. What keeper already has

### 2.1 Inventory

`Present` = built and reachable. `Partial` = built for a narrower case. `Absent` = nothing in the tree. `By decision` = absent because a recorded decision refuses or defers it.

| Capability | Status | Mechanism | Where | Digest |
| --- | --- | --- | --- | --- |
| Providers and models over the OpenAI wire | Present | `Provider {id, kind, name, base_url, created_ms}`; `ProviderKind {Hermes, Ollama}` closed at two; one transport, `POST /v1/chat/completions`, streamed | `bots/mod.rs:69`, `:120`; `chat.rs:42`, `:216-238` | G1 §1 |
| A generic OpenAI-compatible endpoint (CLIProxyAPI) | Absent | saving it as `ollama` is accepted, but discovery, images, `tool_choice` and the voice default misbehave | `discover.rs:116-119`, `:336`; `quirks.rs:229-231`; `voice_target.rs:161-164` | G1 §1, D3 A |
| Agents with a system prompt of their own | Partial | Hermes keeps its profile prompt on the server; keeper's identity is a shape, a colour and a mark | `bots_ipc.rs:1178-1187`; `bots/mod.rs:240` | G1 §2, §7 |
| Many conversations per bot, streaming at once | Present | many `bot_sessions` rows; concurrent streams in one process | `bots_ipc.rs:946-950` | G1 §2 |
| Archive | Present | a reversible `archived` flag | `session.rs:458`, `:934` | G1 §2 |
| A conversation as files in a drive | Absent | `keeper.db`; AD-154 makes the store the truth | `session.rs:108-145` | G1 §2 |
| Tool steps kept with the conversation | Absent | only the user and assistant rows; `tool_call_count` and audit rows | `bots_ipc.rs:1275-1284`, `:1684-1702` | G1 §2 |
| One conversation on two devices | Partial | Hermes only: `X-Hermes-Session-Id`, adoption, polling at 2, 5, then 15 s | `remote.rs:68`, `:268-278`, `:431-480`; `follow.rs:44-63` | G1 §2 |
| A router or master agent | Absent | only voice-target resolution | `voice_target.rs:9-18` | G1 §7 |
| Agent-to-agent messages | Absent | — | — | G1 §7 |
| Drive tools | Present (Ollama bots, desktop) | `drive_list/read/glob/grep/stat/write/edit`; no delete, no move; bounded | `tools.rs:73-145`, `:171-248` | G1 §3 |
| Grants and audit | Present | `grant::decide` (subtree > profile > drive; write under a drive-wide grant asks); audit row before the effect | `grant.rs:711-787`, `:805-830`; `bots_tools.rs:21-33`; `audit.rs:97` | G1 §3 |
| An approval that waits and resumes | Partial | a blocking ask polled every 250 ms, in memory only; unattended runs refuse; precedent: the Matrix drafts Approval Pane | `bots_drive_ipc.rs:162-215`; `bots_tools.rs:109-114`; `vm.rs:521-531` | G1 §3, §7 |
| Shell or exec | By decision | D-3; DW-213: "no tool executes a shell string"; AD-159 | `docs/decisions.md:84-114`; `prd.md:1151`; `tasks.rs:152-165` | G1 §3 |
| MCP client | Absent | a grep over `bots/` and the shell bot files finds nothing | — | G1 §3 |
| keeper as an MCP server | By decision (deferred) | DW-215: a listening socket needs its own threat model | `docs/decisions.md:146-150` | G1 §3 |
| Web fetch | Absent | — | — | G1 §3 |
| Scheduled bot turns | Present (Mac app only) | `TaskKind::Bot`, the shared cron dialect, a 1 h lease, a runner port that defaults to `None` | `tasks.rs:213-232`, `:255-283`; `engine.rs:5013-5097`; `platform.rs:413-414`; `keeper/src/sync.rs:52-57` | G1 §4, G2 §2 |
| A bot task's run log in its session (AD-225) | Absent | specified, not built | — | G1 §4 |
| A task pinned to a device | Absent | `TaskRow.profile_id` only | `db.rs:3162-3166` | G2 §2 |
| The sessions zone | Present (desktop) | `[folder.sessions]`, default `60-sessions`; status is the folder; `.keeper/` never synced | `profile/mod.rs:224`; `folder.rs:261-264`; `docs/sessions.md:17-33`, `:243-246` | G2 §1 |
| The board | Present, per session | four columns over `task`-tagged files: `status:` and a fractional `order:` | `docs/sessions.md:730-734`; `shape.rs:370` | G2 §1, D2 §2 |
| A board across a zone | Absent `[INFERENCE, G2]` | — | — | G2 §7 |
| Templates and spaces | Present | `_template/<name>/`, `_spaces/` saved queries | `docs/sessions.md:289-500`, `:811-816` | G2 §1 |
| Promote at archive, and on adding a ref | Present | `session_archive` with `promotes`; "Copy it into artifacts/ first" | `sessions_ipc.rs:1641-1676`, `:4469-4470`, `:4536-4537` | G5 §1 |
| The promote panel | Absent (specified) | the panel is phase 7's FR-243/FR-244; `docs/sessions.md` lists it with FR-229/235/236/241 (unread marks, history, capture), "none implemented" — **corrected by ruling R26**, which takes FR-243/FR-244 for the panel and leaves the others unbuilt (DW-391) | `docs/sessions.md:1083-1089` | G5 §1, G2 §1 |
| Sessions on the phone | Absent | DW-237 | `sessions_ipc.rs:41-44`; `deferred-work.md:3953` | G2 §1 |
| Search over a notes vault | Present | `<vault>/.keeper/search.db`: FTS5 plus vectors, brute-force cosine | `search_index.rs:1-30`, `:362` | G1 §6, G2 §4 |
| Retrieval over drives for a bot | By decision | DW-212 | `epic-61-…:327-328` | G1 §6 |
| Reading OKF | Present, read only | `keeper_core::notes::okf`; `.okf/OKF-0.2-digest.md` as a context file | `context_files.rs:24-28`, `:67` | G2 §5 |
| Device identity | Present | `sync.db` device ULID and label; the account's device slug and `device.<device>.toml` | `db.rs:1626-1658`; `org_account/layout.rs:484-491`; `device_state.rs:5`, `:21-24` | G2 §6 |
| A spoken turn on the device | Present (Mac, iPhone) | `SFSpeechRecognizer` on device, the pure turn machine | `voice/turn.rs`; `voice_macos.rs`; `voice_ios.rs` | G5 §2, D3 B |
| End of turn by a model | Absent | a 1800 ms pause after the last partial | `turn.rs:60-66` | D3 B |
| Barge-in | Present | speech while `Speaking` stops the voice first | `turn.rs:18-22` | G5 §2 |
| Echo cancellation | Present | voice processing on the input node; on iOS an 800 ms tail gate | `voice/platform.rs:16-21`; `voice_ios.rs:221`, `:855-867` | G5 §2 |
| On-device models from `_models/` | Present (transcription) | LFS-hydrated from the config repository; a per-role file list; refuses an incomplete set | `transcription/models.rs:19`; `account_ipc.rs:224-300` | D3 B |
| A Matrix client | Present (a messenger for people) | matrix-sdk 0.18, Simplified Sliding Sync, E2EE, password, OIDC and Beeper JWT login | `src-tauri/Cargo.toml:61-62`; `account.rs:209-231`; `auth.rs:101-147` | G5 §5, D1 §3 |
| A Matrix client for agents | Absent | `client_for` is private; no create-room, invite, `send_raw`, state or event handler is exposed | `account.rs:1942`; `bridges/mod.rs:76` | D1 §3 |
| Push to phones | Absent | local notifications only; APNs deferred behind D-1 | `src-tauri/Cargo.toml:37-39`; `docs/decisions.md:13-24` | G5 §5 |
| To-device messages | Absent (SDK-internal only) `[INFERENCE, G5]` | — | — | G5 §5 |
| Calls (Element Call, MatrixRTC) | Absent | "calls arrive post-MVP via the Element Call widget" | `docs/constraints-and-limitations.md:30` | G5 §5 |
| A bot that drives the notes view | Absent | the file is the API; an open note adopts external edits with a diff bar | `docs/notes.md:403-414`; `bots_tools.rs:423-465` | G5 §4 |
| Drawing or a canvas | Absent | no dependency, no component | — | G5 §4 |
| Android | Absent | no `gen/android`, no Android `Platform`, a `compile_error!` guard | `ipc.rs:460-464`; `crates/keeper/Cargo.toml:55-69` | G5 §3 |
| A headless host that runs bot turns | Absent | `keeper-syncd` never runs bot tasks; its `LinuxPlatform` lives in a bin crate | `keeper-syncd/src/platform.rs:57-470` | D1 §2 |
| Bots on the phone | Partial | chat, voice and following a Hermes session; "the drive tools live on your Mac" | `ipc.rs:1520`, `:1535`; `docs/ios.md:739` | G1 §7 |

### 2.2 Providers, the wire and discovery `[REPO]` (G1 §1)

- **A provider** is `Provider {id, kind, name, base_url, created_ms}` (`bots/mod.rs:120`) in the table `bot_providers(…, health_state, health_checked_ms, health_detail, read_timeout_ms)` (`store.rs:82-94`). The code calls a third kind "one row and one match arm" (`mod.rs:56-59`, DW-214).
- **A bot** is `Bot {id, provider_id, target, name, pin_order, identity, created_ms}` (`mod.rs:263`), `UNIQUE(provider_id, target)` (`store.rs:107-119`). `target` is a Hermes profile (a `/p/{target}` prefix) or an Ollama tag (the body's `model`), joined in one place, `Endpoint::url` (`mod.rs:353-375`).
- **Credentials** go through the keychain port as `bot_provider_token/<pid>` and `bot_token/<pid>/<target>`, bot first, then provider (`mod.rs:382`, `:396`, `:414`); an org-account OIDC token can stand in (`:439`).
- **One wire:** `POST /v1/chat/completions` with `stream: true` and `stream_options.include_usage` (`chat.rs:42`, `:216-238`; AD-149). No Responses API, no Anthropic Messages, no Ollama `/api/chat` (DW-210).
- **Per-kind differences** live only in the const table `quirks()` (`quirks.rs:208-237`): Ollama drops `tool_choice` and takes a bare-string image part; Hermes takes an object; reasoning fields differ; embeddings and server sessions are `Unknown` on Hermes.
- **Streaming bounds:** an SSE frame cap of 1 MiB (`sse.rs:52`), tool-call arguments capped at 1 MiB (`chat.rs:578`), the whole stream at 16 MiB (`:947-962`); a partial reply is kept with `Failed` or `Cancelled` (`:14-28`).
- **Timeouts:** connect 10 s; silence 120 s by default, per provider through `read_timeout_ms`; pool idle 20 s; never a total deadline (`http.rs:55`, `:65`, `:73`; `bots_ipc.rs:249-253`). NFR-46's 30 s floor is documented and not enforced (`ARCHITECTURE-BOTS.md:346`).
- **Retries:** at most two sends, only when nothing streamed and the failure was a connect error, 429 or 5xx; `Retry-After` honoured (`chat.rs:933-1060`).
- **Discovery** (`discover.rs:1-31`): health is `GET /api/version` (Ollama) or `GET /health` (Hermes) (`:116-119`); models are `/api/tags` or `/v1/models` merged with `/api/model/options` (`:194`, `:325`, `:489`); a named bot can be probed (`:219`); listing Hermes bots is refused (`NO_BOT_ROSTER`, `:95`); capabilities are `Option<bool>` and an unread one is `None`, never `false` (AD-151); a 30 s silence budget (`:72`).

### 2.3 Conversations and the tool loop `[REPO]` (G1 §2–§3)

- **Storage** is per-device SQLite `keeper.db`: `bot_sessions`, `bot_messages`, `bot_dismissed_remote_sessions` (`session.rs:108-145`, `:177-200`); no JSON in a row; later columns nullable or defaulted (`:97-101`).
- **The crash contract:** the assistant row is inserted empty with `partial=1` before the request leaves (`bots_ipc.rs:1276-1284`).
- **Replay loses the tool trace:** a turn stores a user row and an assistant row; tool calls survive only as `tool_call_count` and audit rows (`bots_ipc.rs:1275-1284`, `:1684-1702`).
- **Conversations never sync.** Device restore carries providers, bots and grants (`org_account/device_state.rs:5-6`; D-27) and re-mints bot ids (`account_restore.rs:784`).
- **No keeper-side prompt per bot.** The only system message is the context-file bundle (only when tools are offered) and the spoken-language line (`bots_ipc.rs:1178-1187`).
- **When tools are offered** (`offer_tools`, `tools.rs:613`): never to a Hermes bot ("Hermes runs its tools on its own host", `grant.rs:543-557`); to an Ollama bot with a live grant and a `tools` capability not `Some(false)`; a read grant gets no write specs (`tools.rs:236-243`).
- **Caps:** read 64 KiB, list 200, grep 100, glob 100, walk 20 000 entries, write 1 MB, rendered result 80 KiB, 8 rounds a turn, 8 calls a round (`tools.rs:73-145`).
- **Order in the host** (`bots_tools.rs:21-33`, `:117-228`): find the profile → `grant::check` → `audit::append_intent` (committed before the effect, or the call is refused) → Ask/Deny → `keeper_sync::bots_fs` → `audit::complete`.
- **The approver** sends `BotStreamEvent::ApprovalAsked` and blocks the tool call, polling every 250 ms, until `bots_approval_answer` arrives; Stop or a vanished pane is a refusal; "Always for this folder" saves a subtree grant first (`bots_drive_ipc.rs:162-215`, `:223`). Nothing is persisted; nothing resumes after a restart.
- **Data, not instructions:** every tool result is wrapped in `FILE_CONTENT_IS_DATA` (`tools.rs:153-155`); context files sit under `UNTRUSTED_PREAMBLE` (`context_files.rs:97-102`). Enforcement is `decide` plus `browse::resolve` (`tools.rs:35-41`); only the UI writes grants (`grant.rs:36-41`, NFR-48).
- **Context files:** `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `.cursorrules` and `.okf/OKF-0.2-digest.md`, 32 KiB each, 64 KiB total, 12 levels, only under granted roots (`context_files.rs:61`, `:67`, `:77-91`, `:290`).

### 2.4 Bot tasks `[REPO]` (G1 §4, G2 §2, D1 §1)

- **The row** is in `sync.db` `tasks` with `bot_id`, `prompt_subpath` and `model` (`db.rs:202-213`, `:3150-3154`); `TaskMode` `off | manual | scheduled`; a missed-window policy `run_now | skip | delay` (grace 15 min, default delay 30 min, `tasks.rs:53-115`); a 1 h lease (`engine.rs:749`).
- **The closed vocabulary** is `sync`, `release`, `verify`, `bot`, `copy`, `gc` (`tasks.rs:152-236`); `update` is refused forever (`docs/sync.md:2343-2349`), and there is no shell-string kind (`tasks.rs:158-181`). It is drift-guarded against `TASK_KINDS` in `src/lib/stores/sync.ts` (story 59.11).
- **The schedule grammar** is 5-field cron, `@hourly | @daily | @weekly`, or `every <n><unit>` from the end of the previous run; floor 60 s, ceiling 366 d; a bad schedule is refused at save (`docs/sync.md:2394-2440`; `tasks.rs:9-49`).
- **Which host runs it:** whichever tick sees it due first; both the app and `keeper-syncd` tick at about 1 Hz (`tasks.rs:4-12`, `:346-348`; AD-62, AD-136). The phone keeps no tick (AD-226).
- **The runner** exists only in the desktop shell (`keeper/src/sync.rs:52-57`). `keeper-syncd` and the phone record `Deferred`: "this host cannot run a bot task; the keeper app on the Mac runs it" (`platform.rs:413-414`).
- **The runner duplicates the turn.** `ShellBotTaskRunner::prepare` (`bot_task.rs:109`) is a second copy of `arm_turn`; it runs `run_tool_loop_reporting` directly with `approve: None` and an inert cancel signal (`:96-107`, `:172`, `:220`). The doc at `keeper-sync/src/platform.rs:395` says it runs "over its own `open_turn`", and `TurnOrigin` (AD-224) has zero matches (D1 §1).
- **Unattended asks are refused** and folded into `task_runs.detail` with `TOOL_REFUSED_MARK` (`bots_tools.rs:109-114`; `platform.rs:316`; `engine.rs:5124-5140`).
- **History:** 50 runs per task, with host, outcome and a detail line (`docs/sync.md:2130-2133`).

### 2.5 The sessions zone, the board and promotion `[REPO]` (G2 §1, G5 §1)

- **Layout** (`docs/sessions.md:17-33`): `README.md` and `AGENTS.md` at the zone root, `_template/`, `_spaces/`, `active/YYYY-MM-DD-<slug>/`, `archive/<year>/…`, and `.keeper/` (cache, journal, trash; never synced, a Tier-0 exclusion, `docs/decisions.md:1127`).
- **Status is never a stored flag:** it *is* the folder's location (`docs/sessions.md:243-246`).
- **Two shapes**, both read forever (`:55-87`): *flat* (one markdown pool; the shape test is `AGENTS.md` alone, `:601-603`) and *folder* (`README.md` plus `refs/` and `prompts/`). `artifacts/` (versioned output) and `workspace/` (scratch, fenced: `files_write.rs:1255-1298`, `WriteRefusal::SessionWorkspace`) survive in both.
- **Kinds are tags:** `about`, `task`, `log`, `prompt`, `ref`; an *Untagged* space lists the rest (`:59-72`).
- **The board is already a kanban:** "a board of four columns — in preparation, to do, done, deferred — over the files tagged `task`. A card's column is its `status:` and its position is its `order:`, a fractional number, so dragging one card rewrites one file rather than renumbering everything below it" (`:730-734`). Widgets `> [!board]`, `> [!log]`, `> [!refs]` render it in any note (`:742-744`).
- **The zone's `AGENTS.md`** is "agent rules for the zone"; a flat session's `AGENTS.md` is "the navigation contract, written for whoever, or whatever, is handed the folder with no other context" (`:19-20`, `:72-74`). In the bots tool surface it enters the request as data under `UNTRUSTED_PREAMBLE`, never obeyed (`context_files.rs:24-28`, `:66-67`).
- **Templates** copy a skeleton, restamp the record, fill `{{title}}/{{id}}/{{date}}/{{time}}`, always add `artifacts/` and `workspace/` (FR-288) (`:289-500`).
- **Promotion is an explicit copy** "under a stable name, recorded in the `## Promote` table" (`:66-68`). Built: the README skeleton's table (`sessions_ipc.rs:957-958`), archive promotes (FR-245, AD-111, journaled and idempotent, `:1641-1676`), promote-on-add-ref (`:4469-4470`). Not built: the promote panel (FR-243/FR-244, ruling R26; `docs/sessions.md:1083-1089` lists it among FR-229/235/236/241, which stay unbuilt beside it, DW-391).
- **The drive** ignores session workspaces: `/60-sessions/**/workspace/*` (`/workspace/tgdrive/.gitignore:104-105`), "scratch … disposable by design"; the README says the same (`/workspace/tgdrive/README.md:39-42`) `[REPO]`.

### 2.6 How fast another device sees a change `[REPO]` (G2 §3, D2 §5)

| Leg | Trigger | Window |
| --- | --- | --- |
| watcher | FSEvents or inotify, 500 ms debounce | continuous (`docs/sync.md:3463`) |
| settle | size, mtime, ctime and inode stable | 5 s (10 s removable, 60 s ceiling); Linux adds an open-writer veto (NFR-25) |
| commit | once settle has elapsed | "seconds after the watcher sees it" (`:3380-3385`) |
| push | journaled, drained on the 1 Hz tick | settle + 1 s (NFR-25) |
| remote poll | `REMOTE_POLL_MS = 300 000` (`engine.rs:570`), checked only inside a scan pass (`:16815`) | up to 5 min, and in a quiet folder with a live watcher up to the hourly backstop (`LIVE_WATCH_BACKSTOP_MS`, `:615`) |

- **Device A to device B** is commit plus push (seconds) plus B's next remote poll: "a quiet live-watcher folder can wait for the hourly backstop. This scheduling gap was observed on hesperia/v0.8.27 … Forgejo does not push to clients" (`docs/sync.md:3490-3495`).
- **The root cause and its fix** are §12.9.

### 2.7 Search, embeddings and OKF `[REPO]` (G1 §6, G2 §4–§5)

- **`embed.rs`** posts OpenAI-shaped `/v1/embeddings` in batches of at most 32 with E5/nomic prefixes, and refuses a kind whose embeddings quirk is `No` (`embed.rs:10-12`, `:45-60`, `:149-154`).
- **Its only consumer** is the notes index: `<vault>/.keeper/search.db`, FTS5 plus f32 vectors, brute-force cosine, 0.45 lexical and 0.55 vector (`search_index.rs:1-30`, `:25-30`, `:362`, `:583`); chunks of 1 200 target and 4 000 maximum characters, heading-first, no overlap (`notes/chunk.rs:11-13`, AD-262). The scale assumption is under ~10⁵ chunks (DW-262). The index is derived and disposable (D-21).
- **Not a bot tool.** Epic 61 refused retrieval for bots (DW-212).
- **OKF is present and read only.** `keeper_core::notes::okf` is a tolerant typed view of OKF v0.2 frontmatter; `type:` is the only hard key. Note reads carry `OkfFacts` (`doc_type`, `generated_by`, `verified[].by`, `human_reviewed`) rendered in the tool row (`bot-tool-call.tsx:22-27`, `:185-203`). keeper "reads OKF and never writes it" (spec-61-11's Never list, `:40`).

### 2.8 Device identity `[REPO]` (G2 §6)

- **The sync engine's identity** is `DeviceIdentity { id: ULID, label }` in `sync.db` (`db.rs:1626-1658`); the label rides every commit as `Keeper-Device: <label> (<id>)` (`git/commit.rs:619-620`) and names conflict copies.
- **The account's device** is a slug from the host label (`a-z0-9-`, ≤ 32, iOS = model + 4 hex; `org_account/layout.rs:484-491`), persisted as `account.<id>.device_slug`, with `<login>/device.<device>.toml` written only by that device (D-27, FR-727). Reinstall adoption reuses a name only for a record carrying this machine's fingerprint (AD-329).
- **No device-targeted task exists** `[INFERENCE, G2]`; affinity is emergent from folder binding and runner capability.

### 2.9 Voice `[REPO]` (G1 §5, G5 §2, D3 B)

- **D-5 governs:** "speech becomes text on the device, the text is sent to the bot as an ordinary message, and the answer is spoken — and it will **not** ship a model of its own, send a voice anywhere, or start its own microphone" (`docs/decisions.md:171-173`).
- **The turn machine** (`voice/turn.rs`) is pure: `Idle → Listening → Heard → Sending → Speaking → Idle | Failed`, decided by `advance(state, event)`; inputs include `PartialHeard`, `FinalHeard`, `AnswerSentence`, `AnswerDone`, `StopHeard`, `SpeechDetected`, `Silence`, `Abandoned` (`turn.rs:95+`).
- **End of turn is a timer:** `END_OF_UTTERANCE_PAUSE` = 1800 ms after the last partial, because "the on-device recogniser does not end an utterance by itself in a continuous request" (`turn.rs:60-66`); `NOTHING_HEARD_TIMEOUT` = 8 s (`:58`).
- **Barge-in stops speech first:** "`SpeechDetected` while `Speaking` yields `Effect::StopSpeaking` before any other effect, because the person started talking and nothing should still be talking over them. What they said decides what follows (AD-208)" (`turn.rs:18-22`).
- **Full duplex on both Apple platforms** since AD-213: voice processing on the input node keeps keeper's voice out of the transcript, so the microphone stays open for barge-in (`turn.rs:36-43`; `voice/platform.rs:53-60`, `:92`, `:107`). D-5's older sentence "false while an utterance speaks on a half-duplex platform, which the Mac is" (`decisions.md:187-188`) predates that revision.
- **Echo cancellation:** `setVoiceProcessingEnabled(true)`; story 22.7 measured the far end dropping about 24 dB on hesperia (`platform.rs:16-21`). iOS also sets `voiceChat` (a `.default` + `defaultToSpeaker` pair silently defeated cancellation, `docs/ios.md:1116-1121`) and runs an 800 ms `TAIL_GATE` that drops late transcripts as `echo_dropped` (AD-205…AD-209; `voice_ios.rs:221`, `:855-867`, `:1139-1143`).
- **Recognition is on device by enforcement:** every request sets `requiresOnDeviceRecognition = true`, and a source scan fails the build otherwise (AD-166, FR-402, NFR-50; `docs/decisions.md:207-215`).
- **The voice target** is `bots.voice_target`, else the pinned bot most recently talked to; never what is on screen (`voice_target.rs:1-23`; AD-205/206). Trap: `bots.voice_target` is a `UserGlobal` setting (`config/keys.rs:495-501`) while bot ids are re-minted per device (`account_restore.rs:784`), so the same value can name no bot on another device `[INFERENCE, G1]`.
- **Transcription is a separate engine:** the vendored FluidAudio fork behind `trait SpeechEngine`, macOS only; "the voice pipeline and the transcription engine are disjoint" (D3 B). §8.7 has the detail.

### 2.10 Matrix in keeper `[REPO]` (G5 §5, D1 §3)

- **The SDK:** `matrix-sdk = { version = "0.18", features = ["e2e-encryption", "qrcode", "sqlite", "sso-login"] }` and `matrix-sdk-ui = "0.18"` (`src-tauri/Cargo.toml:61-62`), homed in keeper-core (AD-6).
- **Sync:** `SyncService` plus `RoomListService` per account under a supervised task (`account.rs:7`, `:40-42`, `:209-231`); a manual `/versions` probe for sliding sync (`auth.rs:575-578`); timelines are snapshot-then-diff (`timeline.rs:2-5`).
- **E2EE:** encrypted timelines; QR device verification; key backup through `Recovery` (`backup.rs:4-7`); an optional store passphrase.
- **Login:** password, OIDC (`client.oauth()`, `auth.rs:140-147`), Beeper JWT (`auth/beeper.rs:214-216`).
- **Push:** none. "content originates only from the local decrypting loop, never a push gateway" (`src-tauri/Cargo.toml:37-39`); iOS push is deferred behind the paid-program gate (`docs/decisions.md:13-24`).
- **Bot users** render as ordinary room members; nothing special-cases them (`bots/identity.rs:163-171`) `[INFERENCE, G5]`.
- **Bridges** are appservices reached through the homeserver (`egress.rs:12-15`); `bbctl` can run a self-hosted bridge (`bridges/bbctl.rs:88-92`).

### 2.11 The notes view `[REPO]` (G5 §4)

- **CodeMirror 6** with `@lezer/markdown`; no ProseMirror, no Yjs (`package.json:41-51`, `:62`).
- **Three modes:** Preview, Note (live preview, the default), Source (`docs/notes.md:348-352`); frontmatter as a properties panel (`:70-73`).
- **Widgets:** `> [!board]` (a four-column kanban whose drag writes `status:`/`order:`), `> [!log]`, `> [!refs]`, mermaid, gallery, CSV, `![[…]]` embeds; media chips that become `keeper-media` blocks (`docs/notes.md:125-261`).
- **A bot drives it only by writing files:** "An agent with nothing but a text editor is a first-class author. It edits the `.md` files; the file is the API" — the open editor adopts the change with a diff bar, unread marks and history (`docs/notes.md:403-414`). No verb opens a note, highlights or inserts in the UI `[INFERENCE, G5]`. Table lenses and sticky windows are specified, not built (FR-123/124).

### 2.12 The rules a new agent system inherits `[REPO]` (G1 §8)

- **AD-24** core reaches the OS only through `Platform` (`ARCHITECTURE-SPINE.md:171`). **AD-27** an off capability renders nothing (`:187-190`). **AD-40** keeper-core never depends on keeper-sync, and keeper-syncd depends only on keeper-sync (`:293-296`). **AD-52** syncd is its own binary. **AD-6** new Rust defaults into keeper-core.
- **AD-55/56** every decision lives in keeper-core; the shell is a call site. **AD-62/136** one clock per host, a due-gate plus a lease. **AD-65** `browse::resolve` is the single containment rule.
- **AD-146** the kind set is closed; **AD-147** secrets through the keychain port; **AD-148** egress derived, host only; **AD-149** one wire bounded by silence; **AD-151** unread is `unknown`, never `false`; **AD-154** keeper's store is the conversation's truth; **AD-158** grants re-checked per call, a write never auto-approved by a grant alone, audit before effect; **AD-159** tools bounded and disclosed, file content is data.
- **AD-176/177/178** Hermes identity, step-level following, no new destination. **AD-205/206** the voice turn finishes in Rust, its target chosen on screen. **AD-224** a bot task is a closed verb and its runner port defaults to `None`.
- **D-3** no exec kind; **D-4** no default endpoint, hosted model or keeper-operated proxy; **NFR-11** egress honesty; **NFR-43** unknown rows skipped, columns additive; **NFR-46…49** bounded silence, audit before effect, no self-widening, every byte bounded; **NFR-67** embeddings add no destination.

**Where the docs and the code disagree** (each a fact the program must not inherit as written):
- AD-224's `open_turn` with a `TurnOrigin` does not exist; the runner calls the loop directly (G1, D1 §1).
- AD-224 restricts a task's prompt to `prompts/`; the code accepts any markdown file (`tasks.rs:255-258`, AD-244).
- NFR-46's 30 s floor is not enforced (`ARCHITECTURE-BOTS.md:346`).
- "No JSON blob in a row" is cited as AD-139 (`ARCHITECTURE-BOTS.md:28`); AD-139 is `on_missed`. The rule holds; the number is wrong (G1 §8).
- `sessions_exec.rs`'s module doc claims "a `Mutex` per zone" and "resumes on registry start"; neither exists (`sessions_exec.rs:4-10`; D2 §1).
- `sessions_ipc.rs:2628-2629` claims keeper's provenance on session writes; `sessions_exec` does plain `std::fs` writes, so the commit is the watcher's `[INFERENCE, D2]`.

---

## 3. Prior art the operator already paid for

All of §3 is G3's reading of the makistack repository and keeper's own research, cited as G3 recorded it.

### 3.1 Nixie on OpenClaw, and why it moved to Hermes `[REPO, makistack]` (G3 §1)

- **Before (2026-07-11):** Nixie was tgorka's bot on **OpenClaw** on electra — image `ghcr.io/openclaw/openclaw:2026.7.1-beta.5`, one container on port 18789, state volume `nixie_data:/home/node/.openclaw`, Tailscale Serve `:8443`, Caddy `handle_path /nixie/*`, ACL admin-only. Channels: Telegram (owner allowlisted, `telegram:856190366`) and a Matrix DM on the private tuwunel homeserver, plus ElevenLabs TTS. Model chain: Codex OAuth primary (`gpt-5.6-terra`) → OpenRouter fallbacks (nemotron-3-super-120b, `openrouter/free`, mimo-v2.5) → OpenCode. Skills all disabled; no MCP servers.
- **Dixi** is Marta's separate OpenClaw instance on the same host — its own container, port, 1Password item and ACL `group:marta` — "the mandated isolation given OpenClaw's own 'not a hostile multi-tenant security boundary' trust model".
- **Why it moved (2026-08-21):** OpenClaw's 2026 CVE stream — the one-click RCE and token exfiltration CVE-2026-25253, "ClawJacked" cross-origin WebSocket takeover, a WhatsApp-to-host-escape chain (GHSA-hjr6-g723-hmfm, CVSS 8.8) — and a hardening verdict that the bot must never hold crown-jewel credentials. The operator's requirements R1–R11: archive and restore dixi, move nixie to Hermes, **zero external channels** (tailnet only), dashboard and Open WebUI frontends, local Ollama beside cloud credentials, MCP control of Paseo, drive read access, a Bot-Mode roster of BMAD personas.
- **What was kept:** the whole hardening posture (non-root, `cap_drop: ALL`, `no-new-privileges`, no docker socket, read-only mounts, the network position as the control) and Nixie's persona, memory and skills, imported by `hermes claw migrate` (SOUL.md; MEMORY.md merged and deduplicated; skills → `skills/openclaw-imports/`, story 22-3).
- **What failed or was withdrawn:**
  - session **transcripts do not migrate**;
  - config mappings are lossy (`timeoutSeconds → max_turns = /10 capped 200`);
  - the local MythoMax model was withdrawn by the operator;
  - **drive access and the OKF plugin were withdrawn outright** — "nie podpinaj dysku": no bind mount, no filesystem MCP, no `hermes-okf`; story 22-6 was dropped in full;
  - pi-knowledge was rejected (a Python `register(ctx)` ABI against a TypeScript omp plugin; no MCP surface);
  - `hermes-okf` as a memory provider was withdrawn (a third-party 0.5.x PyPI package against an exact-pinning repository).
- **How it landed:** 22-2 built `docker/hermes/` — one container pinned **by digest** (`:latest` moved twice in 40 minutes), three address-qualified publishes (`127.0.0.1:9119` dashboard via Serve; `100.101.101.20:8642` and `172.17.0.1:8642` API server), fail-closed dashboard basic-auth, state in `/opt/data` (SQLite WAL + FTS5). 22-3 made `config.yaml` and `SOUL.md` committed literals bound `:ro`, killing the `openclaw config patch --stdin` drift machinery. `openrouter` is the configured primary; `openai-codex` is promoted after a one-time TTY device-code login; the Ollama LXC is wired for tool-less chat only, because Hermes refuses sub-64 000-token contexts for agent use and the LXC pins 16 384. 22-4 deleted `docker/nixie/**` atomically and retargeted Serve `:8443 → :9119`; 22-5 pointed Open WebUI at `http://172.17.0.1:8643/v1`.

### 3.2 Hermes Agent as deployed (v0.20.5 / 0.21.0) `[REPO, makistack]` (G3 §2)

- **Language and licence:** Python ≥ 3.11, < 3.14, MIT, NousResearch.
- **Architecture:** one `AIAgent` serves CLI, TUI, gateway, ACP and API server — "platform differences live in the entry point, not the agent". s6-overlay; each profile a supervised service; non-root UID 10000, remapped to 1000.
- **State:** one directory, `HERMES_HOME=/opt/data` — `config.yaml`, `.env`, `auth.json`, `SOUL.md` (**slot #1 of the system prompt**), `memories/`, `skills/`, `sessions/`, `cron/`, `profiles/<name>/`, `state.db`. No Postgres, no Redis; never two gateway containers on one data dir.
- **Sessions and memory:** server-side in `state.db` (unique titles, parent lineage on compression, ids rotate); API continuity through `X-Hermes-Session-Id` / `X-Hermes-Session-Key`; memory is `memories/MEMORY.md`, `USER.md`, skills and FTS5 recall.
- **Skills, cron, subagents:** agentskills.io markdown; routines in `hermes cron list`; `subagent.start/complete` events; API-server sessions run under `approvals.unattended_mode` (default **deny**) with `UNRECOVERABLE_BLOCKLIST` unoverridable.
- **Gateways:** a platform activates on **credential presence** — supply none and `api_server` is the only platform, which is how "no channels" is spelled. API server 8642 (bearer required even on loopback), dashboard 9119 (fails closed non-loopback), MCP over stdio, HTTP or OAuth. `hermes config check` **validates nothing** (`hermes_cli/config.py:5862-5900`).
- **Bots:** "a Bot **is** a Hermes profile" — isolated config, memory, skills and credentials under `profiles/<name>/`; `message_agent` fire-and-forget behind `agent.bot_mode_protocol`; cross-machine peers via `hermes peer add`.
- **Weaknesses:** 7+ CVEs in about six months (CVE-2026-53869 DNS rebinding on WS, CVE-2026-10223 memory-tool RCE with a public PoC, CVE-2026-11461, CVE-2026-10548 …); `terminal` can still `cat` or overwrite paths the write guards deny; a July-2026 prefix-auth breaking change; no bot roster on the bearer listener.

### 3.3 OpenClaw security findings and the hardening decisions `[REPO, makistack]` (G3 §3)

- **Risks recorded:** CVE-2026-25253 (CVSS 8.8, via the Control-UI `gatewayUrl`); ClawJacked; the WhatsApp host escape; Bitsight's 30 000+ exposed instances probed "within minutes"; **ClawHavoc**, 1 184 malicious ClawHub skills (reverse shells, credential exfiltration, a 22 MB README-padding evasion); a "souls" infostealer harvesting `openclaw.json`, `device.json` and `soul.md`; Giskard's shared-`main`-DM cross-user leak.
- **Frameworks:** Willison's **lethal trifecta** and Meta's **Rule of Two** — Nixie with Telegram ingress, web/browser and any sensitive MCP grant meets all three and "must not run autonomously". "The Attacker Moves Second": all 12 published injection defences defeated; human red-teaming 100%.
- **Q1 — MCP to Paseo: no**, as an autonomous bot tool (repo read-write, `~/.claude`, gh/Graphite tokens, SSH keys, the 1Password service account, `claude-yolo`: a textbook confused deputy); at most read-only telemetry.
- **Q2 — notes: no to read-write** (stored prompt injection and memory poisoning: MINJA > 95% in the lab, ZombieAgent persistent memory); **neuradrive entirely off-limits**.
- **Baseline hardening, ten points:** patch cadence; drop small web-enabled fallback models or deny them `group:web`/`browser`; `sandbox.mode: "all"` and `workspaceAccess: none`; file-system and network isolation with an egress allowlist; `trustedProxies`; channel lockdown (`dmPolicy: pairing`, mention gate, never merge Nixie's and Dixi's trust boundaries); secrets brokered per call; no unscanned ClawHub skills; audit logs and rate limits; **human approval as a durable rule, not per-action prompts**. The **zero-click link-preview exfiltration** path (PromptArmor): read access plus the ability to emit a link is already exfiltration.

### 3.4 Story 22-8 — the BMAD roster on Hermes `[REPO, makistack]` (G3 §4)

- **Eight profiles:** `nixie` (internal id `default`), Mary (Business Analyst), John (Product Manager), Sally (UX Designer), Winston (System Architect), Amelia (Senior Software Engineer), Murat (Master Test Architect and Quality Advisor) — titles from `_bmad/config.toml:56-185` — and an infrastructure profile `webui`. A profile is a separate `HERMES_HOME`; **nothing inherits**, so every home pins the whole launch chain, `max_turns`, `hard_stop_enabled` and `approvals`.
- **Orchestration:** `message_agent` exists only in canonical Bot Chat sessions; fire-and-forget, no live interrupt; group rooms cap at **2–6 bots, 3 serial rounds, 10 messages per send**; `@name` scopes a round, `@user` escalates to the human. "A seven-bot room does not fit and must not be planned."
- **Per-persona tools:** `memory, session_search, skills` (+ `web` for Mary and John); **no persona gets `file`** ("files (ro)" is not expressible, and persona writes under `/opt/data` could poison Nixie's memory); **no persona gets an MCP server**; no profile pins an LXC model (`agent_init.py:2795` raises below 64k context). Nixie alone holds `terminal, file, memory, session_search, skills, mcp-paseo` with `web` and `browser` disabled: the broker holder has no untrusted-content vector. `webui` has zero toolsets and no memory, on its own port.

### 3.5 Story 22-7 — the Paseo broker `[REPO, makistack]` (G3 §5)

- Paseo had **no network API** (daemon on `127.0.0.1:6767` inside superset-host's network namespace). The broker `docker/superset-host/paseo-mcp.py` is a stdlib-only JSON-RPC 2.0 MCP endpoint on `/mcp`: auth before routing, no token ⇒ **503**, a 64 KiB body cap, every subprocess an argv list.
- **Exactly four verbs:** `list_agents`, `get_agent_status`, `create_agent(prompt, workspace)`, `send_agent_prompt`. Responses go through an 8-field whitelist (`agentId, workspaceId, title, status, provider, createdAt, updatedAt, prUrl`); provider, model, argv and cwd from the caller are rejected with 400; the broker pins `PASEO_MCP_PROVIDER`; ceilings answer 429; every call is audited.
- **"The PR is the airlock":** no merge verb, no drive writes, no `tail_logs` (logs are an exfiltration channel), no `archive_*`, no `create_schedule`. Published on docker0 only (`172.17.0.1:5189`); attached to Nixie only; the kill switch is a blank 1Password field.

### 3.6 Observability and evals `[REPO, makistack]` (G3 §6)

- Nothing in the OTel GenAI conventions is Stable (all Development as of July 2026); `gen_ai.evaluation.result` makes eval scores telemetry on the same pipeline. Opik is the only self-hostable platform with evidenced online evaluation; Grafana is a native OTLP destination, so the existing LGTM stack is first-class.
- METR: experienced developers 19% **slower** while forecasting +24%. Eval method: trace → label → cluster → dedupe → versioned golden set → CI gate; judge calibration κ ≈ 0.75 with a stronger, different-family judge; harness configuration alone swings scores 10–20 points.
- The live fleet: telemetry is write-only (5 966 accepts, 0 rejects; no PR-count series), so no change can be judged an improvement today.

### 3.7 keeper's research-ai-chat decisions that still bind `[REPO]` (G3 §7)

Section numbers in this subsection are research-ai-chat's own (`research-ai-chat-2026-09-02.md §N`), not this document's.

- **research-ai-chat §2.6:** Hermes never hands the client a pending tool call (`tool_execution == "server"`), so keeper cannot run drive tools on Hermes' behalf over `/v1/chat/completions`.
- **research-ai-chat §2.15 / §4.2:** no bot roster on the bearer listener; named `/p/<profile>/` prefixes reject the default key since July 2026.
- **research-ai-chat §4.1:** the honest common subset of both back ends is system + user + assistant text, one streamed answer, `data:` images and usage; one abstraction must carry per-backend capability records.
- **research-ai-chat §4.6:** `reqwest-eventsource` rejected (pins reqwest ^0.12); `ollama-rs`'s `tool-implementations` feature must never be enabled (GPL-3.0 via metadata that passes cargo-deny); `piper-rs` declares MIT but compiles GPL-3.0 espeak-ng. The licence firewall (`deny.toml:7-60`) is the enforcement point. All LLM plumbing is Rust; the webview makes no HTTP calls.
- **research-ai-chat §10:** code and model licences differ for nearly every voice engine; openWakeWord's pre-trained models are CC BY-NC-SA; Porcupine is proprietary with mandatory telemetry; no sidecar beyond `keeper-rec`.
- **research-ai-chat §6.9:** the tool engine needs keeper-core's decision home and keeper-sync's containment; containment lives in keeper-sync because the shell does not build on Linux.
- **research-ai-chat §7:** containment is canonicalise-then-component-prefix (the CVE-2025-53110 class); every downstream CLI's permission model is in-process, not OS-enforced.
- **Notes search (AD-261…AD-268):** vectors come "from the provider you already configured, never a model keeper ships" (AD-264).

### 3.8 OKF on the drives `[REPO, makistack and drive]` (G3 §8)

- **OKF v0.2** (Google Cloud, Apache-2.0, announced 2026-06-12) is "a directory of Markdown files with YAML frontmatter… no runtime, no SDK, no schema registry": one file is one concept, the path is the identity; reserved `index.md` and `log.md`; a non-empty `type:` is the only hard requirement; provenance `sources[].id` with footnote-keyed attribution; trust actors `<producer>/<version>`, `human:<id>`, `process:<id>` — **never sign as `human:` when you are an agent**; `status`/`stale_after`; consumers preserve unknown keys and must not reject them; broken links are knowledge not yet written (the drive's `/workspace/tgdrive/.okf/OKF-0.2-digest.md`).
- **On tgdrive:** one bundle per content zone plus a root catalogue in `.okf/config.yaml` (`tgdrive`, `tgdrive-notes`, `tgdrive-records`, `tgdrive-work`, `tgdrive-media`, `tgdrive-library`, `tgdrive-sessions`, `tgdrive-comms`, `tgdrive-archive`), each `entry: README.md`. Excluded by contract: `00-inbox/**`, `99-temp/**`, `recordings/**`, the library's bulk folders, `30-work/clients/**` (a generated index of client filenames would itself be a disclosure), keeper's `*.sync-conflict-*.md` and `.keeper/**`, and `_bmad/**`.
- **Tooling:** `.okf/bin/okf` fronts four local, offline Python scripts (`validate`, `index [--check]`, `links`, `migrate`); third-party OKF tools exist, but "none of them is trusted with this content by default" because the drive's filenames are part of its stealth.
- **A new zone `80-agents/` is not in the bundle list** `[INFERENCE]`; whether it becomes a bundle, is listed, or is excluded is the owner's drive configuration, not keeper's (§13 #45).

### 3.9 Lessons `[REPO, makistack]` (G3 §9)

1. **In-process guardrails do not hold.** All 12 published defences bypassed; Hermes' own line: "the only security boundary against an adversarial LLM is the operating system".
2. **Public chat ingress is an untrusted-input firehose.** Removing the inbound channel is what made the Paseo broker grantable.
3. **Approval prompts are weak against social engineering** (Snyk: one confirmation was enough); approve durable rules; approval fatigue is a failure mode.
4. **Read-write notes or memory is stored prompt injection** that activates weeks later; memory and persona files stay out of untrusted-reachable write scope.
5. **Weak models with web tools and the sandbox off are the critical configuration**; never pin cheap fallbacks for untrusted-input turns.
6. **A moving tag is not a pin**; exact-pin or refuse third-party packages and skills.
7. **Multi-user means separate gateways and credentials**, never a shared `main` DM.
8. **Config inside a state volume behind a CLI drifts**; committed files bound read-only remove the class.
9. **Zero-click link-preview exfiltration**; never give a read verb that streams content where metadata suffices.
10. **VRAM sizing lies**: without a context pin, Ollama sized `num_ctx` from a bogus 46 GiB GTT reading; the 16 384 pin that now blocks local agent models was paid for.
11. **Never remove and re-add a knowledge base**; a deploy between the two destroyed a 12k-chunk index.
12. **LFS pointers are ASCII documents to any indexer**: 38 178 pointers in `00-inbox`; never index the inbox; exclude pointer zones.
13. **Telemetry write-only, outcomes unmeasured.**
14. **A bind mount without its committed file creates an empty directory**; Ansible never reaps stacks.
15. **`hermes config check` validates nothing**; verify by effect.
16. **Silent data loss on the drive**: an embedded `.git` hid 36 repositories and 84 323 files; an over-broad ignore ate 297 files; a preserved mtime hid new files; a clean `git status` is not synced LFS (tgdrive `AGENTS.md:52-72`).
17. **A naive `read_file` over a placeholder tree returns a ~130-byte LFS pointer** or mass-hydrates it.
18. **Frontend config read only on first launch** plus a `/v1`-less URL that false-passes verification: record the trap beside the variables.

### 3.10 Where this program departs from epic 22

> Coordinator note: epic 22 withdrew drive access from the operator's bots outright ("nie podpinaj
> dysku"; story 22-6 dropped), answered "no" to read-write notes (Q2) and put neuradrive entirely
> off-limits (G3 §1, §3). The owner's rounds 2–3 ask for the opposite: agents whose homes, sessions
> and memory live in tgdrive and neuradrive, a steward of neuradrive (Dr Lucyna Novak), and Nixi
> writing over the notes view. The pinned resolution keeps epic 22's reasons and changes the
> mechanism: the boundary is the operating system (one `keeper-agentd` per principal under its own
> OS user, tgdrive never mounted for `agentd-neuraffica`, P5), every drive access goes through
> keeper's grants and `browse::resolve` (§2.3), memory and `SOUL.md` are written only by the
> consolidator or a human (P12, §9.4), and consequential calls under `untrusted` integrity are
> blocked or need approval (P8, §9.7). Epic 22's own lesson 1 ("the only security boundary … is the
> operating system") is the reason for P5's process split.
>
> **Ruling R16** (2026-10-02) records it in the coordinator's words: epic 22's refusals scoped
> Hermes, not keeper. They are not reversed; makistack's record stands, and Q1 survives here as a
> T3 approval on every Paseo `create_agent`.

- **The Paseo broker stays the shape for coding.** Story 96.3 ("coding through Paseo") meets Q1's refusal only through 22-7's broker: four verbs, the PR as the airlock, attached to one agent `[INFERENCE]`.
- **Hermes is not a runtime here.** keeper reads its own `SOUL.md` from the drive and never touches a Hermes profile (ruling R1); none of Hermes' CVEs ride into keeper, and none of its server-side tool execution either (§3.7).

### 3.11 The infrastructure the agents run on `[REPO, makistack]` (G5 §6)

| Host | Role |
| --- | --- |
| tetyda | Proxmox VE hypervisor (Minisforum N5 Pro, Ryzen AI 9 HX 370, 96 GB); `ci_runner` |
| electra (vmid 103) | Docker services and AI workloads (21 stacks), Caddy, Samba, Tailscale; tuwunel; always on |
| delectra (105) | on-demand dev sandbox (8 GB); runs `keeper-test-synapse` |
| ollama (106, LXC) | Ollama on the Radeon 890M (`gfx1150`) and XDNA NPU; Vulkan/RADV; `OLLAMA_CONTEXT_LENGTH=16384`; `qwen3:4b` 27.3 tok/s |
| truenas (100) | ZFS pool `maia`, SMB shares |
| driada (101), plejada (102) | Windows workstations (tgorka, Marta) |
| hesperia, kalypso | the operator's always-on Mac and the keeper iPhone (keeper's docs), not in `hosts.yml` `[INFERENCE, G5]` |

- **tuwunel** (`ghcr.io/matrix-construct/tuwunel`, RocksDB) on electra `:8008`: federation off, registration closed, tailnet only — today Dixi's DM bus (`AGENTS.md:58`).
- **Synapse** exists only as keeper's test homeserver `keeper-test-synapse` on delectra.
- **No push gateway** (no Sygnal; `ntfy` only as an Uptime Kuma option), **no LiveKit or Element Call**.
- **neuraffica** is the second principal: `superset-host-neuraffica` (Paseo/omp over `/home/tgorka/prj-neuraffica`), `neuranotes` (Nextcloud, group `neuraffica` = tgorka + Marta), and **neuradrive**, an LFS-pointer-only `keeper-syncd` checkout (~3 GB of a 429 GB drive), pull-only since 2026-09-09.
- **CLIProxyAPI has zero hits in makistack** (grep 2026-10-01). Ruling R13 names it at `https://electra.siren-alsephina.ts.net:8452`, so it runs on electra outside makistack's infrastructure code `[INFERENCE]` (§11.1).

---

## 4. Agent products and harnesses

All of §4 is R1's reading, `[SOURCE]` unless marked.

### 4.1 Which "Grok bot", and which "DeepSeek harness"

| Candidate | Evidence | Verdict |
| --- | --- | --- |
| **Grok Bot** (xAI, August 2026): named Bots on a shared cloud computer | [overview](https://docs.x.ai/grok-bot/overview), [launch](https://x.ai/news/introducing-grok-bot) | the most plausible reading: the product is called "Grok Bot" |
| Grok Build (`grok`), the official Rust coding TUI | [repo](https://github.com/xai-org/grok-build) | analysed as the harness candidate |
| superagent-ai/grok-cli, a community tool driveable from a Telegram bot | [README](https://github.com/superagent-ai/grok-cli/blob/main/README.md) | a short note only |
| Grok companions (Ani); the @grok reply account on X | [grokani.org](https://grokani.org/) (not an xAI page) | consumer chat, not a harness; design `[UNVERIFIED]` |
| **DeepSeek Harness (`dsh`)**: official, MIT, released 2026-08-13 | [repo](https://github.com/deepseek-ai/deepseek-harness), [composio](https://composio.dev/content/deepseek-harness-vd-pi-agent) | analysed |
| DeepSeek-TUI / "CodeWhale" (community, Rust) | [lib.rs](https://lib.rs/crates/deepseek-tui-cli), [devdigest](https://www.developersdigest.tech/tools/deepseek-tui) | not analysed |

### 4.2 The products `[SOURCE]` (R1 *Products*)

**Hermes Agent** (Nous Research; Python, MIT; [repo](https://github.com/NousResearch/hermes-agent))
- **Sessions:** one SQLite file, `~/.hermes/state.db`, with FTS5; compression splits record a `parent_session_id`; `/branch` children, archive, unarchive and pin; export to JSONL, Markdown or HTML; older JSONL transcripts no longer read; `/handoff telegram` moves a live CLI session to a messaging app ([sessions](https://hermes-agent.nousresearch.com/docs/user-guide/sessions)).
- **Multi-agent:** a **profile** is a separate home with its own `SOUL.md`, memory, sessions, cron jobs and `state.db`; never two processes on one home ([profiles](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/profiles.md)). `delegate_task` runs subagents, 10 concurrent and depth 1 by default; leaf children cannot write memory, send messages or schedule cron; completed results are durable, but a running child is **not** resumed after a restart ([delegation](https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation)).
- **Board:** a durable kanban in SQLite (`kanban.db`): triage → todo → ready → running → blocked → review → done; linked parents gate children; comments are how agents talk; the dispatcher reclaims stale claims and dead PIDs; workers see only their board through `HERMES_KANBAN_BOARD`; explicitly single-host ([kanban](https://hermes-agent.nousresearch.com/docs/user-guide/features/kanban)).
- **Cron:** `~/.hermes/cron/jobs.json`, results delivered to any platform ([cron](https://hermes-agent.nousresearch.com/docs/user-guide/features/cron)).
- **Memory:** `MEMORY.md` and `USER.md`, self-created and self-refined skills, optional Honcho (§9.1).
- **Approvals:** `smart` (an extra LLM judges risk), `manual`, `off`; yes/no replies on messaging apps; the prompt times out after 300 s and is denied; an expired prompt **cannot be reopened**; a hard blocklist applies even in YOLO mode; once / session / always, and "always" writes `config.yaml` ([security](https://hermes-agent.nousresearch.com/docs/user-guide/security)).
- **Gateways:** about 20, including Telegram, Signal, WhatsApp, Matrix, email, SMS and iMessage via BlueBubbles.
- **Weakness:** the regex Skills Guard could be bypassed completely, letting a skill exfiltrate environment variables (P0, [#7072](https://github.com/NousResearch/hermes-agent/issues/7072)).

**OpenClaw** (formerly Clawdbot/Moltbot; TypeScript on Node; the README badge says MIT, the GitHub API reports "Other"; [repo](https://github.com/openclaw/openclaw))
- **Sessions:** one SQLite database per agent (`openclaw-agent.sqlite`); transcripts archived as files; legacy `sessions.json`/JSONL migrated by `doctor --fix`; optional daily or idle resets ([session](https://docs.openclaw.ai/concepts/session)).
- **Multi-agent:** each agent has its own workspace (`SOUL.md`/`AGENTS.md`), state directory and session store; **bindings** route channel accounts or contacts to agents; a coordinator "team" preset; `sessions_spawn` subagents announce results and can fork the parent's context; `tools.agentToAgent` controls agent-to-agent access ([multi-agent](https://docs.openclaw.ai/concepts/multi-agent), [subagents](https://docs.openclaw.ai/tools/subagents)).
- **Board:** an optional Workboard plugin with claims, heartbeats, parent/child links, decompose, and replay-safe notification cursors ([workboard](https://docs.openclaw.ai/plugins/workboard)).
- **Cron:** "Automations" — one `GatewayScheduler` timer; deadlines live in each owner's store, rebuilt at start, missed ticks after sleep merged ([architecture](https://docs.openclaw.ai/concepts/architecture), [automations](https://docs.openclaw.ai/automation/cron-jobs)).
- **Memory:** `USER.md`, `MEMORY.md`, daily `memory/YYYY-MM-DD.md`; a "dreaming" sweep consolidates; a memory flush runs before compaction ([memory](https://docs.openclaw.ai/concepts/memory)).
- **Approvals:** policy deny / allowlist / ask / auto / full in SQLite; an approval is tied to the exact argv, cwd and resolved executable (plus a content hash for a writable file) and re-checked before launch; with no UI reachable the fallback is deny; closing the turn cancels a pending approval, so it is not durable ([exec-approvals](https://docs.openclaw.ai/tools/exec-approvals)).
- **Devices:** macOS, iOS and Android apps pair as signed **nodes** over one WebSocket gateway.
- **Incidents:** the one-click RCE CVE-2026-25253 ([ProArch](https://www.proarch.com/blog/threats-vulnerabilities/openclaw-rce-vulnerability-cve-2026-25253)); ClawJacked ([Oasis](https://www.oasis.security/blog/openclaw-vulnerability)); 341 malicious ClawHub skills, about 12% of the registry ([Conscia](https://conscia.com/blog/the-openclaw-security-crisis/)); about 135k instances exposed without authentication ([AdminByRequest](https://www.adminbyrequest.com/en/blogs/openclaw-went-from-viral-ai-agent-to-security-crisis-in-just-three-weeks)).

**Grok Bot** (xAI, hosted by Cursor; proprietary; desktop on macOS, Windows and Linux, mobile on iOS and Android, the Bots' computers in Cursor's cloud; [overview](https://docs.x.ai/grok-bot/overview))
- **Personas:** named Bots, each with its own memory; all Bots share **one** cloud computer per user; xAI states that Bots are not a security boundary from each other.
- **Multi-agent:** group chats of 2–6 Bots, `@mentions`, asynchronous Bot-to-Bot messages that wake the receiver, and handing ownership of a task between Bots ([collaboration](https://docs.x.ai/grok-bot/chat-and-collaboration.md)).
- **Routines:** by schedule or event, up to 50 per Bot, the 20 most recent runs kept; one shared skill library, learnable by demonstration ([routines](https://docs.x.ai/grok-bot/skills-routines-and-automations.md)).
- **Approvals:** Allow once / Always allow / Deny; Auto Review adds "Ask first" rules, which always win; running commands on your own computer defaults to "Ask every time"; secrets are masked and kept out of the transcript; rules are stored per desktop install ([approvals](https://docs.x.ai/grok-bot/approvals-security-and-privacy)).
- **Not established:** session storage and branching (§14).

**Grok Build** (Rust, Apache-2.0, no external contributions; [repo](https://github.com/xai-org/grok-build)) and **grok-cli**
- **Sessions:** one **directory** per session under `~/.grok/sessions/<cwd>/<id>/`: `summary.json` (the index entry), `updates.jsonl` (the authoritative log), `plan.json` (todos), `rewind_points.jsonl`, `subagents/`; `/fork` can run in a new worktree; `/rewind` truncates history but restores no files; an SQLite FTS5 index serves search only ([sessions](https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-pager/docs/user-guide/17-sessions.md)).
- **Multi-agent:** subagents one level deep; personas are TOML files with input/output contracts; `send_subagent_message` steers, queues or interjects within a quota ([subagents](https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-pager/docs/user-guide/16-subagents.md)).
- **Approvals:** ask, auto (a classifier), dontAsk, always-approve; deny > ask > allow; "always allow" per project in `permission.toml`; hooks fail open ([permissions](https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-pager/docs/user-guide/22-permissions-and-safety.md)).
- **superagent grok-cli** (TypeScript, MIT): Telegram remote control via `/pair`, schedules run by a daemon, a computer-use subagent on macOS ([README](https://github.com/superagent-ai/grok-cli/blob/main/README.md)).

**DeepSeek Harness (`dsh`)** (TypeScript, MIT, built on Cordis, "everything is a plugin"; [repo](https://github.com/deepseek-ai/deepseek-harness))
- **Sessions:** an append-only JSONL event log stored as checksummed zstd frames; appends fsynced per batch; a torn tail cut off before the next append; one writer per session; a fork is a seed plus an `inheritedEventCount`; invariant: **anything the model sees must be in the log** ([persistence](https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/docs/subsystems/persistence.md), [architecture](https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/docs/architecture.md)).
- **Multi-agent:** experimental Agent Teams — a durable roster, a task DAG, and a mailbox recovered as "queued minus delivered" ([agent-team](https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/docs/subsystems/agent-team.md)).
- **Todo and cron:** a todo package and durable reminder schedules ([subsystems](https://github.com/deepseek-ai/deepseek-harness/tree/master/docs/subsystems)).
- **Tools:** shell, LSP, MCP, SSH, browser use, computer use (Cua), webhooks.
- **Approvals:** outcomes `allowed-once | rejected | cancelled | unavailable`; anything but `allowed-once` is denied; the per-session `ask`/`never` policy is itself a log event, so replay rebuilds it ([approval](https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/docs/subsystems/approval.md)).
- **Interfaces:** Web UI (127.0.0.1:3080), desktop, ACP, SDK. **Weakness:** an unaudited developer preview ([SAFETY](https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/SAFETY.md)).

**pi** (pi-mono, now earendil-works/pi; TypeScript, MIT; `badlogic/pi-mono` answers 301 to `earendil-works/pi`; [repo](https://github.com/earendil-works/pi)) and **omp** (oh-my-pi; TypeScript plus a Rust native core, MIT; [repo](https://github.com/can1357/oh-my-pi))
- **pi sessions** are a **JSONL tree** (`id`/`parentId`); `/tree` branches in place, `/fork` and `/clone` create files; system-prompt and tool changes are logged too ([format](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/docs/session-format.md), [sessions](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/docs/sessions.md)). MCP and skills, **no built-in permission system**. `pi-durable` (experimental) commits each turn before showing it ([durable](https://raw.githubusercontent.com/earendil-works/pi/main/packages/durable/README.md)). pi-chat covers Slack.
- **omp sessions** extend pi's tree with a content-addressed blob store and a `history.db` index ([session](https://raw.githubusercontent.com/can1357/oh-my-pi/main/docs/session.md)). `task` subagents in isolated worktrees return schema-validated results; peers message each other; an advisor model reviews turns; `todo`; retain/recall memory; a `computer` tool; `/collab` shares a session through a relay link. Approval tiers read/write/exec, and the **default mode is yolo** ([approval](https://raw.githubusercontent.com/can1357/oh-my-pi/main/docs/approval-mode.md)).

**LangChain deepagents and LangGraph** (Python, MIT; the harness "trusts the LLM", boundaries come from tools or sandboxes; [repo](https://github.com/langchain-ai/deepagents))
- **Sessions:** a thread is a sequence of checkpoints (InMemory, SQLite, Postgres) with "time travel"; a Store gives memory across threads ([persistence](https://docs.langchain.com/oss/python/langgraph/persistence)).
- **Features:** subagents through a `task` tool, opt-in `write_todos`, `AGENTS.md` memory, skills, pluggable filesystem back ends ([overview](https://docs.langchain.com/oss/python/deepagents/overview)).
- **Approvals:** `interrupt_on` per tool with approve / edit / reject / respond, and a checkpointer is **required**; `interrupt()` saves state and waits indefinitely; resume with `Command(resume=...)` on the same `thread_id`; parallel interrupts are matched by id; `response_schema` validates resume input; caveat: on resume the interrupted node **runs again from its start** ([HITL](https://docs.langchain.com/oss/python/deepagents/human-in-the-loop), [interrupts](https://docs.langchain.com/oss/python/langgraph/interrupts)).
- **Gateways:** none native.

**OpenAI Codex CLI** (codex-rs; Rust, Apache-2.0; [Cargo.toml](https://raw.githubusercontent.com/openai/codex/main/codex-rs/Cargo.toml))
- **Sessions:** rollout JSONL files plus an SQLite metadata database behind a `ThreadStore` trait ([thread-store](https://github.com/openai/codex/tree/main/codex-rs/thread-store)); the app server has `thread/fork` (with `lastTurnId`), `thread/archive` (moves the JSONL into an archive directory, with its child threads), `thread/unarchive`, `thread/resume` ([app-server](https://learn.chatgpt.com/docs/app-server.md)).
- **Multi-agent:** subagents, custom agents in `~/.codex/agents/*.toml`, `/agent` to switch threads ([subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents)).
- **Approvals:** approval policy and sandbox mode are separate; the client receives JSON-RPC `requestApproval`; `acceptWithExecpolicyAmendment` turns an approval into a rule; **auto-review** hands approvals to a reviewer agent and stops the turn after 3 denials in a row or 10 in the last 50 ([auto-review](https://learn.chatgpt.com/docs/sandboxing/auto-review.md)); `execpolicy` rules are Starlark with match/not_match examples checked at load ([execpolicy](https://github.com/openai/codex/tree/main/codex-rs/execpolicy)).
- **Memory and automations** are desktop-app features ([memories](https://learn.chatgpt.com/docs/customization/memories), [automations](https://developers.openai.com/codex/app/automations)).
- **Incident:** CVE-2025-61260, a project-local MCP config led to code execution ([Check Point](https://research.checkpoint.com/2025/openai-codex-cli-command-injection-vulnerability/)).
- **Crates:** on 2026-10-01 `codex-execpolicy`, `codex-core`, `codex-thread-store`, `codex-rollout`, `codex-linux-sandbox`, `codex-apply-patch` and `codex-rmcp-client` were **not published**; `codex-protocol` on crates.io is a third-party fork. Reuse means a git dependency or copying at a pinned commit (§5.2).

**goose** (Block, now under the Linux Foundation's AAIF; Rust, Apache-2.0; [repo](https://github.com/aaif-goose/goose))
- **Sessions:** SQLite (`sessions.db`); JSONL files before v1.10 ([logs](https://goose-docs.ai/docs/guides/logs)); fork from an edited message, duplicate, JSON export and import ([sessions](https://goose-docs.ai/docs/guides/sessions/session-management/)).
- **Features:** subagents and subrecipes; `goose schedule` runs recipes on cron ([CLI](https://goose-docs.ai/docs/guides/goose-cli-commands/)); 70+ MCP extensions.
- **Approvals:** Autonomous (the **default**), Manual, Smart, Chat-only ([permissions](https://goose-docs.ai/docs/guides/goose-permissions/)).
- **Multi-device:** `goose-roaming`, a standalone Rust library: peer-to-peer ACP over iroh QUIC with a mutual public-key allowlist (`TrustBook`) re-read on every inbound connection and closed if the reload fails ([roaming](https://github.com/aaif-goose/goose/tree/main/crates/goose-roaming)). `goose-sdk` is published (0.1.0-alpha.11); goose-roaming is not.
- **Incident:** "Operation Pale Fire", Block's own red-team using goose as the entry point ([Block](https://engineering.block.xyz/blog/how-we-red-teamed-our-own-ai-agent-)).

**Violoop** (BVIO Technology; hardware)
- HDMI in, USB keyboard and mouse out; a Qwen 8B model on the main chip; a **separate STM32H563** is the only thing that sends input, and irreversible actions need a **physical key press** ([Violoop](https://violoop.ai/blog/best-ai-agents-for-computer-automation-2026/)).
- MindStudio's test: reversible actions ran at once; one spoken instruction set up recurring monitoring; memory is reviewable in a companion app; each job is logged with model and token cost; the claim that frames are discarded could not be verified ([MindStudio](https://www.mindstudio.ai/blog/violoop-ai-hardware-agent-mac)). Not shipped (Kickstarter); §7.4 has its approval model.

### 4.3 Comparison `[SOURCE]` (R1 *Comparison*)

| Product | Language / licence | Sessions | Multi-agent | Board / todo | Cron | Memory | Tools | Approvals | Gateways | Weakness |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Hermes | Py / MIT | SQLite; branch, archive | profiles, delegate, kanban | SQLite kanban | jobs.json | MD + skills | shell, browser, MCP | smart/manual; 300 s timeout | ~20 incl. iMessage | skill-scanner bypass |
| OpenClaw | TS / MIT? | SQLite per agent; archive | bindings, subagents | Workboard | Automations | MD + dreaming | exec, nodes, browser | argv-bound; not durable | 20+ channels, nodes | CVEs, ClawHavoc |
| Grok Bot | proprietary | cloud `[UNVERIFIED]` | Bots, groups, async messages | — | routines | per Bot | cloud computer | Auto Review rules | desktop + mobile | shared computer |
| Grok Build | Rust / Apache | per-session dir; fork | depth 1, personas | plan.json | /loop | /flush, /dream | shell, MCP | modes + rules | ACP | hooks fail open |
| dsh | TS / MIT | event-log JSONL; seeded fork | Agent Teams | todo + task DAG | reminders | skills | computer, browser | ask/never, fail-closed | Web/ACP | preview |
| pi | TS / MIT | JSONL tree | ext / pi-durable | — | — | skills | shell, MCP | none | pi-chat | no permissions |
| omp | TS+Rust / MIT | JSONL tree + blobs | task, worktrees, peers | todo | — | retain/recall | computer, browser, LSP | tiers; yolo default | /collab | yolo default |
| deepagents | Py / MIT | checkpoints | task subagents | write_todos | `[UNVERIFIED]` | AGENTS.md, Store | FS back ends | durable interrupt | — | trusts the LLM |
| Codex | Rust / Apache | JSONL + SQLite; fork, archive | subagents, roles | `[UNVERIFIED]` | app only | memories | sandboxed shell | policy + auto-review | app-server | CVE-2025-61260 |
| goose | Rust / Apache | SQLite; fork | subagents, recipes | — | schedule | extension | MCP | autonomous default | roaming P2P | Pale Fire |
| Violoop | hardware, closed | on device | — | — | monitoring jobs | reviewable | HID input | physical key | companion app | unshipped |

- **What none of them has** `[INFERENCE, R1]`: a pending approval that survives a process restart, except LangGraph's documented durable interrupt (§14). Hermes' expires and cannot be reopened; OpenClaw's dies with the turn.
- **What all the file-based ones share:** a session is a single-writer log, and the index is derived.

### 4.4 The ten patterns, and where the program uses each

R1 named ten patterns worth copying for a Rust, file-backed, cross-device system `[SOURCE]`. The third column is this program's use `[INFERENCE]` over the pins.

| # | Pattern (R1) | Where the program uses it |
| --- | --- | --- |
| 1 | **Append-only JSONL session tree with `id`/`parentId`**; branching inside one file; prompt and tool changes logged — pi ([format](https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/docs/session-format.md)) | ruling R3's chunked event log; prompt and tool changes are events. Branching in place is not pinned. |
| 2 | **One directory per session of small files**: `summary.json` the index, `updates.jsonl` the authority, `plan.json` and `rewind_points.jsonl` beside it; syncs at file granularity — Grok Build ([sessions](https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-pager/docs/user-guide/17-sessions.md)) | P3: the existing session folder gains `agent.toml`, `log/`, `approvals/`; `.keeper/` holds the rebuildable index |
| 3 | **"Anything the model sees is logged"**: one writer, a flush barrier, torn-tail truncation, forks by seed plus offset — dsh ([persistence](https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/docs/subsystems/persistence.md)) | ruling R3: one writer per file (the claim holder's host), fsync at turn end, the host's own torn tail truncated on open; story 89.5's replay with tool steps closes G1's lost-trace gap (§2.3) |
| 4 | **An approval is a saved checkpoint** with an id and a typed resume schema, answerable from another device later — LangGraph ([interrupts](https://docs.langchain.com/oss/python/langgraph/interrupts)) | P9: `approvals/<ulid>.json`, a parked run, consume once (§7.7) |
| 5 | **Approval policy as a log event**; closed outcomes; anything but "allowed once" denied; replay rebuilds policy — dsh ([approval](https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/docs/subsystems/approval.md)) | P9: the decision is a Matrix event and a decision file; scopes `once` / `session`, never `always` at T4+ |
| 6 | **Durable kanban**: claims with TTL and heartbeat, link-gated promotion, comments as the inter-agent channel, workers pinned to one board — Hermes ([kanban](https://hermes-agent.nousresearch.com/docs/user-guide/features/kanban)) | ruling R2's `run:` badge and card fields on keeper's existing board; P6's claims (TTL 180 s, renew 60 s) |
| 7 | **Durable mailbox recovered as "queued minus delivered"**; delivered only once the target stored it durably — dsh Agent Teams ([agent-team](https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/docs/subsystems/agent-team.md)) | P4: a Matrix room is the mailbox; the homeserver stores until sync (R7 §1) |
| 8 | **A persona is a home directory with a single writer**; bindings route messages to personas — Hermes profiles ([profiles](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/profiles.md)), OpenClaw bindings ([multi-agent](https://docs.openclaw.ai/concepts/multi-agent)) | P2 / ruling R1: `80-agents/<agent>/`; a Matrix user per agent is the binding |
| 9 | **Layered command policy**: Starlark prefix rules with built-in tested examples, strictest wins, approvals that become rules — Codex execpolicy ([execpolicy](https://github.com/openai/codex/tree/main/codex-rs/execpolicy)); approval bound to exact argv and executable — OpenClaw ([exec-approvals](https://docs.openclaw.ai/tools/exec-approvals)) | story 96.1's argv-bound `run`; P9's `exec_binding` in the digest |
| 10 | **Device trust as a mutual key allowlist**, reloaded per connection — goose-roaming ([roaming](https://github.com/aaif-goose/goose/tree/main/crates/goose-roaming)); a confirmation channel the model cannot reach — Violoop ([Violoop](https://violoop.ai/blog/best-ai-agents-for-computer-automation-2026/)) | ruling R12: a decision counts only from a verified Matrix device of a human reader; the model has no key |

---

## 5. Rust building blocks

All of §5 is R2's reading, `[SOURCE]` from the crates.io API (R2's key `CIO(x)` = `https://crates.io/api/v1/crates/x`), the GitHub API (`GH(o/r)` = `https://api.github.com/repos/o/r`) and each crate's own files, unless marked. Downloads are all-time / last 90 days. **Nothing was compiled**, so every "iOS: yes" below is `[INFERENCE]` or `[UNVERIFIED]` (§14).

### 5.1 LLM clients and agent frameworks

| Crate | Purpose | Licence | Version / last release | DL · ★ | iOS | Sidecar |
| --- | --- | --- | --- | --- | --- | --- |
| rig-core (+ rig-agent, rig-memory) | agent framework: providers, tools, hooks, memory, vector adapters | MIT | 0.43.0 / 2026-09-30 | 3.14M / 1.71M · 8.8k | `[INFERENCE]` yes | no |
| swiftide | harness + streaming RAG | MIT | 0.32.1 / **2025-11-15** | 85k / 2k · 787 | `[INFERENCE]` likely | no |
| autoagents | ReAct executors, WASM tool sandbox | MIT OR Apache-2.0 | 0.4.0 / 2026-07-08 | 15k / 4k · 760 | `[UNVERIFIED]` | no |
| adk-rust | port of Google's ADK, 43 crates | Apache-2.0 | 2.2.0 / 2026-09-01 | 32k / 20k · 689 | `[UNVERIFIED]` | no |
| langchain-rust | port of LangChain | MIT | 4.6.0 / **2024-10-06** | 157k / 11k · 1.3k | — | no |
| genai | native-protocol client, 27+ providers | MIT OR Apache-2.0 | 0.6.5 (0.7.0-rc.1 2026-09-27) | 430k / 181k · 896 | `[INFERENCE]` yes | no |
| async-openai | typed client for OpenAI and compatibles | MIT | 0.42.1 / 2026-09-28 | 8.76M / 2.69M · 2.0k | `[INFERENCE]` yes | no |
| openai-api-rs | OpenAI client | MIT | 10.0.1 / 2026-04-17 | 740k / 98k · 486 | `[INFERENCE]` yes | no |
| llm-chain | chains | MIT | 0.13.0 / **2023-11-15** | 98k | — | no |
| llm (graniet) | several back ends | MIT | 1.3.8 / 2026-04-19 | 122k / 16k | `[UNVERIFIED]` | no |
| mistralrs | local inference on candle | MIT | 0.8.1 / 2026-04-02 | 204k / 138k · 7.7k | `[UNVERIFIED]` | no (server optional) |

- **rig** ([repo](https://github.com/0xPlaygrounds/rig)): `AgentHook` has typed events `TextDelta`, `ReasoningDelta`, `ToolCallDelta`, `RunStart`, `RunSettled`; `DispatchAction::{Proceed, Patch, Deny}` per tool call ("a `Deny` of kind `Cancelled` cancels the run"); `ModelTurnAction::{stop, retry_with_feedback}`; a `ConversationMemory` trait (`load`, `append`, `clear`), `Compactor`, `DemotionHook`. **25 rig-core releases since 2025-10-01**, all 0.x.
- **swiftide** ([repo](https://github.com/bosun-ai/swiftide)) promises "lifecycle hooks, and stop conditions" and "pause and resume … for human approval"; pushed 2026-09-28, but nothing published since 2025-11.
- **adk-rust** ([repo](https://github.com/zavora-ai/adk-rust)) claims "SQLite checkpointers", "approval interrupts bound to a digest" and "Governed Computer Use"; GitHub says NOASSERTION, the LICENSE is Apache-2.0.
- **genai** ([repo](https://github.com/jeremychone/rust-genai)): `exec_chat_stream`, `ChatOptions::with_tool_choice`, custom endpoints through `GENAI_{n}_ENDPOINT`.
- **async-openai** ([repo](https://github.com/64bit/async-openai)): SSE streaming, a `responses` feature, `*_byot` methods "when shape of request/response in OpenAI-compatible APIs don't exactly match", WASM builds.
- **openai-api-rs**: `ChatCompletionStream`, `ResponseStream` (`src/v1/api.rs`).

### 5.2 codex-rs and goose as code, not dependencies

- **codex-rs** (Apache-2.0, 127k★, [repo](https://github.com/openai/codex)): workspace version `0.0.0`; `codex-core`, `codex-exec`, `codex-apply-patch`, `codex-sandboxing` are not on crates.io; `codex-apply-patch` depends on `codex-exec-server`, `tree-sitter-bash` and `similar`. Sandboxing always launches a process: `seatbelt.rs` runs `/usr/bin/sandbox-exec` with bundled `.sbpl` policies; `landlock.rs` re-executes the binary as `codex-linux-sandbox`; the `codex-bwrap` crate compiles vendored bubblewrap C, **LGPL-2.0-or-later**. Reuse means copying source at a pinned commit.
- **goose** (`aaif-goose/goose`, Apache-2.0, 54.8k★, v1.52.0 of 2026-09-23) is a whole application; only `goose-sdk` 0.1.0-alpha.11 is published (873 downloads), with UniFFI bindings for Python and Kotlin, **not Swift**; the workspace pins `rmcp` 3.4.1 and `agent-client-protocol` 2.2.0. The `goose` crate on crates.io is an unrelated load tester.

### 5.3 Protocols

| Crate | Licence | Version / date | DL · ★ | Notes |
| --- | --- | --- | --- | --- |
| **rmcp** | Apache-2.0, residual MIT | 3.5.0 / 2026-09-28 | 30.7M / 16.2M · 4.0k | features `client`, `server`, `transport-io`, `transport-async-rw` (in-process duplex), `transport-child-process`, `transport-streamable-http-{client,server}`, `auth`, `elicitation` ([rust-sdk](https://github.com/modelcontextprotocol/rust-sdk)) |
| **agent-client-protocol** | Apache-2.0 | 2.2.0 / 2026-09-18 | 4.79M / 1.85M · 212 | Client, Agent, Proxy and Conductor roles; `-http`, `-rmcp`; draft v2 resume/fork behind `unstable_*` ([rust-sdk](https://github.com/agentclientprotocol/rust-sdk)) |
| a2a-lf (a2aproject/a2a-rs) | Apache-2.0 | 0.4.1 / 2026-09-30 | 67k · 87 | created 2026-04-03 ([repo](https://github.com/a2aproject/a2a-rs)) |
| a2a-rs (EmilLindfors) | MIT | 0.10.0 / 2026-09-23 | 12k · 87 | community |
| a2a-protocol-types (tomtom215) | Apache-2.0 | 0.14.1 / 2026-09-30 | 60k | types only, "no I/O" |
| ag-ui-core / ag-ui-client | MIT | 0.1.0 / **2025-08-12** | 21k | its TODO lists state handling, docs and JSON Patch as open ([ag-ui](https://github.com/ag-ui-protocol/ag-ui)) |

- **On iOS** rmcp's `transport-child-process` cannot work, because a sandboxed iOS app cannot spawn subprocesses (Stack Overflow answers [36531728](https://stackoverflow.com/questions/36531728) and [15893849](https://stackoverflow.com/questions/15893849); not an Apple primary source). The in-process duplex and HTTP transports are fine there `[INFERENCE, R2]`.

### 5.4 Local RAG and search

| Crate | Licence | Version / date | DL · ★ | iOS | Notes |
| --- | --- | --- | --- | --- | --- |
| fastembed | Apache-2.0 | 7.1.0 / 2026-09-22 | 3.7M / 1.9M · 1.0k | via ort `[INFERENCE]` | pins `ort =2.0.0-rc.13`; default features download ORT binaries and use hf-hub; `tokenizers` with `onig` (C) |
| ort | MIT OR Apache-2.0 | 2.0.0-**rc.13** / 2026-07-28 | 20.1M · 2.5k | **prebuilt for `aarch64-apple-ios` and `-ios-sim` (CoreML)** per `ort-sys/build/download/dist.tsv` | still a release candidate |
| candle-core | MIT OR Apache-2.0 | 0.11.0 / 2026-06-26 | 8.9M · 21.1k | `[UNVERIFIED]` | Metal through `objc2-metal` |
| model2vec-rs | MIT | 0.3.0 / 2026-09-19 | 194k · 217 | `[INFERENCE]` yes | light fallback |
| embed_anything | Apache-2.0 | 0.7.1 / 2026-07-10 | 46k · 1.3k | — | **needs Poppler CLIs** through `pdf2image`; pins ort rc.10 |
| tantivy | MIT | 0.26.2 / 2026-09-08 | 19.0M · 16.2k | `[INFERENCE]` yes | BM25 |
| sqlite-vec | MIT/Apache-2.0 | 0.1.9 (0.1.10-alpha.4) | 3.1M · 8.2k | "runs anywhere SQLite runs" | pre-v1, pure C |
| sqlite-vss | MIT/Apache-2.0 | 0.1.2 / **2023-08** | 20k | — | replaced by sqlite-vec |
| lancedb | Apache-2.0 | 0.39.0 / 2026-09-17 | 1.38M · 11.6k | `[UNVERIFIED]` | heavy Arrow stack |
| usearch | Apache-2.0 | 2.26.2 / 2026-08-31 | 1.37M · 4.3k | README lists iOS | C++ |
| hnsw_rs | MIT/Apache-2.0 | 0.3.4 / 2026-02-28 | 1.05M | `[INFERENCE]` yes | — |
| arroy (sibling hannoy 0.2.0) | MIT | 0.8.0 / 2026-08-12 | 508k · 312 | `[INFERENCE]` yes | LMDB via heed |
| text-splitter | MIT | 0.33.0 / 2026-09-24 | 2.27M · 633 | `[INFERENCE]` yes | optional `tokenizers` |

- **keeper already has the search stack it needs** for story 95.4's `drive_search`: FTS5 plus brute-force cosine in `search.db`, with vectors from the configured provider (AD-264; §2.7). Adding tantivy or an ANN index would be a second index against DW-262's measured scale `[INFERENCE]`.

### 5.5 Commands, terminals and sandboxing

| Crate / mechanism | Licence | Version / date | Notes |
| --- | --- | --- | --- |
| portable-pty | MIT | 0.9.0 / 2025-02-11 (17.6M) | spawns the shell; impossible on iOS |
| alacritty_terminal | Apache-2.0 | 0.26.0 / 2026-04-06 | terminal state and grid |
| vt100 | MIT | 0.16.2 / 2025-07-12 | light screen parser |
| wezterm-term | MIT | not on crates.io | git dependency or the `tattoy-wezterm-term` fork; termwiz 0.23.3 is published |
| landlock | MIT OR Apache-2.0 | 0.4.7 / 2026-07-27 | Linux only; restricts the applying thread, so call it in the child before exec `[INFERENCE, R2]` |
| nix | MIT | 0.31.3 / 2026-05-11 | Unix syscalls |
| macOS Seatbelt | — | no crate | codex shells out to `/usr/bin/sandbox-exec` with SBPL files |
| bubblewrap | **LGPL-2.0-or-later** | — | an external `bwrap` binary only |
| birdcage | **GPL-3.0-or-later** | 0.8.1 / 2024-04 | rejected on licence |

### 5.6 Durable workflows and scheduling

| Crate | Licence | Version / date | Notes |
| --- | --- | --- | --- |
| duroxide (Microsoft) | MIT | 0.1.30 / 2026-07-29 | "embeddable durable execution runtime… runs in-process on Tokio"; SQLite provider; human-in-the-loop waits; durable timers; "**Preview**" ([repo](https://github.com/microsoft/duroxide)) |
| apalis | MIT | 0.7.4; **1.0.0-rc.10** 2026-09-15 | Redis, PostgreSQL, SQLite, in-memory; `apalis-workflow`, `apalis-cron` |
| croner | MIT | 4.0.0 / 2026-08-31 | cron parser and next run |
| cron | MIT OR Apache-2.0 | 0.17.0 / 2026-06-18 | cron parser |
| tokio-cron-scheduler | MIT/Apache-2.0 | 0.15.1 / 2025-10-28 | persistence only through Postgres or NATS |
| restate-sdk | MIT | 0.12.1 / 2026-09-22 | needs the Restate **server**, BUSL-1.1 |

- **keeper already has a schedule grammar and a due-gate** (`tasks.rs:9-49`, AD-62/136); ruling R9 reuses keeper-sync's pure parser for a card's `schedule:`, so none of these crates is needed `[INFERENCE]`.

### 5.7 Computer use and OS automation

| Crate | Licence | Version / date | Platforms |
| --- | --- | --- | --- |
| enigo | MIT | 0.6.1 / 2025-08-28 | macOS, Windows, X11; Wayland and libei "Experimental"; no iOS |
| rdev | MIT | 0.5.3 / **2023-06-26** | stalled |
| objc2-application-services | Zlib OR Apache-2.0 OR MIT | 0.3.2 / 2025-10-04 | `AXUIElement` feature |
| accessibility-sys | MIT/Apache-2.0 | 0.2.0 / 2025-03-22 | raw accessibility FFI |
| xcap | Apache-2.0 | 0.9.8 / 2026-08-01 | Linux X11 and Wayland (PipeWire), macOS, Windows |
| screencapturekit | MIT OR Apache-2.0 | 11.0.0 / 2026-09-24 | macOS 13+ |
| atspi | Apache-2.0 OR MIT | 0.30.0 / 2026-05-06 | Linux over zbus; `tokio` feature |

None works on iOS, so computer use is desktop-only `[INFERENCE, R2]`.

### 5.8 R2's shortlist and reject list, and what the program takes

| Need | R2's pick | The program `[INFERENCE]` over the pins |
| --- | --- | --- |
| LLM client | async-openai (genai for native Anthropic/Gemini) | keeper's own wire (`chat.rs`), one more `ProviderKind` (P10); a second client would break AD-149's one wire |
| Agent design reference | rig-agent's hook taxonomy | the `TurnSink` / `ApprovalPort` ports of the extraction (§12.4) |
| Tools / MCP | rmcp (in-process duplex) | story 96.2's MCP servers per agent |
| Front-end protocol | agent-client-protocol | not taken: P4 routes the front end through Matrix events |
| A2A | a2a-protocol-types if interop is needed | not taken; epic 99's gate agent speaks Matrix |
| Embeddings, search | fastembed + model2vec-rs; tantivy + sqlite-vec or usearch | not taken: keeper's `search.db` and provider embeddings (§5.4) |
| Chunking | text-splitter | not taken: `notes/chunk.rs` (AD-262) |
| Terminal | portable-pty + alacritty_terminal or vt100 | not pinned; story 96.1 is argv-bound, not a PTY |
| Sandbox | landlock + nix on Linux; generated SBPL and `sandbox-exec` on macOS | story 96.1, with a seccomp filter (`seccompiler`) beside landlock, because landlock's network rights cover TCP only (ruling R24(5)) |
| Scheduling | croner + your own Tokio loop; evaluate duroxide | keeper-sync's parser and each host's tick (ruling R9) |
| Computer use | enigo, xcap / screencapturekit, objc2-application-services, atspi | story 96.4 uses Peekaboo's MCP on hesperia instead (§7.8) |

**Rejected by R2:**

| Reject | Reason |
| --- | --- |
| langchain-rust, llm-chain, sqlite-vss, rdev | stale (last release 2024-10, 2023-11, 2023-08, 2023-06) |
| swiftide, for now | no release in 10+ months while main moves |
| the AG-UI Rust SDK | 0.1.0, client only, TODO incomplete |
| embed_anything | shells out to Poppler; an older ort RC |
| restate-sdk | needs a separate BUSL-1.1 server |
| birdcage | GPL-3.0-or-later |
| codex `bwrap` / bubblewrap | LGPL; external binary only |
| codex-core / codex-exec as dependencies | unpublished (0.0.0), coupled; copy files at a pinned commit |
| goose / goose-sdk | a whole application; alpha SDK; no Swift bindings |
| tokio-cron-scheduler persistence | Postgres or NATS only, a server |
| rmcp `transport-child-process`, portable-pty on iOS | iOS forbids subprocesses |
| adk-rust, for now | 43 crates and 689★ for a young project `[INFERENCE, R2]` |

### 5.9 Build the loop, or adopt a framework

R2 recommends building "a small loop of your own on async-openai (or genai) plus rmcp, and use rig's hook design as the blueprint" `[INFERENCE, R2]`, for six reasons:

1. **Tool calling and streaming** are already in the thin clients; the loop is short: stream → collect tool-call deltas → approval gate → run tools → append results → repeat.
2. **A framework's approval gate** is only as good as rig's `DispatchAction::Deny`, an async hook held in memory. An approval that spans a device switch needs the pending state on disk, which neither rig's hooks nor its `ConversationMemory` provides (no checkpoint or serialised run type found in `hook.rs` or `rig-memory`).
3. **Cancellation** is a `CancellationToken` in a `select!` either way.
4. **Persistence as plain files that sync:** adk-rust's and duroxide's SQLite files on a file-sync service risk conflicts; an append-only JSONL log per session matches "plain files, resumable elsewhere".
5. **Stability:** rig published 25 breaking 0.x releases in 12 months, swiftide stopped releasing, adk-rust is 43 crates.
6. **What is worth having from rig is small:** the hook taxonomy and its compaction (`Compactor`, `TokenWindowMemory`).

**keeper already built that loop** `[REPO]` (D1 §1): `run_tool_loop_reporting` at `keeper-core/src/bots/tools.rs:1215` over `stream_chat` at `chat.rs:989`, with its own SSE framer and caps (§2.2). So the program neither adopts a framework nor adds a client: it extracts the loop keeper has into `keeper-agent` with no behaviour change (story 90.1, ruling R6) and adds the durable approval R2 says no framework gives (P9).

### 5.10 The licence firewall, and what a port carries `[REPO]` (D3 B, G3 §7)

- **`src-tauri/deny.toml`** is an exhaustive allow-list (Apache-2.0, MIT, BSD-2/3, ISC, Zlib, BSL-1.0, CC0, MPL-2.0, Unicode-3.0, OpenSSL, CDLA-Permissive-2.0; confidence 0.8), an explicit deny for `git2`/`libgit2-sys`, and one `allow-git` pin to `https://github.com/tgorka/gitoxide`.
- **What it cannot see:** SwiftPM dependencies (FluidAudio), and the prebuilt `onnxruntime` dylib `ort` downloads, which is not a cargo dependency, so its licence needs manual verification `[INFERENCE, D3]`.
- **Metadata can lie:** `ollama-rs`'s `tool-implementations` and `piper-rs` pass cargo-deny while carrying GPL code (research-ai-chat §4.6, via G3 §7).
- **P14's answer for ported code:** `src-tauri/crates/keeper-ported/` is pure (no keeper dependencies, no network, no tauri), one module per upstream, each with an `UPSTREAM.md` naming the repository, commit, licence, and what was ported or changed. The upstreams and their licences, as the digests recorded them: Hermes Agent MIT (R6), OpenClaw MIT by its README and "Other" by the GitHub API (R1), agentskills.io's `skills-ref` Apache-2.0 (R6), OKF Apache-2.0 (G3 §8), Smart Turn v3 BSD-2-Clause (R5), NanoKVM GPL-3.0 and NanoKVM-Go GPL-3.0 (R3), BMAD-METHOD `[UNVERIFIED]` (G4 §7).

> Coordinator note: two of P14's upstreams are copyleft or unknown. `nanokvm` is limited by P14 to
> "protocol encoding only", and the classic NanoKVM's HID encoding was read from a community API
> reference (`scgreenhalgh/nanokvm-mcp`, MIT; R3 §3), not from GPL source; the module's
> `UPSTREAM.md` should name that reference as its source so no GPL code is copied. `bmad`'s
> upstream licence was not established locally (G4 §7); `UPSTREAM.md` for `bmad` cannot be written
> until it is. OpenClaw's licence is reported two ways (R1). All three are in §14.

---

## 6. Transport: Matrix, or Matrix and a hub

§6.1–§6.6 are R7's reading, `[SOURCE]` unless marked. §6.7–§6.8 record the coordinator's choice and what it gives up.

### 6.1 matrix-rust-sdk

| Topic | Finding |
| --- | --- |
| Version and licence | newest **0.19.1 (2026-09-18)** (0.19.0 on 2026-09-16); Apache-2.0; Element-funded, "production ready" ([CHANGELOG](https://github.com/matrix-org/matrix-rust-sdk/blob/main/crates/matrix-sdk/CHANGELOG.md), [README](https://github.com/matrix-org/matrix-rust-sdk/blob/main/README.md)). keeper pins **0.18** (`src-tauri/Cargo.toml:61`), and ruling R11 keeps it there for this program. |
| Mobile | Element X Android uses the SDK "through an FFI layer"; a `uniffi` feature; Element X iOS and Android are **AGPL-3.0**, reference apps only ([element-x-android](https://github.com/element-hq/element-x-android), [features](https://docs.rs/crate/matrix-sdk/latest/features)) |
| Custom events | `Room::send_raw`, `Room::send_state_event`, `Room::send_state_event_raw`, `Room::typing_notice`, `Client::send`, a send queue ([Room](https://docs.rs/matrix-sdk/0.19.1/matrix_sdk/room/struct.Room.html)); D1 verified `send_raw`, `send_state_event_raw`, `get_state_event_static` and `add_event_handler` in 0.18's source (§12.3) |
| Addressed to-device | `Encryption::encrypt_and_send_raw_to_device(recipient_devices, event_type, …)` behind `experimental-send-custom-to-device` ([source](https://github.com/matrix-org/matrix-rust-sdk/blob/main/crates/matrix-sdk/src/encryption/mod.rs)) |
| To-device semantics | delivered "exactly once to each client device", stored until the device syncs, in arrival order, about 100 per `/sync`, "not intended for conversational data" ([spec](https://github.com/matrix-org/matrix-spec/blob/main/content/client-server-api/modules/send_to_device.md)) |
| Ephemeral events | only typing and receipts; MSC2477 (custom ephemeral) is not implemented in tuwunel ([MSC table](https://matrix-construct.github.io/tuwunel/development/compliance/msc.html)); Synapse has no MSC2477 flag ([experimental.py](https://github.com/element-hq/synapse/blob/develop/synapse/config/experimental.py)) |
| Sync | tuwunel serves Simplified Sliding Sync (MSC4186, "sync v5 served"); 0.19 adds `SlidingSync::set_room_subscriptions` |
| E2EE for bots | on by default (`e2e-encryption`, `automatic-room-key-forwarding`, `sqlite`); 0.19 adds a manager for MSC3814 dehydrated devices; encrypted state events are experimental |
| Devices | each login is its own device with its own crypto store `[INFERENCE, R7]`: one copy = one device (P5) |
| Widgets, MatrixRTC | widgets behind `experimental-widgets`; `Client::rtc_transports` (MSC4143/4515), `enable_automatic_call_status` (MSC4426); tuwunel lacks MSC4140 delayed events and MSC4354 sticky events; Synapse has `msc4354_enabled` |

### 6.2 Streaming tokens over Matrix

- **An edit is a full durable room event.** Synapse's default `rc_message` is **0.2 per second with a burst of 10**, with per-user overrides through the admin API ([Synapse config](https://github.com/element-hq/synapse/blob/develop/docs/usage/configuration/config_documentation.md)). Appservices are exempt when registered with `rate_limited: false` ([registration schema](https://github.com/matrix-org/matrix-spec/blob/main/data/api/application-service/definitions/registration.yaml)); Synapse defaults it to `True`.
- **tuwunel's example config** rate-limits only login (`[global.rate_limiting.login]`) ([tuwunel-example.toml](https://github.com/matrix-construct/tuwunel/blob/main/tuwunel-example.toml)). What electra's deployed config sets was not read `[UNVERIFIED]` (§14).
- **Events are capped at 64 KiB.** Beeper uploads oversized final payloads as attachments ([ai-bridge](https://github.com/beeper/ai-bridge)).
- **MSC4471 "Event streams"** (opened 2026-05-14, labelled needs-implementation) targets exactly this: a durable message carries an `m.stream` descriptor naming the publisher's device; viewers send `m.stream.subscribe` as a **to-device** event; the publisher replies with `m.stream.update` to-device events (append or replace, sequenced); a final edit removes the descriptor; "No homeserver work is required." The author targets 5 updates per second; Matthew Hodgson (ara4n) "literally needed this" for transcribing MatrixRTC calls ([MSC4471](https://github.com/matrix-org/matrix-spec-proposals/pull/4471)). The matrix-rust-sdk implementation PR #6607 was **closed unmerged as stale** on 2026-08-13 ([PR](https://github.com/matrix-org/matrix-rust-sdk/pull/6607)).
- **A bot status EDU:** tuwunel issue #531 (2026-07-30) asks for MSC2477 so multi-agent bots can show "thinking / running a tool / waiting for approval"; still open ([issue](https://github.com/matrix-construct/tuwunel/issues/531)).
- **Measured latency:** none published. tuwunel #358 reported 20 s to 2 min sliding-sync delays; the cause was the reporter's ISP ([issue](https://github.com/matrix-construct/tuwunel/issues/358)).
- **What P4's cadence costs** `[INFERENCE]` over these numbers: edits at least 400 ms apart are at most 2.5 a second per stream. On a homeserver with Synapse's defaults, the burst of 10 is spent in about 4 s and the stream then gets one edit every 5 s. The smoke homeserver of ruling R13, `keeper-test-synapse`, is a Synapse, so agent users there need a per-user override or an appservice registration before a streaming smoke test means anything (§13 #20). Every edit is also a stored event: a two-minute answer at full cadence is about 300 events in the room's history.

### 6.3 Prior art on Matrix

| Project | Language / licence | What is relevant |
| --- | --- | --- |
| baibot (etke.cc) | Rust, **AGPL-3.0**; rust-mxlink LGPL-3.0 | an E2EE LLM bot ([repo](https://github.com/etkecc/baibot)) |
| Hermes Agent | Python, MIT; `mautrix` | each DM, thread and user in a shared room gets its own session; thinking and tool panes threaded and edited in place; E2EE off, optional or required; approvals by reaction ([docs](https://hermes-agent.nousresearch.com/docs/user-guide/messaging/matrix)) |
| OpenClaw Matrix plugin | TypeScript, MIT; `matrix-js-sdk` | streaming previews by editing one message; `quiet` mode plus push rules notify only on the final edit; edits "cost extra Matrix API calls"; bot-to-bot opt-in via `allowBots`; `/acp spawn … --bind here` binds a room to an ACP session ([docs](https://docs.openclaw.ai/channels/matrix/messaging.md)) |
| Beeper ai-bridge | Go, mautrix bridgev2; no licence file (`[INFERENCE, R7]` all rights reserved) | a `com.beeper.ai` envelope carries **AG-UI events** in an anchor → stream (`m.reference`) → final-edit lifecycle, sequenced, idempotent transaction ids; human-in-the-loop approvals; runs resume after a restart |
| Beeper pickle | TypeScript; "Other" | "Debounced edits everywhere; native [streaming] on Beeper" |
| maubot / matrix-nio | AGPL-3.0 / ISC | general frameworks |
| Agent MSCs | — | MSC4332 / MSC4391 (in-room bot commands), MSC4295 (bot bounce limit, loop prevention), MSC4333 (moderation bots) |

### 6.4 Push to phones

- **Sygnal:** matrix-org's repository (Apache-2.0) is **archived**; development continues at element-hq/sygnal, **AGPL-3.0 or commercial**; APNs and FCM ([README](https://github.com/element-hq/sygnal)). APNs credentials belong to one app bundle, so keeper needs its own gateway `[INFERENCE, R7]`; running AGPL Sygnal as a separate service is within policy.
- **Android without Google:** ntfy implements the Matrix push gateway (`/_matrix/push/v1/notify`) with UnifiedPush ([server_matrix.go](https://github.com/binwiederhier/ntfy/blob/main/server/server_matrix.go)), Apache-2.0 or GPLv2; Element X Android ships a UnifiedPush module.
- **What wakes a phone:** push rules run on room events, so to-device messages do not wake a phone `[INFERENCE, R7]`. That is one more reason P4's approval requests are room events.
- **Push latency:** none published.

### 6.5 Calling a tool on the person's device

| Protocol | Can a server agent call a tool on the device and get a result? |
| --- | --- |
| ACP v1 | `fs/*` and `terminal/*` are agent-to-client requests, but the only transport is stdio; a remote HTTP/WebSocket transport is a draft RFD that does not replay in-flight messages ([RFD](https://agentclientprotocol.com/rfds/streamable-http-websocket-transport)) |
| ACP v2 (draft) | **removes** client `fs`/`terminal`: "use client-provided MCP servers" ([migration](https://agentclientprotocol.com/protocol/v2/migration)); the draft MCP-over-ACP RFD lets the client offer tools over its ACP connection ([RFD](https://agentclientprotocol.com/rfds/mcp-over-acp.md)) |
| AG-UI frontend tools | yes: `RunAgentInput.tools`; the agent streams `TOOL_CALL_START/ARGS/END`; the front end runs the tool and returns a `role:"tool"` message; MIT ([docs](https://docs.ag-ui.com/concepts/tools)) |
| MCP 2026-07-28 | no: server-initiated requests are **removed** for MRTR (`input_required`); Roots and Sampling deprecated; sessions and SSE resumption gone ([changelog](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/changelog.mdx)); MCP Apps (2026-01-26, stable) renders UI inside the host |
| OpenAI Realtime ("GPT-Live") | function calls arrive at whichever side holds the session; a server can attach through a sideband WebSocket ([guide](https://developers.openai.com/api/docs/guides/realtime-server-controls.md)) |

- **None of them decides which device is the person's current one** `[INFERENCE, R7]`. R7 proposed the AG-UI frontend-tool shape over a hub; P4 carries the same request/result pair as Matrix custom events (P4's "surface request/result" in the `dev.keeper.agent.*` family; the exact event types are the architecture's to name), which is how story 91.3's surface tools (open at heading, highlight, point, propose edit, scroll) reach the notes view.

### 6.6 R7's verdict, and its recommended hybrid `[INFERENCE, R7]`

| Need | Matrix | A custom WS/SSE hub | Hybrid |
| --- | --- | --- | --- |
| Durable addressed messages | **strong**: room DAG, per-device to-device | must build a log | Matrix |
| Token streaming | **weak**: edits rate-limited and durable; MSC4471 unmerged | **strong** | hub |
| Voice media | MatrixRTC needs LiveKit and MSC4140/4354; Element Call is AGPL | **strong** (WebRTC/Opus) | hub / WebRTC |
| Routing to one host | **good**: to-device to (user, device) | good, with a host registry | Matrix to-device for control; hub when connected |
| Offline queueing | **strong** | must build | Matrix |
| Push | **strong** (pushers + Sygnal / ntfy) | must build per platform | Matrix |
| E2EE | **strong** (Olm/Megolm, cross-signing, dehydrated devices) | TLS only unless built | Matrix for content |
| Several people | **strong** (rooms, power levels, membership) | must build ACLs | Matrix |
| Bridges | **strong** (appservices; MSC4190/4326 in tuwunel) | none | Matrix |
| Latency | long-poll sync; no published figures | lowest | hub on the hot path |

R7's hybrid: a Matrix plane for bytes that must survive or reach an offline device (prompts and final answers, session open and hand-off, approvals and decisions, attachments ≤ 64 KiB, the push trigger, bridged traffic); a hub plane for high-rate, ephemeral traffic (token and reasoning deltas, tool progress, status in the AG-UI vocabulary, front-end tool calls routed to the device with the newest heartbeat, live audio), every hub stream anchored to a Matrix event with exactly one final edit; a git plane for session folders. Private and shared principals get separate rooms and hub scopes; the hub checks room membership before opening a stream.

### 6.7 Why the program chose Matrix only (P4)

P4 pins: one Matrix room per session (an agent's main session is the DM with its human); token streaming as debounced `m.replace` edits at least 400 ms apart plus one final edit; custom events `dev.keeper.agent.*` (status, approval request and decision, surface request and result, doorbell) and state events (the claim per session room; host manifest and presence per principal control room); the phone and tablet never write session logs, they send Matrix events and the owning host writes the log. Ruling R11 adds: no to-device dependency.

**The reasons, each with its evidence:**
1. **The private voice option sends only text** (P13, §8.1). R7's hub existed largely for live audio and for token deltas; with no audio on the wire, the hub's strongest row is gone.
2. **Matrix already gives the hard half** of R7's table: addressed, queued, E2EE, pushable delivery among several people (§6.6).
3. **A hub would be the first listening socket keeper ships.** DW-215 holds keeper-as-a-server to its own threat model, "a listening socket is not an outbound request" (`docs/decisions.md:146-150`); the scope guards refuse it for this program.
4. **The phone cannot merge** ("Nothing is merged on a phone … the Mac merges", `docs/ios.md:737`), so a phone that wrote session files would create the conflicts P3's one-writer rule exists to prevent.
5. **tuwunel's example config limits only login** (§6.2), so the edit cadence is a homeserver-configuration question, not a protocol limit, on the owner's server.

**What it gives up** `[INFERENCE]`:
- **Smooth token streaming.** Edits every 400 ms are coarser than a socket's deltas, are durable, and on a Synapse with defaults are throttled to one per 5 s after the burst (§6.2).
- **To-device routing.** R7 recommends to-device for "you own session X" and wake-ups. Ruling R11 replaces it with room events and state events: the claim is a state event in the session room, and the host manifest is a state event in the principal's control room (P5, P6). A to-device message also would not wake a phone (§6.4).
- **Status without history.** No custom ephemeral events exist on tuwunel (§6.1), so `dev.keeper.agent.status` is a stored event.
- **Long final answers.** The 64 KiB event cap means a long answer cannot be one final edit; Beeper's answer is an attachment. P4 does not pin the overflow form (§13 #21). **Settled by ruling R23:** the first 60 KiB plus a link to `artifacts/answer-<ulid>.md`, the whole text in the log and that artifact; in an encrypted room the cut is lowered until the event fits, measured on the Synapse test homeserver in 90.5 (ruling R27; AD-373).

> Coordinator note: P4's justification line says "tuwunel rate-limits only login (R7 §2)". R7 read
> tuwunel's *example* configuration, not electra's deployed one, so that clause is `[UNVERIFIED]`
> for the owner's server (§14). And ruling R13's Matrix smoke homeserver is a Synapse, whose default
> `rc_message` (0.2/s, burst 10) throttles P4's cadence after about four seconds; the smoke tests
> need a per-user rate-limit override for agent users or an appservice registration with
> `rate_limited: false` (R7 §2). Neither changes P4; both change what a passing smoke test proves.

### 6.8 The revisit trigger

P4's trigger, verbatim: **"measured p95 Matrix delivery > 1 s on tuwunel, or server-side voice."**

- **Nobody has measured it.** R7 found no published Matrix delivery latency for a self-hosted homeserver (§6.2, §14). The first measurement is the program's own, in story 90.5's smoke run (§13 #22).
- **Server-side voice** is refused by P13 and by D-5 ("a server fallback is not a revisit, it is a new row in `docs/egress.md` that this decision refuses to write", `docs/decisions.md:248-249`), so only the latency half can fire within this program.
- **If it fires,** R7's hybrid (§6.6) is the documented alternative, with DW-215's threat model written first.

---

## 7. Computer use, KVMs and approvals

§7.1–§7.5 and §7.8 are R3's reading, `[SOURCE]` unless marked. §7.6 and §7.7 are R3's recommendations `[INFERENCE, R3]` as P9 pins them. §7.9 is keeper's side.

### 7.1 Computer-use model APIs (status 2026-10-01)

| API | What the host supplies | Coordinates | Built-in safety |
| --- | --- | --- | --- |
| **Anthropic** `computer_toolset_20260801`, GA, 17 member tools including `zoom` and `hold_key` ([docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool)) | screenshots you resize yourself; too-large images are rejected; Opus 4.7+ takes up to 2576 px on the long edge or 4784 visual tokens; 1024×768 or 1280×720 recommended | the screenshot's pixels, scaled back (÷2 on a Retina Mac) | classifiers scan screenshots for injection; a reply can batch tool calls, so "confirm before each block runs" |
| **OpenAI** GA `computer` tool, a batched `actions[]`; for GPT-6 Astra OpenAI recommends code execution instead (PyAutoGUI or Playwright) ([guide](https://developers.openai.com/api/docs/guides/tools-computer-use)) | screenshots or a persistent runtime | screenshot pixels | levels left to the host: "Hand-off required" (the final step of a password change, bypassing HTTPS warnings); "Always confirm" (deletes, permission changes, CAPTCHAs, running downloaded code, send/post, payments, OS security settings); "Pre-approval can be enough" (logins, uploads, move/rename); "Typing sensitive data… counts as transmission"; in a batch, stop before the first action that needs confirmation ([recipes](https://developers.openai.com/api/docs/guides/tools-computer-use-integration.md)) |
| **Gemini** `computer_use` with `browser`, `mobile` or `desktop`; each action carries an `intent` ([docs](https://ai.google.dev/gemini-api/docs/computer-use)) | screenshots | normalised 0–999 | `safety_decision: require_confirmation` answered with `safety_acknowledgement`; categories `FINANCIAL_TRANSACTIONS`, `SENSITIVE_DATA_MODIFICATION`, `COMMUNICATION_TOOL`, `ACCOUNT_CREATION`, `DATA_MODIFICATION`, `USER_CONSENT_MANAGEMENT`, `LEGAL_TERMS_AND_AGREEMENTS`; injection detection opt-in |
| **UI-TARS** (ByteDance), Apache-2.0, open 1.5-7B weights, UI-TARS-2 report Sep 2025 ([repo](https://github.com/bytedance/UI-TARS)) | screenshots only | absolute on the Qwen2.5-VL grid, via the `ui-tars` parser | its own limitations admit CAPTCHA misuse and hallucinated elements |

- **Accuracy:** the best OSWorld-Verified score is 85% against about 72% for humans, so 15% of tasks still fail; agents take 8–10 minutes for a 2–3 minute task; grounding on the accessibility tree cuts latency ([a16z, Aug 2026](https://a16z.com/can-agents-use-a-computer-yet-weve-got-the-data/)).

### 7.2 Host automation

**macOS**
- **TCC:** four services gate desktop access — Accessibility, ScreenCapture, PostEvent (input injection), ListenEvent (input monitoring) ([HackTricks](https://hacktricks.wiki/en/macos-hardening/macos-security-and-privilege-escalation/macos-security-protections/macos-input-monitoring-screen-capture-accessibility.html)).
- **Peekaboo** now lives at **openclaw/Peekaboo** (MIT, macOS 15+) ([repo](https://github.com/steipete/Peekaboo)): needs Screen Recording and Accessibility; "Event Synthesizing" is optional for background typing; background clicks go through Accessibility; **under SSH or a LaunchAgent a "Bridge" GUI host is needed**, without which CoreGraphics may return only the wallpaper; **re-signing the binary resets its TCC grants** ([permissions.md](https://raw.githubusercontent.com/openclaw/Peekaboo/main/docs/permissions.md)).
- **Synthetic clicks on security dialogs:** MindStudio says macOS has discarded them since Mojave ([article](https://www.mindstudio.ai/blog/violoop-ai-hardware-agent-mac)); HackTricks describes PostEvent clicking "Allow". **Contested** `[UNVERIFIED]`.
- **App Intents and MCP:** 9to5Mac found MCP groundwork in the App Intents code of the 26.1 betas ([2025](https://9to5mac.com/2025/09/22/macos-tahoe-26-1-beta-1-mcp-integration/)); a blog's claim that WWDC26 made MCP system-wide cites session 339, which covers only `LanguageModelExecutor` ([video](https://developer.apple.com/videos/play/wwdc2026/339/)) — `[UNVERIFIED]`.

**Linux**
- **The RemoteDesktop portal v2:** `CreateSession` → `SelectDevices` → `Start` (a user dialog); `ConnectToEIS` (libei) is the input path; ScreenCast attaches through PipeWire; `persist_mode=2` and a single-use `restore_token` allow unattended reconnects ([spec](https://raw.githubusercontent.com/flatpak/xdg-desktop-portal/main/data/org.freedesktop.portal.RemoteDesktop.xml)).
- **Crates:** `ashpd` 0.13.13 for portals and `atspi` 0.30.0 for AT-SPI2, both on zbus ([crates.io](https://crates.io/crates/atspi)).
- **ydotool** writes `/dev/uinput` (usually root) and is AGPL-3.0, so only callable as an external CLI ([repo](https://github.com/ReimuNotMoe/ydotool)).

**iOS:** third-party apps are sandboxed and cannot modify other apps ([Apple Platform Security](https://support.apple.com/guide/security/security-of-runtime-process-sec15bfe098e/web)). The iPhone can be an approval device and cannot be automated in process; only external hardware drives it (§7.3).

### 7.3 IP-KVMs as connectors

**NanoKVM-Go / Go+** (Sipeed, Kickstarter July 2026; [wiki](https://wiki.sipeed.com/hardware/en/kvm/NanoKVM_Go/introduction.html))
- Dual Cortex-A53; Go+ adds a 3.2 TOPS NPU; 256 MB/16 GB (Go), 512 MB/64 GB (Go+); one USB-C port with DP Alt Mode in; Wi-Fi 6.
- Capture up to 4K50 or 2K90, about 60 ms at 1080p60. Targets include the iPhone 15+ (with AssistiveTouch) and Macs.
- **A built-in MCP server** at `https://<ip>/api/mcp` with `Authorization: Bearer <API key>`, on a **self-signed certificate**; the docs tell OpenCode users to set `NODE_TLS_REJECT_UNAUTHORIZED=0` ([MCP guide](https://wiki.sipeed.com/hardware/en/kvm/NanoKVM_Go/mcp.html)).
- Go+ only: "Memory Fabric", text extracted from changing screens.
- Early bird $59/$79, MSRP $89/$129; CNX thinks the SoC is an AX630C ([CNX](https://www.cnx-software.com/2026/07/01/sipeed-nanokvm-go-an-4k-usb-c-kvm-with-recall-like-function-ai-integration/)).
- **`sipeed/NanoKVM-Go` is GPL-3.0 but contains only a LICENSE file** ([repo](https://github.com/sipeed/NanoKVM-Go)): no firmware, no schema.

**Classic NanoKVM** (GPL-3.0, Go server; [repo](https://github.com/sipeed/NanoKVM))
- HID over the WebSocket `/api/ws` as JSON arrays: `[1,keycode,mods…]` for keys, `[2,event,button,x,y]` for the mouse with absolute coordinates 1–32767; text via `/api/hid/paste`; video via `/api/stream/mjpeg` ([community API reference](https://raw.githubusercontent.com/scgreenhalgh/nanokvm-mcp/main/API_REFERENCE.md)).
- That reference says the login password is "encrypted" with a hard-coded AES key — obfuscation, not protection `[INFERENCE, R3]`.
- The firmware ships an on-device PicoClaw agent with `kvm_screenshot`/`kvm_actions` MCP tools and coordinates normalised to [0,1]; its skill warns that success "only means the HID event was sent" ([SKILL.md](https://raw.githubusercontent.com/sipeed/NanoKVM/main/kvmapp/picoclaw/skills/kvm-control/SKILL.md)).

**PiKVM** ([API](https://docs.pikvm.org/api/)): REST `/api/hid/print`, `/api/hid/events/send_key|send_shortcut|send_mouse_move` (origin at the screen's centre); `/api/streamer/snapshot?ocr=true` returns a screenshot or Tesseract text; events over `/api/ws?stream=0`; auth `X-KVMD-User`/`X-KVMD-Passwd` with a TOTP code appended.

**Others:** JetKVM is GPL-2.0 and uses JSON-RPC 2.0 (`jsonrpc.go`) ([repo](https://github.com/jetkvm/kvm)), over WebRTC `[UNVERIFIED]`; GL.iNet Comet is a PiKVM derivative ([glkvm](https://github.com/gl-inet/glkvm)), so probably PiKVM-compatible `[INFERENCE, R3]`.

**Security:** Eclypsium found 9 CVEs across JetKVM, NanoKVM, Comet and Angeet — Comet firmware verified only by an MD5 embedded in the file; NanoKVM CVE-2026-32296, fixed in 2.3.1; 1 611 devices exposed to the internet in January 2026 ([Eclypsium](https://eclypsium.com/blog/your-kvm-is-the-weak-link-how-30-dollar-devices-can-own-your-entire-network/)).

**LLM-to-KVM projects:** `scgreenhalgh/nanokvm-mcp` (MIT), NanoKVM PicoClaw, the NanoKVM-Go MCP server.

### 7.4 Violoop: prepare, then commit

- BVIO Technology Ltd (Hong Kong); Kickstarter $399, retail $699, first shipments November 2026 ([Q&A](https://violoop.ai/qa/)). It reads the screen over HDMI or USB-C and acts as a USB keyboard and mouse; a companion app adds file, script and accessibility actions "under the same approval gate".
- **It "separates preparing from committing":** it does the work, then pauses before sending, submitting, overwriting, paying or deleting.
- **A separate security chip decides go or no-go,** accepting only the device's physical button or a phone confirmation: the chip issues a challenge, the phone signs it with a key that never leaves the phone, the chip verifies.
- The chip firmware is promised before shipping; an on-device action log exists.
- **A classification hazard:** a reviewer reports that "reversible" actions such as clicking "Allow" on a permission dialog run *without* approval ([MindStudio](https://www.mindstudio.ai/blog/violoop-ai-hardware-agent-mac)). §7.6 puts TCC dialogs at T4 for that reason.

### 7.5 How frameworks store and resume a pending action; the research on why it matters

| Framework | Pending action stored as | Resumed by | Danger classified by | Audit / guards |
| --- | --- | --- | --- | --- |
| LangGraph ([docs](https://docs.langchain.com/oss/python/langgraph/interrupts)) | `interrupt(json)` + checkpointer + `thread_id`; optional `response_schema` (≥ 1.2.12) | `Command(resume=…)`; several interrupts mapped by id; **the interrupted node re-runs from its start** | app code | checkpoint history |
| OpenAI Agents SDK ([HITL](https://openai.github.io/openai-agents-python/human_in_the_loop/)) | `RunState.to_json()` | `from_json` → `approve`/`reject` → `Runner.run` | `needs_approval` bool or function; the function fails closed on malformed arguments | snapshots are **not authenticated**: keep them server-side, authenticate the reviewer, consume each request atomically against replay, store a version marker |
| Codex ([docs](https://learn.chatgpt.com/docs/agent-approvals-security)) | session | interactive | sandbox read-only / workspace-write / danger-full-access; approval `on-request`, `never`, `granular` (`untrusted` retired); `auto_review`; destructive-annotated tools always ask | an async safety monitor can pause a task |
| Claude Code ([modes](https://code.claude.com/docs/en/permission-modes), [hooks](https://code.claude.com/docs/en/hooks.md)) | a `PreToolUse` hook returning `"defer"` exits with `tool_deferred` and a `deferred_tool_use {id,name,input}` record | `claude -p --resume`, no timeout; only when one tool call was made that turn | default / acceptEdits / plan / auto (classifier) / dontAsk / bypassPermissions; deny rules in every mode; deny > defer > ask > allow | users approved about 93% of prompts; the sandbox cut prompts 84%; auto mode lets about 17% of overeager actions through ([Anthropic](https://www.anthropic.com/engineering/how-we-contain-claude)) |
| MCP elicitation (2026-07-28) ([Nango](https://nango.dev/blog/mcp-elicitation-explained)) | `input_required` + an opaque `requestState` | the client retries with `inputResponses`, possibly from another process later | server-side | form mode must never collect secrets; URL mode for those |
| OpenClaw ([docs](https://docs.openclaw.ai/tools/exec-approvals)) | SQLite on the executing machine; an approval id; expires after 30 min ([advanced](https://docs.openclaw.ai/tools/exec-approvals-advanced)) | `/approve <id> allow-once\|allow-always\|deny`; a late approval cannot restart a closed turn | deny / allowlist / ask / auto / full; `askFallback` deny; `strictInlineEval` for `python -c` | bound to argv, cwd, env, executable path and, for writable files, a content hash; drift ⇒ deny; a dedicated `operator.approvals` scope |
| Hermes ([docs](https://hermes-agent.nousresearch.com/docs/user-guide/security)) | in memory, 300 s, fail-closed | once / session / always / deny | regex danger patterns; a hardline blocklist; deny globs; `smart` uses an auxiliary LLM; `cron_mode` and `unattended_mode` deny | `approvals suggest` never proposes allowlisting destructive classes |

**Safety research and incidents**
- **OS-Harm** (150 tasks): every model tested complied with much deliberate misuse and fell to static prompt injections ([arXiv](https://arxiv.org/abs/2506.14866)).
- **OS-Blind** (2026; 300 tasks, every instruction benign) ([site](https://limelab.science/OS_Blind/)): most agents over 90% attack success; Claude 4.5 Sonnet went from 73% alone to **92.7% inside a multi-agent system**; refusals happen almost only in the first steps; **splitting a task into subtasks hides the harmful intent**. R3: "This applies directly to your master-dispatches-to-personas design." It is the evidence behind P7's hop limit and P9's +1 tier for delegated work.
- **Anthropic's containment post** ([link](https://www.anthropic.com/engineering/how-we-contain-claude)): a phished prompt exfiltrated credentials in 24 of 25 runs; data left through an allowlisted domain (api.anthropic.com); flagged risks: persistent memory poisoning and multi-agent trust escalation.
- **PocketOS, 25 April 2026:** a Cursor agent on Claude Opus 4.6 deleted a production volume in 9 seconds with an over-scoped Railway token it found in an unrelated file; no prompt injection ([Zenity](https://zenity.io/blog/ai-agent-database-deletion-pocketos)).

### 7.6 The tier taxonomy `[INFERENCE, R3]`, pinned by P9

The tiers follow OpenAI's confirmation levels, Gemini's policy categories and Hermes' hardline floor.

| Tier | Examples | Gate |
| --- | --- | --- |
| **T0 Observe** | screenshot, accessibility-tree read, reading files under granted folders, KVM snapshot or OCR | auto, logged; output marked as possibly containing secrets |
| **T1 Reversible-local** | navigate UI, open an app, scroll, type into an unsubmitted form, write in agent scratch space or git-tracked files | auto within the agent's grants |
| **T2 Recoverable mutation** | move/rename, edit user files with a snapshot first, create drafts, install packages inside a sandbox | policy or an auto-reviewer; may be approved for the whole session |
| **T3 External / transmit** | send or post, upload, type sensitive data, change sharing, accept terms, contact a new domain, MCP tools marked destructive | human approval per action, showing the exact payload |
| **T4 Irreversible / privileged** | hard delete, `rm -r`, dropping a database, payments, credentials, OS security settings or TCC dialogs, `sudo`, running downloaded code, KVM power/ISO/BIOS | human approval from a device; never "allow-always" |
| **T5 Forbidden / hand-off** | the final step of a password change, bypassing HTTPS warnings or CAPTCHAs, disk wipes, disabling the approval system, editing its own grants | deny, and hand the step to the human |

**Raise by one tier** (R3): the action came from untrusted content; the run is unattended or scheduled; the action was dispatched to a sub-agent; the target is reached through a KVM.
**Raise by one tier** (P9 as pinned): delegated, unattended, or after untrusted input. P9 does not repeat R3's fourth condition (a KVM target); story 96.5 decides whether a KVM action is raised or simply starts at T4 for its power, ISO and BIOS verbs (§13 #32). **Settled by ruling R22:** a target reached through a KVM is the fourth raise condition; the raise is applied once, and actions that exist only through a KVM carry it in their base tier (AD-392, AD-409).

**As the reviews left it** (ruling R28): the `session` scope at T2 is never offered in a `main` session and lasts at most 24 h elsewhere (S-11); a T4 action is never approved from a notification, never from the owning host's own process, and only by the requester (S-10, S-22, S-28); an MCP server's annotations lower a tier only when a person trusts them (S-14); a `schedule:` or `workflow:` an agent writes is T3 and waits for a person's tick (S-21).

### 7.7 The pending-approval record

**R3's shape** (`approvals/<ULID>.json`) `[INFERENCE, R3]`, built from the Agents SDK, OpenClaw and Claude Code guidance. The request file is never edited after creation.

- **Identity:** `schema_version`, `id` (ULID), `created_at`, `expires_at`, `agent_def_version`.
- **Origin:** `device_id`, `persona_id`, `session_id`, `run_id`, `dispatch_chain[]` (master → persona → sub-agent), `checkpoint_ref` + `checkpoint_sha256`.
- **Action:** `tool`, `target` {host or KVM id, app, window id}, `args` (canonical JSON), `exec_binding` {argv, cwd, env subset, exe path + hash, operand file hashes}, `human_summary`, `preview` {screenshot path + hash, AX element id}.
- **Risk:** `tier`, `categories[]`, `reversible`, `classifier_verdicts[]`, `provenance_taint[]`, `matched_rules[]`.
- **Preconditions:** `expected_screen_hash` or `ax_path`, `max_staleness_s`.
- **Binding:** `binding_digest` = SHA-256 of (args + exec_binding + checkpoint hash + preconditions), plus `request_sig` from the originating device's key.
- **Scopes:** `once` or `session`; never `always` at T4 and above.
- **Decision file** (`<ULID>.decision.<device>.json` in R3, so sync never merges one file): `decision` (approve / deny / edit), `scope`, `edited_args` (forcing a new digest), `decided_by` {user, device}, `decided_at`, `nonce`, a signature over `binding_digest` + `nonce`.
- **Consumption:** a `lease` file {runner_device, fencing_token, acquired_at}, then `consumed_at`, `result`, `side_effects`, `prev_audit_hash` chaining the audit log.
- **Resume rules:** re-check the digest and preconditions and deny on drift; consume exactly once; executors must be idempotent, because LangGraph-style resumes re-run the interrupted step.

**What P9 and ruling R12 keep and change:**

| R3 | As pinned | Why `[INFERENCE]` |
| --- | --- | --- |
| request file immutable, digest-bound | kept: `approvals/<ulid>.json` with tool, canonical args, exec binding, checkpoint hash, preconditions | — |
| decision files per device, `<ULID>.decision.<device>.json` | one `<ulid>.decision.json`, written by the **owning host** on receipt of the human's Matrix decision | the phone never writes session files (P4, `docs/ios.md:737`); one writer per file (ruling R3) |
| `request_sig` and a decision signature from device keys | the decision is a Matrix event from a **verified device** of a human in the session label's readers, checked through matrix-sdk (ruling R12) | Matrix cross-signing already is the per-device key; the model holds no Matrix key (R1 pattern 10) |
| a `lease` file with a fencing token | the session's claim (P6) fences the log, and consumption is a `dev.keeper.agent.approval.consumed` event the homeserver accepts before the effect (ruling R28 S-01); the claim epoch is not a precondition (ruling R25) | claims fence every log line; only the homeserver is a shared, ordered store that a crashed host has written before it acted |
| `once` / `session`, never `always` at T4+ | kept: "Never 'always' at T4+"; `session` never in a `main` session, at most 24 h elsewhere (ruling R28 S-11) | — |
| consume once; re-check digest and preconditions | kept | — |
| `binding_digest` over args, exec binding, checkpoint hash and preconditions | the same plus the record's `id`, `session` and `agent` (ruling R28 S-24), over canonical JSON with keys in UTF-16 order, RFC 8785 string escapes and integers only (ruling R25 as R29 F11 fixes it) | a decision cannot be paired with another record; two hosts compute one digest |
| `human_summary` | `summary`, composed by keeper from a per-tool template, never model text (ruling R28 S-10) | a summary the model writes is a surface it can shape |

### 7.8 Ranked host-automation approaches `[INFERENCE, R3]`

**macOS**

| Rank | Approach | For | Against |
| --- | --- | --- | --- |
| 1 | structured calls: CLI, files, `shortcuts run` / App Intents | deterministic, typed, auditable | only what apps expose |
| 2 | Accessibility API (AXUIElement), Peekaboo-style element ids | background, semantic, fast | the Accessibility grant; weak on canvas and web apps |
| 3 | ScreenCaptureKit + a vision model grounded on the AX tree | universal | Screen Recording grant; a possible monthly re-prompt; pixels to a model |
| 4 | CGEvent synthetic input | anything visible | focus, fragility, the PostEvent grant |
| 5 | an external KVM's HID (NanoKVM-Go, PiKVM) | below the OS: lock screen, BIOS, TCC dialogs | the most power and the weakest device security; behind T4 |
| 6 | a macOS guest VM | contains risky GUI work | not the user's own session |

**Linux** (the server is mostly headless)

| Rank | Approach | For | Against |
| --- | --- | --- | --- |
| 1 | CLI / D-Bus inside a bubblewrap sandbox | deterministic | not for GUI-only apps |
| 2 | AT-SPI via `atspi` | semantic, pure Rust | coverage varies |
| 3 | RemoteDesktop + ScreenCast portal with libei (`ashpd`) | compositor-approved; restore tokens for unattended reconnects | a consent dialog once; persistence bugs in some compositors |
| 4 | Xvfb + xdotool in a container | isolated | X11 only |
| 5 | ydotool / uinput | any compositor | root-equivalent, blind, AGPL |
| 6 | IP-KVM | out-of-band recovery | as on macOS |

- **What the program picks** (program map, epic 96): rank 1 through story 96.1's argv-bound `run`; rank 2–3 on the Mac through Peekaboo's MCP server on hesperia (story 96.4), run inside a GUI session because of the Bridge requirement (§7.2); rank 5 through `keeper-ported::nanokvm`'s protocol encoding (story 96.5).

### 7.9 keeper's approval path today, and the `run` tool

- **Today** (§2.3): the approver blocks the tool call and polls every 250 ms; the ask lives in memory; Stop or a closed pane refuses; an unattended run refuses every ask (`bots_drive_ipc.rs:162-215`; `bots_tools.rs:109-114`) `[REPO]`. The blocking wait uses `block_in_place`, which panics on a current-thread tokio runtime (`bots_drive_ipc.rs:196`; D1 §6), hence ruling R8's multi-threaded runtime for agentd.
- **The precedent for a durable pending state** is the Matrix drafts Approval Pane (`vm.rs:521-531`; G1 §7).
- **What P9 adds:** the pending action becomes a file and a Matrix event; the run parks; the decision arrives from any device; the owning host resumes it (stories 93.1–93.4).

> Coordinator note: story 96.1 ("a sandboxed `run` tool (landlock / sandbox-exec, argv-bound
> approval)") meets three recorded refusals. The PRD says "no tool executes a shell string;
> `docs/decisions.md` D-3 stands and the general exec kind remains Epic 60's, unbuilt" (DW-213,
> `prd.md:1151`; `epic-61-…:330-331`); AD-159 lists "a shell string executed" among what it
> prevents (`ARCHITECTURE-BOTS.md:202`); and Epic 60 is reserved and never built (C1 §1). The
> pinned program keeps the letter of all three — the tool takes an argv, never a shell string; it is
> an agent tool, not a `TaskKind` (ruling R9); and it is sandboxed and approved per argv (P9) — but
> it reverses DW-213's intent that keeper runs no commands for a model. That reversal needs its own
> D-entry, and DW-213 needs a resolution line when epic 96 lands. **Settled by ruling R15** (D-33),
> and the sandbox is landlock with a seccomp filter on Linux (ruling R24(5)); with network a run
> mounts only its workspace (ruling R28 S-03).

---

## 8. Voice

### 8.1 What the owner chose: the private option, D-5 kept

Round 3: "**Private option** (keeps D-5 - yes for the option". P13 pins it: **on-device only**; Silero VAD and Smart Turn v3 (ONNX) loaded from the owner's config repository `_models/` (the D-29 precedent; nothing bundled, nothing downloaded); a backchannel rule before barge-in stops speech; the assistant turn truncated at the played sentence, with `heard_until` logged. P4 adds that this option sends only text.

**What "private" means here, precisely** `[INFERENCE]` over D-5 and R5:
- **Speech becomes text on the device** through the system recogniser, as today (`requiresOnDeviceRecognition = true`, enforced by a source scan; §2.9). The text goes to the agent's Matrix room as an ordinary message (P4, story 91.4). No audio leaves the device.
- **It is not R5's "privacy-max".** R5's privacy-max streams Opus audio to a self-hosted LiveKit server and runs STT there (§8.9). That sends a voice to a server, which D-5 refuses ("send a voice anywhere"), and it is P4's revisit trigger ("server-side voice"). The chosen option is narrower than R5's.
- **The new models are turn-taking, not recognition.** Silero decides "is someone speaking", Smart Turn decides "has the person finished"; the words still come from Apple's recogniser.
- **D-5's "not ship a model of its own" survives** in D-29's sense: the weights come from the owner's config repository, as transcription's do, and keeper's bundle carries none (§1.6 item 7).

### 8.2 Hosted speech-to-speech APIs, for the record `[SOURCE]` (R5 §1)

| Service | Transports | Turn detection / barge-in | Tools during speech | Limits / price | Through an OpenAI-compatible proxy? |
| --- | --- | --- | --- | --- | --- |
| **OpenAI Realtime `gpt-realtime-2.1`** | WebRTC, WebSocket, SIP ([guide](https://developers.openai.com/api/docs/guides/realtime)) | `server_vad` or `semantic_vad` with `eagerness`; `interrupt_response` ([VAD](https://developers.openai.com/api/docs/guides/realtime-vad.md)); WebRTC/SIP cut unplayed audio server-side; over WebSocket the client sends `conversation.item.truncate{audio_end_ms}`, which cuts audio but does **not** produce an aligned truncated transcript ([conversations](https://developers.openai.com/api/docs/guides/realtime-conversations.md)) | async function calling; remote MCP ([OpenAI](https://openai.com/index/introducing-gpt-realtime/)) | 60-minute sessions; audio $32 / 1M tokens in, $64 out; 128k context ([model](https://developers.openai.com/api/docs/models/gpt-realtime-2.1)) | yes: LiteLLM `/realtime` ([LiteLLM](https://docs.litellm.ai/docs/realtime)) |
| **OpenAI GPT-Live 1** | WebRTC, WebSocket, SIP, a server-side sideband WebSocket ([GPT-Live](https://developers.openai.com/api/docs/guides/live.md)) | true full duplex; both transcripts carry `start_ms`/`end_ms` | **delegation**: in `client` mode the app receives `session.delegation.created`, runs any back end and answers with `session.commentary.append` or `session.thinking.append` ([delegation](https://developers.openai.com/api/docs/guides/live-delegation.md)) | $0.05/min, per second; at 90% of 128k it swaps in a new voice engine seeded with a summary; `store` false by default ([model](https://developers.openai.com/api/docs/models/gpt-live-1), [sessions](https://developers.openai.com/api/docs/guides/live-conversations.md)) | not established; it uses `v1/live/sessions`, not `v1/realtime` |
| **Gemini 3.8 Live** (Preview) | stateful WSS; WebRTC through LiveKit or Pipecat ([overview](https://ai.google.dev/gemini-api/docs/live-api.md)) | `automatic_activity_detection`; barge-in sets `interrupted=true` | `NON_BLOCKING` calls scheduled `SILENT`, `WHEN_IDLE` or `INTERRUPTED`; proactive audio and affective dialog in `v1beta` ([capabilities](https://ai.google.dev/gemini-api/docs/live-api/capabilities)) | 15-minute audio sessions without compression; ~10-minute connections; resumption handles valid 2 h ([sessions](https://ai.google.dev/gemini-api/docs/live-api/session-management)) | LiteLLM lists Gemini |
| **xAI `grok-voice-think-fast-2.0`** | WSS; `output_audio_buffer.*` on WebRTC/SIP only | `server_vad` only; `idle_timeout_ms` | functions, web/X search, MCP; reasoning effort `high` by default | opt-in resumption; history expires after 30 min idle ([xAI](https://docs.x.ai/developers/model-capabilities/audio/speech-to-speech)) | speaks the OpenAI Realtime protocol with documented differences |
| **Amazon Nova 2 Sonic** | Bedrock `InvokeModelWithBidirectionalStream` | `endpointingSensitivity` waits 1.5, 1.75 or 2.0 s ([turn-taking](https://docs.aws.amazon.com/nova/latest/nova2-userguide/sonic-turn-taking.html)); on barge-in the client flushes its own audio ([barge-in](https://docs.aws.amazon.com/nova/latest/nova2-userguide/sonic-barge-in.html)) | async tools ([async](https://docs.aws.amazon.com/nova/latest/nova2-userguide/sonic-async-tools.html)) | 8-minute connections ([Nova](https://nova.amazon.com/sonic)) | LiteLLM lists Bedrock |

- **Scores** (NYU RITS's summary of Artificial Analysis): GPT-Realtime about 96% on conversational dynamics, speech reasoning in the low 80s; Grok Voice 93% on reasoning ([RITS](https://rits.shanghai.nyu.edu/ai/nvidia-releases-nemotronlabs-voicechat-an-open-full-duplex-voice-model)).
- **All of these send a voice to a third party**, which D-5 refuses; they are recorded so the refusal has its alternatives beside it.

### 8.3 Open and self-hostable models `[SOURCE]` (R5 §2)

| Model | Type | Licence (code / weights) | Hardware and latency |
| --- | --- | --- | --- |
| Kyutai Moshi | full-duplex speech-to-speech | Python MIT, Rust Apache-2.0 / CC-BY-4.0 ([README](https://github.com/kyutai-labs/moshi)) | speech reasoning 4.3% (Big Bench Audio) |
| moshi-server | serves Kyutai STT and TTS | MIT/Apache; Rust/candle but **links `pyo3`/`numpy`** ([Cargo.toml](https://github.com/kyutai-labs/moshi/blob/main/rust/moshi-server/Cargo.toml)); TTS "uses our Python implementation under the hood" ([DSM README](https://github.com/kyutai-labs/delayed-streams-modeling)) | one L40S: 64 STT streams at 3× real time |
| Unmute | cascade: Kyutai STT → any LLM → Kyutai TTS | MIT ([repo](https://github.com/kyutai-labs/unmute)) | CUDA x86_64, 16 GB+ VRAM; TTS ~750 ms on one GPU, ~450 ms on three; no tool calling |
| Kyutai STT `stt-1b-en_fr` / `stt-2.6b-en` | streaming STT, semantic VAD | CC-BY-4.0 | 0.5 s / 2.5 s delay; MLX on iPhone 16 Pro |
| Kyutai TTS 1.6B / Pocket TTS (100M, 6 languages, CPU) | TTS | CC-BY-4.0 weights; Pocket TTS code MIT ([HF](https://huggingface.co/kyutai/tts-1.6b-en_fr), [blog](https://kyutai.org/blog)) | Pocket TTS on CPU |
| NVIDIA PersonaPlex 7B | full-duplex (Moshi-based) | MIT / NVIDIA Open Model Licence ([README](https://github.com/NVIDIA/personaplex)) | dynamics 91.0%, reasoning 19.1%; ~24 GB+ VRAM `[UNVERIFIED]` |
| NemotronLabs VoiceChat 11B (Aug 2026) | full duplex, native tool calls | NeMo Apache-2.0 / weights **OpenMDW-1.1, research use** | 80 GB GPU, vLLM; English only; 2-minute context; no backchannel handling; barge-in ~480 ms; turn-taking ~448 ms ([arXiv 2609.21967](https://arxiv.org/abs/2609.21967), [RITS](https://rits.shanghai.nyu.edu/ai/nvidia-releases-nemotronlabs-voicechat-an-open-full-duplex-voice-model)) |
| Sesame CSM-1B | conversational TTS | Apache-2.0 ([HF](https://huggingface.co/sesame/csm-1b)) | — |
| Qwen3-Omni-30B-A3B | omni | Apache-2.0; Qwen3.5-Omni has no Qwen-org weights, Realtime is hosted ([Alibaba](https://www.alibabacloud.com/help/en/model-studio/realtime)) | — |
| Step-Audio 2 mini | audio LLM | Apache-2.0 | — |
| Ultravox v0.7 (GLM-4.6 base) | speech-in LLM | MIT ([HF](https://huggingface.co/fixie-ai/ultravox-v0_7-glm-4_6)) | — |
| Kokoro-82M | TTS | Apache-2.0; the Python package falls back to espeak-ng, **GPL-3.0** ([PyPI](https://pypi.org/project/kokoro/)) | — |
| Piper | TTS | the MIT original was archived 2025-10-06; `piper1-gpl` is **GPL-3.0** ([repo](https://github.com/OHF-Voice/piper1-gpl)) | — |
| Nemotron-speech-streaming 0.6B | streaming STT | NVIDIA Open Model Licence ([HF](https://huggingface.co/nvidia/nemotron-speech-streaming-en-0.6b)) | 80 ms chunks |
| Moonshine v2 | streaming STT | models now MIT; legacy non-English non-streaming on the community licence ([changelog](https://github.com/moonshine-ai/moonshine/blob/main/CHANGELOGS.md)) | Medium Streaming ~74 ms on a MacBook Pro |

- Kyutai's **MoshiRAG** (2026-04-30) does asynchronous retrieval from a text LLM, and RL post-training for interactivity followed (2026-06-10) ([blog](https://kyutai.org/blog)): the same pattern as GPT-Live delegation, a fast duplex front handing hard questions to a text model.

### 8.4 Cascaded pipeline practice `[SOURCE]` (R5 §3)

- **VAD:** Silero VAD, MIT, no telemetry ([repo](https://github.com/snakers4/silero-vad)).
- **Semantic end of turn:** Pipecat **Smart Turn v3.x**, BSD-2-Clause — a Whisper-Tiny encoder of 8M parameters, an 8 MB int8 or 32 MB fp32 ONNX file ([HF](https://huggingface.co/pipecat-ai/smart-turn-v3)); about 12 ms on CPU ([Daily](https://www.daily.co/blog/announcing-smart-turn-v3-with-cpu-inference-in-just-12ms)).
- **Avoid the LiveKit turn detector:** the non-OSI LiveKit Model License ([MODEL_LICENSE](https://github.com/livekit/agents/blob/main/MODEL_LICENSE)).
- **Backchannels:** LiveKit's adaptive interruption model separates "uh-huh" from a real barge-in with a default 1 s cooldown at each turn boundary, but only on LiveKit Cloud ([docs](https://docs.livekit.io/agents/logic/turns/adaptive-interruption-handling.md)); Pipecat offers a min-words interruption strategy instead.
- **Interruption semantics:** keep a playback cursor of what was actually played; on barge-in stop playback locally first, then truncate the assistant turn at the cursor.
- **Reference frameworks:** Pipecat (Python, BSD-2-Clause) and LiveKit Agents (Python/Node, Apache-2.0), both frame pipelines driven by a turn state machine.
- **Echo cancellation:** Apple `setVoiceProcessingEnabled` switches the IO node's input and output into voice processing ([Apple engineer](https://developer.apple.com/forums/thread/733733)); Android `AcousticEchoCanceler.create(sessionId)` on the `AudioRecord` after `isAvailable()` ([Android](https://developer.android.com/reference/android/media/audiofx/AcousticEchoCanceler)); Linux desktop WebRTC APM in Rust via `sonora` (BSD-3-Clause).

### 8.5 Rust voice crates `[SOURCE]` (R5 §4, crates.io 2026-10-01)

| Crate | Version / updated | Licence | Notes |
| --- | --- | --- | --- |
| `livekit` | 0.9.3 / 2026-09-25 | Apache-2.0 | Windows, macOS, Linux, iOS, Android ([README](https://github.com/livekit/rust-sdks)); bundles libwebrtc and its APM |
| `str0m` | 0.24.0 / 2026-09-25 | MIT OR Apache-2.0 | sans-I/O WebRTC; no audio device or APM |
| `webrtc` (webrtc-rs) | 0.21.0 / 2026-09-19 | MIT/Apache | rewritten on a sans-I/O core ([repo](https://github.com/webrtc-rs/webrtc)) |
| `sherpa-onnx` | 1.13.8 / 2026-09-11 | Apache-2.0 | streaming ASR, Silero/TEN VAD, Kokoro and Pocket TTS, Moonshine v2 and Parakeet examples; prebuilt desktop archives and an iOS xcframework; **no Android prebuilds** ([docs.rs](https://docs.rs/sherpa-onnx)) |
| `ort` | 2.0.0-rc.13 | MIT OR Apache-2.0 | ONNX Runtime bindings |
| `whisper-rs` | 0.16.0 | **Unlicense** — not on the permissive list | whisper.cpp |
| `moshi` / `moshi-server` | 0.6.4 / 2025-10-01 | MIT/Apache | last release a year ago |
| `kokoro-tts` | 0.3.3 | Apache-2.0 | `ort` + `cmudict-fast`; no espeak-ng in its dependencies |
| `misaki-rs` | — | MIT | G2P for Kokoro |
| `pocket-tts` | 0.6.2 | MIT/Apache | candle, CPU |
| `silero` | 0.7.0 | MIT/Apache | a Silero VAD wrapper |
| `candle-core` | 0.11.0 | MIT/Apache | behind moshi and pocket-tts |

- **Avoid linking** espeak-ng, Piper (`piper1-gpl`) and `whisper-rs` (R5 (c)).

### 8.6 On-device recognition per platform `[SOURCE]` (R5 §5)

- **Apple (iOS/macOS 26):** `SpeechAnalyzer` + `SpeechTranscriber` give volatile partial results; language assets are system-managed and download on first use; no custom vocabulary; warm time to first partial about 0.3–0.5 s on an iPhone 16e ([dev.to](https://dev.to/simple_memo/ios-26s-speechanalyzer-on-a-live-mic-the-5-things-the-docs-dont-tell-you-2ng5)); you feed it buffers, so it can take the echo-cancelled tap, making duplex feasible when the assistant's speech plays through the same engine `[INFERENCE, R5]`.
- **Android:** `createOnDeviceSpeechRecognizer` exists from API 31 but "is not intended to be used for continuous recognition"; from API 33 `EXTRA_AUDIO_SOURCE` and segmented sessions accept your own echo-cancelled PCM ([RecognizerIntent](https://developer.android.com/reference/android/speech/RecognizerIntent)). Duplex is possible but brittle `[INFERENCE, R5]`; R5 recommends your own STT (sherpa-onnx or Moonshine). That recommendation meets D-5's "no model of its own" on Android in the same way as §8.1: a model would have to come from `_models/` (§13 #41).
- **On-device TTS** (`AVSpeechSynthesizer`, Android `TextToSpeech`) is fine for privacy; quality below Kokoro or Pocket TTS `[UNVERIFIED]`.

### 8.7 keeper's on-device voice and model loading today `[REPO]` (D3 B, G5 §2)

- **The ports** `voice_macos.rs` and `voice_ios.rs` are the thinnest wrappers over `SFSpeechRecognizer`, `AVAudioEngine` and `AVSpeechSynthesizer`; requests set `setRequiresOnDeviceRecognition(true)` and `setShouldReportPartialResults(true)` (macOS `:1706-1708`; iOS `:1738-1739`); results become `FinalHeard` / `PartialHeard` (macOS `:1141-1145`; iOS `:1218-1222`); while the synthesiser speaks, any non-empty transcript becomes `SpeechDetected` (iOS `:113-116`). Locales are classified by `supportsOnDeviceRecognition` (`voice_macos.rs:1365-1405`).
- **The transcription engine is separate.** `trait SpeechEngine` (`keeper-core/src/transcription/engine.rs:31-56`: `availability`, `load`, `audio_tracks`, `decode`, `transcribe`, `diarize`, `embed`) has one implementation, `MacSpeechEngine` (`keeper/src/transcribe_macos.rs:34-105`), one worker thread owning the FluidAudio handle because concurrent Core ML managers crash in BNNS (FluidAudio #661); macOS 15 minimum. Every other target answers `Unsupported` through `AbsentEngine` (`transcribe_ipc.rs:97-99`, `:146-148`) — **iOS has no inference engine**.
- **The engine is the vendored FluidAudio fork** `tools/fluidaudio-rs` (`0.14.8-keeper.1`, MIT; a C-ABI FFI over a SwiftPM bridge pinning FluidAudio `0.17.4`); Core ML `.mlmodelc` packages with hard-coded file lists (`tools/fluidaudio-rs/src/lib.rs:26-50`); `#[cfg(target_os = "macos")]` (`:34`). It cannot host an ONNX VAD.
- **`_models/` is content-agnostic distribution:** `CONFIG_MODELS_DIR = "_models"` (`models.rs:19`), `MODELS_TOML` (`:22`), defaults `parakeet-tdt-0.6b-v3` / `speaker-diarization` / `pyannote-community-1` (`:24-26`), `ModelSet::from_toml` (`:82-112`, single-segment names), per-role file lists and `missing()`, `choose()` refusing an incomplete pick (`:212+`). Hydration into `<data_dir>/models/` is `account_ipc.rs:224-300` (`hydrate_models`), re-spawned after each sync (`:1511-1514`); loading refuses on `missing` and checks a freshness digest (`transcribe_ipc.rs:196-257`).
- **What a VAD needs** `[INFERENCE, D3]`: distribution nearly free (a `[vad] dir` section, a `ModelRole::Vad` with `required = ["model.onnx"]`, a `transcription.vad_model` key mirroring `keys.rs:963-981`); execution through `ort` loading the `.onnx` from the hydrated directory behind a new `SpeechEngine` method or a sibling trait; on macOS beside `transcribe_macos.rs`; on iOS a new port module (e.g. `transcribe_ios.rs`) running `ort` with the Core ML execution provider, the heavier path; in the turn machine a new `TurnEvent` (e.g. `UtteranceEnd`) with the 1800 ms pause demoted to a fallback — a table change in `turn.rs`, no shell logic.
- **The gate it must pass:** `voice_on_device` scans `keeper-core/src/voice/**` and `voice*.rs` in the shell for network APIs (research-transcription §2.5) `[REPO]`; an `ort` build feature that downloads binaries runs at build time, not at run time, but its dylib's licence is outside cargo-deny (§5.10).
- **Smart Turn needs its Whisper feature extraction in Rust** (R5 (c)); P14 puts it in `keeper-ported::smart_turn` (feature extraction only).

### 8.8 What P13 changes in the turn machine

> Coordinator note: the turn machine's first rule after abandonment is "**Barge-in stops speech
> first.** `SpeechDetected` while `Speaking` yields `Effect::StopSpeaking` before any other effect,
> because the person started talking and nothing should still be talking over them"
> (`keeper-core/src/voice/turn.rs:18-22`, Epics 62–63; AD-208 governs what follows). P13 pins "a
> backchannel rule before barge-in stops speech": an "mhm" must not stop the agent (story 97.3's
> title). The two cannot both be literal: either speech stops first and resumes after a
> backchannel, or a classification precedes the stop. R5 §3 says to stop playback locally first and
> puts barge-in-to-silence under 150 ms when the device's VAD ducks locally (§8.9); LiveKit's
> classifier is Cloud-only and no permissive backchannel model was found, so the rule is Pipecat's
> min-words strategy or a duck-then-decide `[INFERENCE]`. The pinned resolution is P13's; the
> amended rule replaces `turn.rs:18-22`'s sentence and its pinning tests, and the D-entry for P13
> should record it. **Settled by ruling R14:** pause first — speech pauses at once; a backchannel (a
> word from the list for the person's language, ruling R24(10), or under 600 ms of speech)
> continues it, the stop phrase ends the turn, anything else stops it and becomes the next question
> (AD-411, D-36).

- **`heard_until`.** Over WebSocket, OpenAI's truncate cuts audio without an aligned transcript (§8.2); keeper's speech is streamed by sentence (`AnswerSentence`, AD-214; `turn.rs:27-35`), so the played sentence is the natural cursor and P13 truncates there `[INFERENCE]`.

### 8.9 For the record: R5's architecture, latency budget, and build list `[INFERENCE, R5]`

R5's recommended architecture put a self-hosted LiveKit SFU between the device and a Rust voice agent on the server, with two modes:
- **privacy-max** — platform AEC and Silero on the device; `livekit` to a self-hosted LiveKit (Apache-2.0) over WireGuard; Silero and Smart Turn via `ort` on the server; Kyutai `stt-1b` in `moshi-server` or Nemotron streaming; an LLM on vLLM; Kokoro, Pocket TTS or Kyutai TTS; PersonaPlex or Moshi optionally for chit-chat and backchannels in MoshiRAG style; Moonshine or SpeechAnalyzer offline;
- **quality-max** — the same capture and transport, bridged to GPT-Live in client delegation mode, each `session.delegation.created` opening or continuing a steward session.

Both send audio off the device and are **not** the chosen option (§8.1). The budget is kept because story 97.2 needs a target:

| Stage | Privacy-max | Quality-max | Basis |
| --- | --- | --- | --- |
| capture + AEC frame | 10–20 ms | 10–20 ms | `[INFERENCE]` |
| Opus + network | 20–60 ms | 40–120 ms | `[INFERENCE]` |
| end of turn (VAD hangover + Smart Turn) | 200–300 ms | inside the model | Smart Turn 12 ms; hangover `[UNVERIFIED]` |
| STT finalisation | 80–500 ms | — | model cards |
| LLM time to first token | 150–400 ms | — | `[UNVERIFIED]` |
| TTS first audio | 100–450 ms | — | Unmute ~450 ms; Kokoro/Pocket `[UNVERIFIED]` |
| model response | — | 300–600 ms | `[UNVERIFIED]`; VoiceChat 448 ms as a reference |
| jitter buffer + playback | 40–80 ms | 40–80 ms | `[INFERENCE]` |
| **total to first audio** | **~0.7–1.3 s** | **~0.4–0.8 s** | |
| barge-in to silence | < 150 ms (device VAD ducks locally) | the same plus server truncation | `[INFERENCE]` |

R5's must-build list: a turn state machine like Pipecat's (keeper has one, §2.9); a backchannel classifier (LiveKit's is Cloud-only); a GPT-Live client (not taken); a MatrixRTC participant (not taken); mobile AEC glue (Android, epic 98). Needs ONNX or Rust work: Nemotron streaming has no Rust path `[UNVERIFIED]`; Kyutai TTS needs moshi-server's Python removed; Smart Turn needs its feature extraction; sherpa-onnx on Android needs native builds.

**MatrixRTC** (R5 §6) `[SOURCE]`: MSC4195 (a LiveKit SFU as the MatrixRTC transport) is merged ([PR](https://github.com/matrix-org/matrix-spec-proposals/pull/4195)); Element Call is AGPL-3.0, a separate client only. Prior art: **hermes-matrix-voice-chat-bridge** (Python, MIT) joins as a cross-signed device, exchanges `io.element.call.encryption_keys` over Olm to-device and feeds the SFrame keys to a LiveKit E2EE KeyProvider with identity `@user:server:deviceId` ([matrix_call.py](https://github.com/bunnyfu/hermes-matrix-voice-chat-bridge)); **openclaw-matrix-voice** (TypeScript, MIT; energy VAD, barge-in off by default) ([README](https://github.com/scottgl9/openclaw-matrix-voice)); **matrix-livekit-assistant** (Python, livekit-agents). All three are half-duplex cascades. Calls are out of scope (§2.10).

---

## 9. Memory, self-improvement and privacy

§9.1–§9.3 and §9.5–§9.6 are R6's reading, `[SOURCE]` unless marked; Hermes' source is cited at `main@663362680b`. §9.4 and §9.7 are R6's recommendations `[INFERENCE, R6]` as P12 and P8 pin them.

### 9.1 How Hermes' learning loop works

| Mechanism | How it works (2026-10-01) |
| --- | --- |
| **Stores** | two files per profile in `~/.hermes/memories/`: `MEMORY.md` (environment facts, **2 200 characters**) and `USER.md` (facts about the user, **1 375 characters**); entries separated by `§`; both injected as a **frozen snapshot** at session start — a write is saved at once but the model sees it next session; **no auto-compaction**: an `add` over the cap returns an error listing the entries, and the agent consolidates in the same turn; exact duplicates rejected; entries scanned for injection, exfiltration and invisible Unicode (Hermes docs/memory, recorded as `https://hermes-agent.nousresearch.com/docs/user-guide…` — the URL is cut in the digest file) |
| **Who writes** | the foreground agent's `memory` tool (`add`, `replace`, `remove` by substring) and a background review fork; `memory.write_approval` (default false) stages non-CLI and background writes for `/memory approve`, a staged replace or remove pinned to the exact entry text |
| **Nudges** | `memory.nudge_interval` triggers a review every **10 user turns**; `skills.creation_nudge_interval` after **15 tool-calling iterations**, reset by `skill_manage` (`cli-config.yaml.example`, `agent/agent_init.py`, `agent/turn_iteration_prep.py`, `agent/turn_finalizer.py`); `spawn_background_review_thread` (`agent/background_review.py:1252`) forks the agent reusing the prompt cache and runs `_MEMORY_REVIEW_PROMPT` (`:335`, route each fact to USER *or* MEMORY) or `_SKILL_REVIEW_PROMPT` (`:400`, "Be ACTIVE… A pass that does nothing is a missed learning opportunity"), which also lists what it must not capture (environment failures, negative claims …; the row is cut in the digest) |
| **Skills** | agentskills.io `SKILL.md`, plus Hermes fields (`version`, `platforms`, `metadata.hermes.{tags,category,requires_toolsets,…}`); in `~/.hermes/skills/`; progressive loading (`skills_list` ~3k tokens → `skill_view(name)` → `skill_view(name, path)`); project > local > external; project skills scanned and quarantined; `skill_manage` create/patch/write_file/remove_file/delete; its linter (`incident-log-shape`, `references-sprawl` > 60 files, `oversized-body` > ~24k chars) **only warns**; `skills.write_approval` stages to `~/.hermes/pending/skills/` (the row is cut in the digest) |
| **Evaluation** | none against outcomes; only view, use and patch counters in `.usage.json` `[INFERENCE, R6]` |
| **Curator** (`agent/curator.py`) | runs when idle, not on cron: at most every 168 h, only after 2 h idle; deterministic part marks skills stale after **14 days** unused and moves them to `.archive/` after **30**; **never deletes**; skips pinned and cron-referenced skills; an LLM consolidation pass is **off by default** (`consolidate:false`, 50–100 calls a sweep); a tar.gz backup first; every mutation to an append-only `.curator_ledger.jsonl` with actor, before/after sha256 manifests, single-entry rollback; manages only background-review skills (the row is cut in the digest) |
| **session_search** | SQLite FTS5 (`messages_fts` in `state.db`), four shapes (discovery, scroll, browse, recent); "**makes no LLM calls**" and returns real message windows; the README's "with LLM summarization" is out of date (`tools/session_search_tool.py` has no summariser); ended sessions pruned after 90 days ([sessions](https://hermes-agent.nousresearch.com/docs/user-guide/sessions)) |
| **Honcho** | an optional external provider (one at a time): one workspace, one user peer across profiles, one AI peer per profile; `peer.chat()` every 2 turns by default ([memory-providers](https://hermes-agent.nousresearch.com/docs/user-guide/features/memory-providers)); **the server is AGPL-3.0**, the Python SDK `honcho-ai` 2.5.1 Apache-2.0 |

### 9.2 Comparable systems

| System | Mechanism | Licence / language |
| --- | --- | --- |
| **OpenClaw memory-core** | tiers: instructions human-only; `MEMORY.md`/`USER.md` curated; daily notes episodic; `DREAMS.md` for review; each entry has a **provenance class** (`owner`, `agent`, `untrusted`, `system`) in SQLite columns the model cannot write; **dreaming** on cron `0 3 * * *` is the *only* writer of the curated tier, promoting through deterministic gates (`minScore`, `minRecallCount`, `minUniqueQueries`); `untrusted` and `system` excluded structurally; cron, heartbeat and sub-agent sessions never produce candidates; **a rewrite that drops more than 25% of prior entries is rejected**; hash compare-and-swap, a stored pre-image, changes logged (the row is cut in the digest) | MIT, TypeScript |
| **Letta** | **MemFS**, a git repository of markdown files with YAML `description`; `system/` always in the prompt; `skills/`; every edit a commit; dreaming sub-agents in git worktrees after N steps or at compaction; an optional agent-reviewed mode ([MemFS](https://docs.letta.com/concepts/memfs), [dreaming](https://docs.letta.com/configuration/memory)); the V1 Python server archived | Apache-2.0, TypeScript |
| **mem0 v3** | **ADD-only** extraction in one pass (UPDATE and DELETE removed); hybrid search (semantic + BM25 + entities); `user_id`, `agent_id`, `run_id` scopes; graph memory moved to the paid platform ([migration](https://docs.mem0.ai/migration/oss-v2-to-v3)) | Apache-2.0, Python |
| **Zep / Graphiti** | a bi-temporal graph: edges with validity windows invalidated rather than deleted; every fact traced to source "episodes"; Zep's hosted engine proprietary ([README](https://github.com/getzep/graphiti)) | Apache-2.0, Python, an external graph DB |
| **Claude Code** | `CLAUDE.md` human-written; auto memory in `~/.claude/projects/<repo>/memory/MEMORY.md`, an index of which only the first 200 lines or 25 KB load, plus typed topic files; **machine-local**; sub-agents separate ([docs](https://code.claude.com/docs/en/memory)) | not open `[UNVERIFIED]` |
| **Codex** | memories off by default in `~/.codex/memories/`; extracted after a chat goes idle, secrets redacted; `memories.disable_on_external_context` keeps chats that used MCP, web or tool search out ([docs](https://developers.openai.com/codex/customization/memories)) | Apache-2.0, Rust |
| **Anthropic memory tool** | `memory_20250818` is **client-side**: six commands (view, create, str_replace, insert, delete, rename) on `/memories`, mapped by the application to e.g. a per-user directory, which must block traversal ([docs](https://platform.claude.com/docs/en/agents-and-tools/tool-use/memory-tool)) | API feature |
| **omp** | `memory.backend` `local` / `hindsight` / `mnemopi` / `sharpshooter`; local consolidates into `MEMORY.md`, `memory_summary.md`, `skills/`; `learn` writes `learned.md` (100 entries ≤ 2 000 chars, redacted, visible next session); mnemopi adds `recall`, `retain`, `reflect`, `memory_edit`, retains every 4 turns, scoped per project (`omp://memory.md`, `omp://mnemosyne-memory-backend.md`) | MIT, TypeScript; Hindsight MIT, Python |
| **agentskills.io** | `SKILL.md` frontmatter: `name` (≤ 64, matches the directory), `description` (≤ 1024), `license`, `compatibility`, `metadata` (string→string), experimental `allowed-tools`; body under 500 lines; `skills-ref validate` ([spec](https://agentskills.io/specification)) | Apache-2.0 |

### 9.3 Failure modes, and what is done about them

- **MINJA** ([arXiv 2503.03704](https://arxiv.org/abs/2503.03704), NeurIPS 2025): the memory bank is poisoned through *queries alone*, with bridging steps and progressive shortening; any co-user of a shared bot can do it.
- **AgentPoison** ([arXiv 2407.12784](https://arxiv.org/abs/2407.12784), NeurIPS 2024): optimised triggers reach over 80% attack success with under 1% benign impact and under 0.1% of the store poisoned.
- **Zombie Agents** ([arXiv 2602.15654](https://arxiv.org/abs/2602.15654)): web content enters memory through the agent's *normal* update and stays; "per-session prompt filtering [is] not sufficient".
- **Radware ZombieAgent** ([SecurityWeek, 2026-01-09](https://www.securityweek.com/zombieagent-attack-let-researchers-take-over-chatgpt/)): a shared file planted persistent memory rules in ChatGPT; fixed 2025-12-16.
- **Skill supply chain** ([OWASP AST02](https://owasp.org/www-project-agentic-skills-top-10/ast02)): publishing to ClawHub needed a `SKILL.md` and a week-old GitHub account; 12.4% of 142 836 skills depend on untrusted external resources; 925 had hijackable sources; fixes: sign a canonical digest, pin by `sha256`, an internal mirror, revocation.
- **Drift:** Hermes' review prompt warns that negative claims "harden into refusals" and that unresolved attempts get written up as "best practice" (`background_review.py:400+`); Hermes and OpenClaw both warn that two writers on one memory home compound each other's state.

| Mitigation | Where it is used |
| --- | --- |
| provenance at write time | OpenClaw origin classes; Collaborative Memory's immutable fragment provenance ([arXiv 2505.18279](https://arxiv.org/abs/2505.18279)) |
| human review of memory diffs | Hermes `write_approval` and `/skills diff`; Letta git commits |
| quarantine | Hermes project-skill scans; OpenClaw never promotes untrusted content |
| TTL / decay | Hermes curator (14/30 days) and 90-day prune; OpenClaw 30-day half-life; Graphiti invalidation |
| bounded rewrites | OpenClaw's 25% loss cap |
| rollback | Hermes' ledger |
| taint exclusion | Codex `disable_on_external_context`; OpenClaw turn taint |

### 9.4 The design, as P12 pins it

**R6's recommended layout and write rights** `[INFERENCE, R6]`, mapped onto P2's `80-agents/` (ruling R1):

| R6 path | Who writes (R6) | As pinned (P2, P12) |
| --- | --- | --- |
| `SOUL.md`, `AGENTS.md`, workflows | humans only | `80-agents/<agent>/SOUL.md` (BMAD fields: role, identity, communication style, principles, persistent facts — humans only); `80-agents/_workflows/<name>/` |
| `memory/episodic/YYYY-MM-DD.md` | the agent, append-only | `80-agents/<agent>/journal/YYYY-MM-DD.md`, append-only |
| `memory/proposals/<id>.md` | the foreground turn and the post-session review, staged | `80-agents/<agent>/proposals/<ulid>.md` |
| `memory/core/{USER,MEMORY}.md` | only the consolidator or a human; hard caps; over-cap is an error | `80-agents/<agent>/USER.md`, `MEMORY.md`, "hard char caps, written only by the consolidator or a human" |
| `skills/<name>/SKILL.md` | consolidator or human; `metadata.*` for provenance | `80-agents/_skills/<name>/SKILL.md`, shared by the drive's agents |

- **Where the files go** answers round 3's "memory, skills, soul, etc bot data find a right place in the drives fro this files (tgdrive i neuradrive)": the same zone in both drives, `80` being the unused content number (`/workspace/tgdrive/README.md:9-24`) `[REPO]`.
- **Review gates (R6):** a change to a human's `USER.md` needs that human's approval; a skill or core-memory change on a **shared** drive needs the drive owner's; on a private drive, low-risk edits from trusted sources may apply automatically; every change is a git commit with trailers `Memory-Origin:`, `Source-Session:`, `Reviewed-by:`. P12 pins the first two trailers and "Shared drives: owner review".
- **Provenance fields (R6):** `id`, `principal`, `drive`, `label{conf,integ}`, `origin_class`, `session_kind`, `source_session`, `source_sha256[]`, `author_bot`, `host`, `observed_at`, `supersedes`, `valid_until`, `importance`, `review{state,by,at}`.
- **Schedule (R6, pinned by P12):** during use, after 10 user turns or 15 tool iterations, write proposals only; **nightly** on the always-on host under a lease — promote proposals through deterministic gates, never promote `untrusted` content or content from cron or delegated sessions, reject a rewrite that drops more than 25%, commit with a compare-and-swap against the git base; **weekly curator** — stale after 14 days, archive after 30, never delete, LLM consolidation opt-in; episodic notes 90 days and untrusted-derived proposals 30 days unless endorsed (R6; not in P12's text).
- **Knowledge harvest (P12):** sessions → OKF notes with `human_reviewed: false` → the promote panel (FR-243/FR-244, ruling R26; §2.5). OKF's rule "never sign as `human:` when you are an agent" (§3.8) sets the actor: `agent:<agent>@<host>` (ruling R26).

**Port or borrow** (R6 (c)), and where P14 puts it:

| Upstream (licence) | R6's verdict | P14 module |
| --- | --- | --- |
| Hermes memory tool, char caps, staged approvals (MIT) | **port** | `keeper-ported::hermes` |
| Hermes review prompts (MIT) | borrow with attribution (prompt text) | — |
| Hermes curator state machine (MIT) | **port** the deterministic part (~1 KLOC `[UNVERIFIED]`); git is the ledger | `keeper-ported::hermes` |
| Hermes FTS5 session_search (MIT) | **port** on rusqlite (rusqlite's licence `[UNVERIFIED]`) | — (keeper already ships FTS5, §2.7) |
| OpenClaw dreaming gates and provenance (MIT) | **port** the gates, scoring and loss cap | `keeper-ported::openclaw` |
| Letta MemFS (Apache-2.0) | borrow; the git-synced drive already is it | — |
| mem0 (Apache-2.0) | borrow ADD-only extraction and hybrid ranking | — |
| Graphiti (Apache-2.0) | borrow `valid_at`/`invalid_at` | — |
| **Honcho (AGPL-3.0 server)** | never link; external only, or borrow the peer model | — |
| Claude Code auto memory | borrow the index + topic files with a 200-line cap | — |
| Codex memories (Apache-2.0) | borrow the idle gate and `disable_on_external_context` | — |
| Anthropic memory tool | **implement** the six-command handler | not pinned |
| omp `learn` / Mnemopi (MIT) | borrow caps and bank scoping | — |
| agentskills `skills-ref` (Apache-2.0) | **port** the validator | `keeper-ported::agentskills` |
| CaMeL (Apache-2.0, unmaintained) | borrow capabilities and the dependency graph | — |
| FIDES (MIT notebook) | borrow: build the lattice, Hide and `query_llm` natively | — (labels in `keeper_core::agents`, story 89.4) |

### 9.5 Information-flow control for agents

- **Design Patterns** ([arXiv 2506.08837](https://arxiv.org/abs/2506.08837)): action-selector, plan-then-execute, LLM map-reduce, dual LLM, code-then-execute, context-minimisation. In the dual-LLM pattern a privileged LLM never sees untrusted data and handles quarantined outputs only by reference; caveat: the quarantined output "could have been tampered".
- **CaMeL** ([arXiv 2503.18813](https://arxiv.org/abs/2503.18813)): a planner LLM writes restricted Python; an interpreter tracks a per-variable **dependency graph**; each value carries **capabilities** (provenance and allowed readers); policies checked before every tool call (block or ask); **STRICT** mode closes control-flow leaks; 77% of AgentDojo tasks with provable security against 84% undefended; side channels remain (an exception leaks 1 bit; timing) — the paper suggests Rust-style `Result` types; the repository is Apache-2.0 and "not… maintain[ed]".
- **FIDES** ([arXiv 2505.23643](https://arxiv.org/abs/2505.23643); [microsoft/fides](https://github.com/microsoft/fides), MIT, a notebook): labels are a product lattice, integrity {T, U} × confidentiality as a **set of readers**; labels ride node `metadata` and children inherit; **an LLM response's label is the join of its whole context**; **Hide** puts a label-raising tool result into a variable instead of the context; **query_llm** is a quarantined LLM with constrained output and a capacity lattice `bool ⊑ enum ⊑ string`; policies **P-T** (a tool call only if decided from trusted context) and **P-F** (data flows only to permitted readers); implicit flows not covered.
- **SPA** ([arXiv 2608.27234](https://arxiv.org/abs/2608.27234)): results persisted as *labelled artifacts*, the planner shown only their metadata; attack success 0% on AgentDojo and 0.2% on its multi-query variant.

| Channel | Propagation |
| --- | --- |
| context | output = ⊔ of all inputs (FIDES Algorithm 5) |
| tool results | trusted wrappers assign labels; pure functions join arguments; web results U (FIDES); network tools taint the rest of the turn (OpenClaw) |
| control flow | CaMeL STRICT |
| delegation | **no paper covers durable inter-agent sessions** `[INFERENCE, R6]`: treat it as a tool call — the request carries the sender's context label, the reply the callee's final label; OpenClaw already keeps sub-agent sessions out of memory |
| memory | labelled artifacts (SPA); provenance columns (OpenClaw); private and shared tiers (Collaborative Memory) |

### 9.6 Isolation and memory scoping in practice

- **OpenClaw** is "**not** a hostile multi-tenant security boundary"; "Session IDs select routing; they do not authorize"; the answer is one container cell per tenant, escalating to gVisor or Kata, then separate machines ([multi-tenant-hosting](https://github.com/openclaw/openclaw/blob/main/docs/gateway/multi-tenant-hosting.md)).
- **Hermes:** "Profiles do **not** sandbox the agent"; never two processes on one home; shared memory through an external provider ([profiles](https://hermes-agent.nousresearch.com/docs/user-guide/profiles)).
- **Grok Bot:** one Firecracker microVM per user; all of a user's Bots share its files, cookies and credentials; connectors are account-wide; "The screens are separate work surfaces, not separate security boundaries" ([computer](https://docs.x.ai/grok-bot/computer-and-apps.md), [teams](https://docs.x.ai/grok-bot/teams-and-enterprises)).
- **Scoping:** Claude Code per repository per machine; Codex per `CODEX_HOME`; mem0 `user_id`/`agent_id`/`run_id`; Honcho workspace and peers; Letta per-agent MemFS plus explicit shared memory; omp memory banks; Anthropic per-user mapping of `/memories`.
- **So** `[INFERENCE, R6]`: labels are enforced only inside the application, and the products' own docs say a shared process or VM is not a boundary; private agents run under a separate OS user or container per principal. That is P5's `agentd-<principal>` under its own OS user and systemd unit, and P8's layer 1.

### 9.7 Labels, as P8 pins them

**R6's design** `[INFERENCE, R6, after FIDES and CaMeL]`:
- **Confidentiality** is a set of human readers (`{alice}` private, `{alice,bob}` a shared drive, `*` public); joining takes the **intersection**.
- **Integrity**, most to least trusted: `owner` (typed by the principal in a trusted channel) ⊐ `peer` (the other human) ⊐ `agent` (derived by a bot) ⊐ `untrusted` (web, email, third parties); joining takes the **minimum**.
- **Capacity** on quarantined outputs: `bool ⊑ enum ⊑ string`.
- **Clearance:** each agent's audience is the readers it serves; each drive has a label.
- **Propagation:** trusted wrappers label every file and tool result (web, email and inbound messages `untrusted`); the session's label is the join of everything read; content that would lower readers or integrity is hidden by reference, used through a quarantined LLM with a schema; a delegation carries the sender's label and the receiver's session starts there; a memory write inherits the session's label, may go only to a drive whose readers ⊆ the label's readers, and promotion requires `agent` or better.

| Condition | Result |
| --- | --- |
| a message, tool call or file write to a sink whose audience is not within the data's readers | **block**, unless the owning human declassifies; logged |
| a consequential call (send, spend, delete, exec, write to a shared drive) decided in `untrusted` context (FIDES P-T) | **block** or ask a human |
| a recipient, path or target argument derived from `untrusted` data | **block**; message *bodies* may be untrusted |
| promoting memory or a skill from `untrusted`, cron or sub-agent sources | **block** |
| loading a skill whose hash or signature does not match its reviewed version | **quarantine** |

**As P8 pins it:** three layers — a process per principal (OS user), grants (no grant ⇒ no access), labels; label = (readers: a set of human Matrix ids, integrity `owner ⊐ peer ⊐ agent ⊐ untrusted`), join = readers ∩, integrity min; the session's label is the join of everything read; a send, write or delegation to a sink whose audience ⊄ the label's readers is blocked unless the owning human declassifies (an audit row); consequential calls under `untrusted` are blocked or need approval; sending into a room requires every human member and every agent audience ⊆ the label's readers; sensitive agents (the psychologist, tgdrive-sensitive work) pin a local model.

- **Not pinned from R6:** the capacity lattice and Hide-by-reference; the "recipient derived from untrusted ⇒ block" row; skill-hash quarantine. Story 89.4 and 92.6 decide whether each lands (§13 #1, #4).
- **A local model is a context budget.** The operator's Ollama LXC pins a 16 384-token context, and Hermes refuses agent use below 64k (§3.1, §3.4); a sensitive agent pinned to it gets a small window `[INFERENCE]` (§13 #48).

---

## 10. BMAD's format and execution model

All of §10 is G4's reading of the BMAD method as installed `[REPO]`: `S/` = `~/.claude/plugins/cache/bmad-method/bmad/6.12.0.0/skills/` (111 skill directories, `.claude-plugin/plugin.json:9`); the repository's `_bmad/` holds config, catalogues and five Python scripts. Installed versions: core/bmm 6.12.0, bmb v2.2.2, cis v0.3.2, gds v0.7.2, tea v1.26.0, bmad-loop v0.11.1 (`_bmad/_config/manifest.yaml:2`, `:8-58`). G4 read end to end `bmad-architecture` (and its deprecated alias `bmad-create-architecture`), `bmad-create-epics-and-stories`, `bmad-agent-architect`, `bmad-build`, `bmad-build-auto`, `bmad-dev-story`, `bmad-loop-sweep`, `bmad-loop-resolve` and `bmad-loop-setup`.

**The one-sentence finding:** BMAD has no runtime of its own. The LLM is the interpreter; it reads markdown instructions just in time, runs a handful of Python helpers through `uv run`, and keeps its state in the frontmatter or logs of the files it writes. The only deterministic orchestrator, `bmad-loop`, is an external Python program.

### 10.1 How a persona is defined

| Layer | Content | Evidence |
| --- | --- | --- |
| Descriptor | `[agents.<code>]` with `module`, `team`, `name`, `title`, `icon`, `description` only; rewritten on every install; custom agents in `_bmad/custom/config.toml` | `_bmad/config.toml:1-11`, `:77-83` |
| Agent = skill | no separate agent file: `SKILL.md` (`name`, a `description` that tells the host when to trigger) plus `customize.toml [agent]` | `S/bmad-agent-architect/SKILL.md:1-4` |
| Persona fields | `name`/`title` fixed; configurable `icon`, `activation_steps_prepend/append`, `persistent_facts` (text or `file:` globs), `role`, `identity`, `communication_style`, `principles[]` | `customize.toml:8-49` |
| Menu | `[[agent.menu]]` with `code`, `description`, and exactly one of `skill` or `prompt` (Winston: `CA → bmad-architecture`, `IR → bmad-sprint-planning`) | `customize.toml:51-63` |
| Activation | 8 steps: resolve with `resolve_customization.py --key agent` (or merge by hand) → prepend steps → adopt the persona → persistent facts → `_bmad/bmm/config.yaml` → greet with the icon → append steps → show the menu, **stop and wait**, fuzzy-match, invoke the skill | `SKILL.md:21-31`, `:33-76` |

- **The voice reaches the model only through the prompt:** "embody `{agent.identity}` … follow `{agent.principles}`", kept through any skill it invokes (`SKILL.md:39-41`, `:76`); workflows inherit it ("in addition to your name, communication_style, and persona", `S/bmad-create-epics-and-stories/SKILL.md:10`).
- **Party mode and elicitation see only the one-line `description`** (`resolve_party.py:62-68`; `bmad-advanced-elicitation/SKILL.md:64`).
- **For the program:** P2's `SOUL.md` carries exactly the configurable persona fields (role, identity, communication style, principles, persistent facts); story 89.1's `keeper-ported::bmad` merges a BMAD agent's `customize.toml` into it (the "SOUL import", program map) `[INFERENCE]`.

### 10.2 How a workflow is defined

| Format | Layout | Example |
| --- | --- | --- |
| A. outcome-driven | `SKILL.md` + `customize.toml [workflow]` + `assets/` + `references/` + `scripts/` | `S/bmad-architecture/SKILL.md:49-60`; `customize.toml:9-101`; `scripts/lint_spine.py` |
| B. rendered step files | `SKILL.md` is a loader running `render_skill.py`, then `workflow.md` + `step-0N-*.md` + `spec-template.md` + `review-prompts/` | `S/bmad-build/SKILL.md:6-13`; `workflow.md:57-84` |
| C. classic step files | `steps/step-NN-*.md` (+ `step-01b-continue`) + `templates/` + checklist + CSV/YAML; testarch adds `steps-c/e/v` and a `workflow.yaml` | `S/bmad-create-epics-and-stories/SKILL.md:19-48`, `:93` |
| D. XML-like inline | `<workflow><step n><check if><action><ask><goto>` | `S/bmad-dev-story/SKILL.md:80-504` |

- **Frontmatter:** `SKILL.md` has `name`, `description`, optional `metadata.lifecycle: shim`; classic steps have `name`, `description`, `workflow_path`, `thisStepFile`, `nextStepFile`, `continueStepFile`, `outputFile`, `templateFile`, plus knowledge files; rendered steps declare runtime variables (`spec_file`, `story_key`); testarch `workflow.yaml` has `config_source`, `"{config_source}:output_folder"`, `default_output_file`, `required_tools`, `execution_hints`.
- **Variables:** `{project-root}`, `{skill-root}`, `{skill-name}`; bare paths from the skill root. Central config is a 4-layer TOML merge `config.toml` → `config.user.toml` → `custom/config.toml` → `custom/config.user.toml` (`_bmad/scripts/config_utils.py:98-107`); values keep a **literal** `{project-root}` and the LLM substitutes it ("never double-prefix"). Legacy per-module `config.yaml` is still read.
- **Overlays:** `customize.toml` → `_bmad/custom/<skill>.toml` → `<skill>.user.toml`; scalars override, tables deep-merge, arrays of tables keyed by `code`/`id` replace or append, other arrays append (`config_utils.py:14`, `:37-88`, `:110-119`).
- **Render tokens (format B):** `{{config.a.b}}`; `{{.key}}` (must name exactly one leaf); `{workflow.x}` (lists as bullets, `review_layers` as sections); `[[bmad-snapshot:f.md]]` → an absolute path. Output is an immutable, content-hashed folder `_bmad/render/<skill>/<slug>-<hash12>/<gen20>/` with a `manifest.json`; the script prints `read and follow <workflow.md>` (`render_skill.py:30-33`, `:140-189`, `:232-267`, `:322-380`, `:396`).
- **A defect, observed:** `implementation_artifacts` and `planning_artifacts` are each defined in both `[modules.bmm]` and `[modules.gds]` (`config.toml:19-20`, `:32-33`). `_resolve_replacements` on `bmad-build` raised `RenderError: ambiguous config value 'implementation_artifacts'`, which the CLI turns into `HALT:` (`render_skill.py:146-148`, `:393-395`). So in this install `bmad-build`, `bmad-build-auto`, `bmad-dev-auto` and the `bmad-loop` dev passes stop at load. `keeper-ported::bmad`'s render port meets the same ambiguity on the same config (§13 #42).

### 10.3 The execution model

- **Who interprets:** the LLM only. "Just-In-Time Loading … NEVER load multiple step files simultaneously … halt at checkpoints" (`bmad-build/workflow.md:59-80`); each step ends with "Read fully and follow `./step-03…`"; jumps are prose (EARLY EXIT, `<goto step="9">`). Scripts handle only config merging, rendering, memlog writes, linting, the party roster and the elicitation catalogue.
- **Progress:** classic (C) — a `stepsCompleted` array in the output's frontmatter, saved before the next step; build (B) — spec `status: draft|ready-for-dev|in-progress|in-review|done` plus `route`, `review_loop_iteration`, `context`, `baseline_commit`, review loopbacks capped at 5; architecture (A) — an append-only `.memlog.md`, written atomically, read only on resume (`_bmad/scripts/memlog.py:18-35`, `:61-67`); dev-story (D) — task checkboxes, `sprint-status.yaml`, and a "Senior Developer Review (AI)" section.
- **Resume:** classic — `step-01b-continue` routes after the highest completed step; build — step 1 routes on the spec's `status`; architecture — resume from the memlog.
- **Human checkpoints:** `[A] Advanced Elicitation [P] Party Mode [C] Continue` (only C advances); build's CHECKPOINT 1 (Approve and continue / Approve and stop / Review), after which `<frozen-after-approval>` is locked; architecture's Coaching vs Fast path; elicitation offers 5 of 71 methods (`assets/methods.csv`).
- **Sub-agents by prose, never a named tool:** "Spawn a subagent synchronously", with a fallback to inline; review layers are context-free subagents in `[[workflow.review_layers]]`, launched in parallel and awaited; patches go back to the *same* implementation subagent by id; with no subagents, `bmad-build-auto` HALTs.

| Artifact | Path rule | Evidence |
| --- | --- | --- |
| spec | `{implementation_artifacts}/spec-{slug}.md` | `bmad-build/step-01-clarify-and-route.md:96` |
| epic context | `epic-<N>-context.md` | `:57-65` |
| deferred work | `deferred-work.md` | `:87` |
| auto-run halt | `bmad-build-auto-result-<slug>.md` | `bmad-build-auto/workflow.md:32` |
| spine | `{planning_artifacts}/architecture/architecture-{project_name}-{date}/ARCHITECTURE-SPINE.md` | `bmad-architecture/customize.toml:53-54` |
| epics | `{planning_artifacts}/epics.md` | `step-01-validate-prerequisites.md:80` |
| party keepsake / memory | `{output_folder}/party-mode/`, `…/memories/<party>/.memlog.md` | `bmad-party-mode/customize.toml:52`, `:68` |
| loop result | `$BMAD_LOOP_RUN_DIR/tasks/$BMAD_LOOP_TASK_ID/result.json` | `bmad-loop-sweep/automation-mode.md:10` |

### 10.4 Several personas at once

- **Party mode** is one orchestrator LLM over a roster (`[agents.*]` ∪ `[[workflow.party_members]]`), in four modes: `session` (one mind voices every persona), `auto`, `subagent` (one long-lived handle per persona, each fed the whole room every round), `agent-team` (peer messaging, Claude Code only). Each party has its own memlog, distilled by a reader subagent on entry.
- **PM → architect → dev hand-offs** have no runtime link, only files and catalogue order (`_bmad/_config/bmad-help.csv:15`, `:24-27`); `bmad-help` recommends "a fresh context window" per skill and detects completion by fuzzy file presence.
- **bmad-loop** (installed with `uv tool install … git+https://github.com/bmad-code-org/bmad-loop`) spawns fresh CLI sessions (claude, codex, gemini, copilot, antigravity) in tmux to run `bmad-build-auto`, re-runs on the finished spec for review, runs `bmad-loop-sweep`. Its contract: environment `BMAD_LOOP_MODE`, `_RUN_DIR`, `_TASK_ID`; hook events `events/{ts}-{task}-{event}.json`; a `result.json` validated field by field. A CRITICAL escalation pauses the run until `/bmad-loop-resolve <key>` writes a resolution marker. Policy in `.bmad-loop/policy.toml`; human answers in `.bmad-loop/decisions.json` keyed `DW-n`.
- **File-based:** config, customisations, specs and their `status`, `stepsCompleted`, memlogs, `sprint-status.yaml`, `deferred-work.md`, loop JSON. **Conversational:** menus and answers, persona voice, party turns, elicitation, subagent prompts and summaries.
- **For the program** `[INFERENCE]`: BMAD's "spawn a subagent" is an in-turn helper. P7 keeps in-turn helpers (review passes) and forbids them from owning work; story 94.4 ("helper sessions and review layers") is where BMAD's review layers land.

### 10.5 What a runtime must provide, and where the program provides it

| Capability BMAD assumes | Assumed by | The program `[INFERENCE]` over the program map |
| --- | --- | --- |
| read a whole file or a range | `bmad-dev-story/SKILL.md:102-104`; `bmad-build/workflow.md:69` | `drive_read` (§2.3) |
| write files; edit YAML frontmatter in place | `bmad-build/step-03-implement.md:21`, `:25` | `drive_write` / `drive_edit` |
| list / glob | `step-01-clarify-and-route.md:49`; `gds-game-architecture/steps/step-01-init.md:65-68` | `drive_list` / `drive_glob` |
| grep the code, read `git log` | `bmad-loop-sweep/SKILL.md:47-49` | `drive_grep`; `git log` not offered |
| run commands (`uv run`, Python ≥ 3.11) | `bmad-agent-architect/SKILL.md:23`; `bmad-build/SKILL.md:9` | replaced by Rust ports of the helpers (P11, §10.6); story 96.1's `run` for the rest |
| git: `rev-parse`, diff to a temp file, commit | `bmad-dev-story/SKILL.md:281`; `step-05-present.md:21` | the drive engine commits; coding goes through Paseo (story 96.3) |
| run tests and linters | `bmad-dev-story/SKILL.md:355-361` | story 96.1 / 96.3 |
| ask the user and wait (HALT, menus) | `step-02-design-epics.md:202-216`; `bmad-build/step-02-plan.md:52-56` | `ask_human` through the proxy (P11), parked like an approval |
| invoke a skill by name | `bmad-agent-architect/SKILL.md:74` | the workflow tool surface (story 94.2) |
| spawn sync / parallel context-free subagents | `step-04-review.md:6-7`; `reviewer-gate.md:9` | helper sessions (story 94.4) |
| re-address a live subagent by id | `step-04-review.md:65`; `mode-subagent.md:7` | helper sessions by id (story 94.4) |
| agent teams + a capability probe | `mode-agent-team.md:3`; `bmad-testarch-atdd/steps-c/step-04-generate-tests.md:123-137` | delegation (P7) |
| per-agent model choice | `mode-subagent.md:29-31`; `step-04-review.md:6` | `agent.toml`'s model (P2) |
| web search | `bmad-architecture/SKILL.md:17`; `bmad-party-mode/SKILL.md:15` | not in the program map |
| MCP / external systems | `bmad-architecture/customize.toml:68-83` | MCP servers per agent (story 96.2) |
| headless / TTY detection, env vars | `bmad-architecture/references/headless.md:3` | the run's origin (`TurnOrigin`, story 90.1) |
| open an editor / HTML report | `bmad-build/customize.toml:46-61` | surface tools in the notes view (story 91.3) |
| token counting, the current date | `bmad-build/step-02-plan.md:26`; `bmad-dev-story/SKILL.md:58` | the host |
| session lifecycle hooks, tmux | `.bmad-loop/bmad_loop_hook.py:4-13` | the session log's events |

### 10.6 What a Rust runtime can parse, and what only an LLM can

**Deterministic** (story 94.1's `keeper-ported::bmad`: "config merge, overlays, render tokens, memlog, phase graph, party roster"):
- the TOML config layers and `customize.toml`: port `structural_merge` (`config_utils.py:79-95`) exactly;
- CSVs: `skill-manifest.csv` (`canonicalId,name,description,module,path`; the `path` column is stale — 0 of 111 paths exist under the repository, all 111 exist by `name` in `S/`), `bmad-help.csv` (13 columns incl. `phase`, `preceded-by`, `followed-by`, `required`, `outputs`), `files-manifest.csv` (1 418 rows, 25 under `_bmad/`), `methods.csv`;
- YAML and frontmatter: `manifest.yaml`, module `config.yaml`, all frontmatter, `sprint-status.yaml` (status values in comments at `:8-34`, the map from `:50`);
- line grammars: the DW ledger (`bmad-loop-sweep/deferred-work-format.md:42-76`), memlog lines (`memlog.py:37-59`);
- render tokens (`render_skill.py:30-33`);
- JSON: loop `result.json`, `escalation.json`, `context.json`, `resolution.json`; `policy.toml`; `decisions.json`; hook events; `lint_spine.py` and `resolve_party.py` output.

**Only an LLM:** every step body, including the conditions inside `<check if="requirements ambiguous">`; instructions like "do not rely on filename patterns or regex — reason about the intent"; the pseudo-JavaScript `runtime.canLaunchSubagents?.()`; `{{story_key}}` runtime variables; the `"{config_source}:x"` indirection; review triage and headless detection; dangling references the LLM tolerates (`{workflow_path}/workflow.md`, absent in `gds-game-architecture/`).

### 10.7 Not established by G4

`bmad-prd`, `bmad-spec`, `step-oneshot.md` and `compile-epic-context.md` were not read; the `bmad-loop` orchestrator's source is not on disk; BMAD-METHOD's own licence is not stated locally (only the vendored module template carries MIT, `templates/module-template/LICENSE:1`) `[UNVERIFIED]`; the render CLI was not run (it writes `_bmad/render`). All in §14.

---

## 11. Proxy, launcher, iOS limits, file coordination, Android

§11.1–§11.5 are R4's reading, `[SOURCE]` unless marked; §11.6 is R7 §6, R5 and G5 §3.

### 11.1 CLIProxyAPI

| Aspect | Finding |
| --- | --- |
| language / licence | Go, MIT; ~53.6k stars ([repo](https://github.com/router-for-me/CLIProxyAPI)) |
| cadence | v8.0.5, v8.0.6 and v8.0.7 all on 2026-09-30 ([v8.0.7](https://github.com/router-for-me/CLIProxyAPI/releases/tag/v8.0.7)) |
| deployment | one binary per OS/arch (Linux glibc-2.17 with plugins; a `no-plugin` musl/OpenWrt build; FreeBSD arm64 no-plugin); Dockerfile and compose; a Go-only embedding SDK (`docs/sdk-usage.md`) |
| OAuth upstreams | `vertex, aistudio, antigravity, claude, codex, kimi, xai, meta` ([config.example.yaml](https://raw.githubusercontent.com/router-for-me/CLIProxyAPI/main/config.example.yaml) `oauth.model-alias`) plus Devin; README: Claude Code, Codex, Grok Build, Antigravity |
| removed upstreams | **Qwen** on 2026-04-15 ([8fac296](https://github.com/router-for-me/CLIProxyAPI/commit/8fac29631db5cbcd69f396592f4718e165464724)); **Gemini CLI OAuth** on 2026-06-18 ([78ba8ba](https://github.com/router-for-me/CLIProxyAPI/commit/78ba8ba731dd531437947a6e0aadda4c13817907)); reasons not given |
| API-key upstreams | Gemini, Codex, Claude (incl. Anthropic-compatible gateways such as DeepSeek or Kimi), xAI, Meta, any OpenAI-compatible provider such as OpenRouter |
| downstream endpoints (`internal/api/server_routes.go`) | `GET /v1/models` (one merged list), `POST /v1/chat/completions`, `/v1/completions`, `/v1/messages`, `/v1/messages/count_tokens`, `POST /v1/responses` plus a WebSocket on `GET /v1/responses`, `/v1/responses/compact`, `/v1/realtime*`, `/v1/images/*`, `/v1/videos*`, `GET /v1beta/models`, `/v1beta/models/*action`, `/v1beta/interactions`, `/backend-api/codex/responses` |
| multi-account | `round-robin`, `weighted-round-robin`, `fill-first`; optional `session-affinity` (TTL 1 h) keeps a session, and subagents, on one account for the prompt cache; retries 403/408/429/5xx with credential cooldown |
| other | a management API at `/v8/management` (`/v0` deprecated); mDNS `_ai-gateway._tcp`; dynamic-library plugins run as trusted in-process code |

- **Fidelity across protocols** — five issues closed as fixed in September 2026: truncated tool calls reported as `stop_reason: tool_use` ([#5836](https://github.com/router-for-me/CLIProxyAPI/issues/5836)); content blocks interleaved out of order ([#5581](https://github.com/router-for-me/CLIProxyAPI/issues/5581)); unpaired tool calls in Codex Responses histories producing Anthropic 400s ([#5805](https://github.com/router-for-me/CLIProxyAPI/issues/5805)); restoring a cloaked tool alias aborting the stream when the model invented a name ([#6208](https://github.com/router-for-me/CLIProxyAPI/issues/6208)); tool identity lost when frames were split mid-stream ([#6218](https://github.com/router-for-me/CLIProxyAPI/issues/6218)). Same protocol on both sides is good; across protocols, expect a trickle of edge cases `[INFERENCE, R4]`.
- **keeper's side** `[INFERENCE]`: keeper speaks only Chat Completions (AD-149), so any Claude or Codex model behind CLIProxyAPI is a protocol conversion; keeper already keeps partial replies, caps tool arguments at 1 MiB and retries only before the first byte (§2.2). The routes P10 needs (`GET /v1/models` for discovery and health, `POST /v1/chat/completions`) are in the route list, which settles G1's `[UNVERIFIED]` "CLIProxyAPI's routes" for those two.
- **Similar projects:** [sub2api](https://github.com/Wei-Shaw/sub2api) (Go, ~43k stars, built for sharing one subscription, "拼车共享"), [OmniRoute](https://github.com/diegosouzapw/OmniRoute) (TypeScript), [9router](https://github.com/decolua/9router) (JavaScript), [copilot-api](https://github.com/ericc-ch/copilot-api) (Copilot to OpenAI/Anthropic).
- **Where it runs for this program:** `https://electra.siren-alsephina.ts.net:8452`, `/v1/models` and `/v1/chat/completions`, its token at `~/.omp/cliproxyapi.token`, never logged (ruling R13); outside makistack's infrastructure code (§3.11). Tests take the address from `KEEPER_OPENAI_SMOKE_BASE_URL`, never a literal (ruling R28 S-20).

### 11.2 The terms-of-service risk

- **Anthropic:** OAuth is "designed to support ordinary use of Claude Code and other native Anthropic applications"; developers may not "route requests through Free, Pro, or Max plan credentials"; Anthropic "may [enforce] without prior notice" ([legal page](https://code.claude.com/docs/en/legal-and-compliance)). The Register reports that OAuth in "any other product… including the Agent SDK" violates the Consumer Terms ([The Register, 2026-02-20](https://www.theregister.com/software/2026/02/20/anthropic-clarifies-ban-on-third-party-tool-access-to-claude/5014546)).
- **CLIProxyAPI's Claude handling** includes "the Claude Code CLI disguise and system prompt replacement" (`disable-claude-cloak-mode`): the spoofing Anthropic says it acts against.
- **OpenAI:** signing in with ChatGPT in open-source clients for your own use is reported as supported; proxies that pool a subscription (sub2api) are "flagged by fraud-prevention systems" — a secondary report of an X post by Tibo Sottiaux ([explainx.ai, 2026-08-21](https://explainx.ai/blog/codex-usage-limits-sub2api-sign-in-chatgpt-august-2026)).
- **A compliant pattern:** Raycast's "Bring Your Own Subscription" runs the user's installed `claude` (≥ 2.1.245) and `codex` (≥ 0.142.5) binaries and never handles credentials ([Raycast](https://manual.raycast.com/ai/bring-your-own-subscription)).
- **R4's recommendation:** speak Messages, Responses and Chat Completions natively; treat any proxy as an optional external endpoint on the server, never a sidecar; pair each client with an upstream of the same protocol, turn on `session-affinity`, pin versions; do not route Claude subscription OAuth through it.
- **Where keeper stands** `[INFERENCE]`: D-4 refuses a *keeper-operated* proxy (`docs/decisions.md:120`); the owner's own endpoint is "the endpoint is yours" (P10). keeper ships no proxy, recommends none and sends nothing the person did not configure. Which upstreams the owner's CLIProxyAPI uses is the owner's choice and the owner's terms risk (§13 #29). **Ruling R28 S-20** gives that risk an owner and a moment: before 91.5 the owner records in `docs/agents.md` which upstreams sit behind the endpoint, whether its cloak mode (`disable-claude-cloak-mode`) is on, and that they accept the risk; `agents init` names no default bot.

### 11.3 Launcher UX: ask quickly, reach the right agent, see the result

- **macOS 26 Spotlight:** "hundreds of actions"; "any app can provide actions to Spotlight using the App Intents API"; "quick keys" jump to an action ([Apple Newsroom](https://www.apple.com/newsroom/2025/06/macos-tahoe-26-makes-the-mac-more-capable-productive-and-intelligent-than-ever/)). From WWDC25 session 260 ([video](https://developer.apple.com/videos/play/wwdc2025/260/)): an intent appears only if every required parameter without a default is in its parameter summary and it is not `isDiscoverable=false` or `assistantOnly`; suggestions from `suggestedEntities`, `allEntities` or `PredictableIntent`; a background intent can return an "Opens Intent"; personal automations such as folder triggers.
- **Raycast** ([Quick AI](https://manual.raycast.com/ai/quick-ai), [AI Extensions](https://manual.raycast.com/ai/ai-extensions), [Agents](https://manual.raycast.com/ai/agents)): `Tab` from root search asks AI, or Quick AI as the fallback for an unmatched query; the answer streams in place; `↵` pastes into the previous app; `⌘J` moves the conversation into AI Chat; `@extension` routes to tools and chains; Agents (name, instructions, model, tool scope) each get a hotkey or alias; approval cards Allow (`↵`), Always allow (`⌘↵`), Deny (`Esc`); modes Ask, Auto, Always Allow.
- **Alfred:** the official ChatGPT workflow via keyword, Universal Action or Fallback Search; needs an OpenAI key ([Alfred Gallery](https://alfred.app/workflows/alfredapp/openai/)).
- **iOS:** App Shortcuts appear in Siri, Shortcuts and Spotlight ([WWDC26 group lab](https://developer.apple.com/videos/play/wwdc2026/8011/)); the Action button runs a shortcut ([Apple Support](https://support.apple.com/guide/iphone/use-and-customize-the-action-button-iphe89d61d66/ios)); iOS 27 shipped 2026-09-14 ([Apple Newsroom](https://www.apple.com/newsroom/2026/09/major-updates-for-apples-software-platforms-are-now-available/)), and Apple engineers said the new Siri AI needs an app schema (group lab, 3:09, 10:23), so a free-form "ask my agent" fits only `system.search` perhaps `[INFERENCE, R4]`; `supportedModes` (iOS 26+) runs an intent in the background or moves it forward later ([docs](https://developer.apple.com/documentation/appintents/appintent/supportedmodes)); `SnippetIntent` (iOS 26+) shows interactive results with buttons, usable to approve or see a result without opening the app ([docs](https://developer.apple.com/documentation/appintents/snippetintent)).
- **Patterns that make it fast** `[INFERENCE, R4]`: free text to the main routing agent by default, `@agent` to override; the answer in place, one key to a full session; the first action returns a handle at once and the result arrives later in a snippet or Live Activity; approvals as cards with Allow, Always and Deny.
- **For the program** `[INFERENCE]`: the "main routing agent" is Nixi (P1), and P9 removes "Always" at T4 and above. keeper's existing reach on the Mac (the hotkey, the tray and a `keeper://voice/talk` link, D-6) is where a launcher would sit; no story in the program map builds App Intents.

### 11.4 iOS limits for an agent client

| Mechanism | Budget / constraint | Source |
| --- | --- | --- |
| foreground | unrestricted | — |
| `beginBackgroundTask` | a "limited amount of time" to finish | [background strategies](https://developer.apple.com/documentation/backgroundtasks/choosing-background-strategies-for-your-app) |
| `BGAppRefreshTask` | up to 30 s, when the system chooses | same |
| `BGProcessingTask` | deferred to low-activity periods such as overnight charging | same |
| **`BGContinuedProcessingTask` (iOS 26)** | must start in the foreground "in response to someone's action"; progress in a cancellable Live Activity; killed when little progress shows; cancelled if the app is swiped away; network and GPU allowed | [long-running tasks](https://developer.apple.com/documentation/backgroundtasks/performing-long-running-tasks-on-ios-and-ipados), [class](https://developer.apple.com/documentation/backgroundtasks/bgcontinuedprocessingtask) |
| background push (`content-available`) | 30 s; low priority, not guaranteed; Apple says no more than 2–3 an hour; only the newest held push kept; dropped if force-quit | [background updates](https://developer.apple.com/documentation/usernotifications/pushing-background-updates-to-your-app) |
| actionable notifications | a button runs in the background without opening the app, possibly while locked, when `.complete`-protected files are unreadable | [actionable notifications](https://developer.apple.com/documentation/usernotifications/declaring-your-actionable-notification-types) |
| Live Activities | up to 8 h (12 h on the Lock Screen); 4 KB payload; no network of their own; ActivityKit push, push-to-start; an hourly budget, `NSSupportsLiveActivitiesFrequentUpdates` | [Live Activities](https://developer.apple.com/documentation/activitykit/displaying-live-data-with-live-activities), [ActivityKit push](https://developer.apple.com/documentation/activitykit/starting-and-updating-live-activities-with-activitykit-push-notifications) |
| on-device Foundation Models in the background | throttled under load; a rate-limited error | WWDC26 group lab, 13:26 |

- **So** `[INFERENCE, R4]`: nothing on iOS runs an agent loop unattended for long; the phone is a thin client that submits work to the server or the Mac and gets results by ActivityKit and alert pushes with action buttons; a Rust core can parse state and speak the client protocol but should not host the loop. P5 pins exactly this ("Phone/tablet: clients only").
- **For approvals on the phone** (story 98.1) `[INFERENCE]`: an actionable notification answers while locked, so whatever the action needs must sit outside `.complete` protection; the decision it sends is a Matrix event from the phone's verified device (ruling R12), which a push-woken extension may or may not be able to send in 30 s (§14).
- **D-1 reopened.** keeper on iPhone is a free-Personal-Team sideload today (`docs/decisions.md:241`, D-5 citing D-1); P15 assumes a paid account, so APNs through a self-hosted gateway (Sygnal, AGPL, as a separate service; §6.4) and notification actions become possible.

### 11.5 Coordinating several agents through shared files

| Prior art | Storage | Claiming | Sync |
| --- | --- | --- | --- |
| **beads `bd`** ([README](https://github.com/steveyegge/beads)) | a Dolt SQL database; `.beads/issues.jsonl` is an export, not the truth | `bd update <id> --claim` sets assignee and `in_progress` atomically, "first claim wins", re-claiming your own is idempotent; `merge-slot` is one exclusive lock per project; hash ids ([coordination.md](https://raw.githubusercontent.com/gastownhall/beads/main/docs/multi-agent/coordination.md)) | `bd dolt push/pull` to `refs/dolt/data` with cell-level merge; embedded mode one writer |
| **Claude Code agent teams** ([docs](https://code.claude.com/docs/en/agent-teams)) | one JSON mailbox per agent (`~/.claude/teams/{team}/inboxes/{agent}.json`); tasks in `~/.claude/tasks/{team}/` | "Task claiming uses file locking"; an observer saw a `.lock` with flock and one JSON per task ([ClaudeCodeCamp](https://www.claudecodecamp.com/p/claude-code-agent-teams-how-they-work-under-the-hood)) | local only; experimental |
| **Backlog.md** ([repo](https://github.com/MrLesk/Backlog.md)) | one Markdown file per task in git | no lease; "one task per agent session, one PR per task" | optional cross-branch reads |
| **Taskmaster** ([repo](https://github.com/eyaltoledano/claude-task-master)) | `.taskmaster/` | not verified | MIT with Commons Clause |
| **git-bug** ([repo](https://github.com/git-bug/git-bug)) | an operation DAG as git objects | — | `git bug push/pull`; GPLv3 |
| **CRDTs** | [Automerge](https://github.com/automerge/automerge) (its Rust API "low level and not well documented"); [Loro](https://github.com/loro-dev/loro) (Rust, JS, Swift; MIT; lists, maps, trees) | — | automatic merge |

- **Leases need fencing.** Taking a lock is a compare-and-set, which "requires consensus"; a lease is safe only with fencing tokens the storage checks ([Kleppmann](https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html)). File sync gives no compare-and-set: Syncthing resolves concurrent edits by mtime and device id and renames the loser `.sync-conflict-…` ([Syncthing](https://docs.syncthing.net/users/syncing.html)); keeper's engine keeps "your version … alongside as .sync-conflict-…" the same way (research-transcription §9.3). flock-style locking stops being safe the moment two devices share a synced folder `[INFERENCE, R4]`.
- **Conflict-free formats** `[INFERENCE, R4]`: each message a new file never edited; one append-only log per device; board state by deterministic replay (a hybrid logical clock, then device id); Loro only where ordered lists or trees need it.
- **R4's coordination recommendation:** the Linux server's main agent as the single claim authority issuing leases with a monotonically increasing fencing epoch; workers include the epoch on every write; offline devices claim only tentatively.

**How the pins apply it** `[INFERENCE]`:

| R4 | As pinned |
| --- | --- |
| one append-only log per device | ruling R3: `log/YYYY-MM-DD.<host>.<n>.jsonl`, one writer per file, the claim holder's host |
| messages one per file, never edited | P4: messages are Matrix events, not files; approvals are immutable files (P9) |
| a single claim authority with a fencing epoch | P6: the claim is a Matrix state event `{host, epoch, expires_at}` per session room; takeover writes epoch + 1 and reads it back; log lines carry the epoch; readers drop lines from a superseded epoch written after the takeover |
| offline devices claim tentatively | P6: placement prefers the always-on host; `waiting: <host>` otherwise; hesperia takes Nixi's main session only after the claim expires |
| never two devices on one file | P3 and ruling R3; the phone never writes session files (P4) |

- **What a Matrix state event is not** `[INFERENCE]`: a compare-and-set. Two hosts that both write epoch + 1 are ordered by the room's state resolution, and "read back" tells each which one won; the fence that makes a loser harmless is the epoch on every log line (§13 #10). **Ruling R28 S-05:** read-back alone is not enough — two takers inside one round trip can each read back their own write — so the taker re-reads after a settle, every line also carries the claim event it was written under, and two `acquired` lines at one epoch mark the session conflicted.

### 11.6 Android

**keeper on Android today** `[REPO]` (G5 §3): no build — no `src-tauri/gen/android` (only `gen/apple`), no Android `Platform` (`ipc.rs:460-464`), a `compile_error!` guard for non-iOS mobile targets (`spec-12-2-…:99`), no `android` script (`package.json:9-18`), desktop-only dependencies gated `cfg(not(any(target_os = "ios", target_os = "android")))` (`crates/keeper/Cargo.toml:55-69`). The plan says "macOS app first; iPhone next; Windows/iPad/Android/Linux later" (`product-inputs.md:10`); `convertMediaSrc` arrives "only when Android starts" (`ARCHITECTURE-SPINE.md:523-524`). DW-290 holds the sign-in recipe: Auth Tab (`androidx.browser` ≥ 1.9.0) through Tauri's `startActivityForResult`, a Custom Tabs bridge-activity fallback, about 100 lines of Kotlin, not tauri-plugin-web-auth (`deferred-work.md:6530-6535`). Tauri mobile is "younger than desktop" (`docs/constraints-and-limitations.md:44`).

**Tauri 2 on Android** `[SOURCE]` (R7 §6):
- tauri v2.12.1 (2026-09-30), Apache-2.0. The official notification plugin is local only; remote push comes from community plugins (e.g. spicavi/tauri-plugin-push-notifications).
- No official foreground-service plugin; the community `tauri-plugin-background-service` 1.0.1 (MIT OR Apache-2.0) wraps one and notes the "6-hour cumulative timeout" for `dataSync` ([docs.rs](https://docs.rs/crate/tauri-plugin-background-service/latest)).
- Speech through community plugins only (tauri-plugin-stt, MIT, the OS engine on mobile; tauri-plugin-tts).
- Open bugs: **#15671**, a blank webview on relaunch after a foreground service kept the process alive and the user swiped the app away (tauri 2.11.5, Android 14/15), with a fix that needs a Tauri patch ([issue](https://github.com/tauri-apps/tauri/issues/15671)); **#15506**, `requestPermissions` crashes.
- Foreground-service rules ([Android](https://developer.android.com/develop/background-work/services/fgs/changes)): Android 14 — every service declares a type and its permission; Android 15 — `dataSync` and `mediaProcessing` time-limited in the background, `BOOT_COMPLETED` cannot start some types; Android 16 — jobs from a foreground service count against normal quotas; the microphone counts only while in use; `shortService` about 3 minutes; `specialUse` needs a Play justification.

**Push and voice on Android** `[SOURCE]`: ntfy as the Matrix push gateway with UnifiedPush (§6.4); `AcousticEchoCanceler` on the `AudioRecord` (§8.4); the on-device recogniser "not intended to be used for continuous recognition", segmentable with your own PCM from API 33 (§8.6); sherpa-onnx ships no Android prebuilds (§8.5).

> Coordinator note: R7 §6 sizes Android foreground services for "a sideloaded tablet acting as an
> agent host" (`specialUse`, plus `microphone` from the foreground). P5 pins the tablet as a
> client only, so no agent-host service is needed and #15671's blank-webview bug matters only if a
> service is added for push or voice. Separately, D-5 says voice exists on the Apple platforms "and
> on neither of the others" because there it would need shipped weights (`docs/decisions.md:193-198`).
> Story 98.4 ("Android push and voice … AEC") therefore either uses the platform recogniser
> (D-5-compatible, but R5 calls continuous use brittle) or a recogniser model from `_models/` and a
> new on-device engine on Android (D-29's precedent, and a larger story). P15 does not say which.

---

## 12. The repo seams that decide the build

§12.1–§12.4 are D1's reading, §12.5–§12.10 D2's, §12.11–§12.12 D3's, §12.13 C1's; all `[REPO]` unless marked.

### 12.1 Today's call graphs (D1 §1)

**A typed chat turn with drive tools** (**[T]** = touches tauri):

| Step | Function (path:line) | Crate | Tauri? |
| --- | --- | --- | --- |
| IPC entry | `bots_chat_send` `bots_ipc.rs:1226` | keeper | **[T]** `State`, `Channel` |
| build the turn | `open_turn` `bots_ipc.rs:1239-1319` | keeper | **[T]** `Channel` |
| ↳ rows | `bot_of`:212, `provider_of`:205 → `store::get_bot/get_provider` `store.rs:615/335` | core | no |
| ↳ token | `endpoint_of`:237 → `account_ipc::bot_credential` `account_ipc.rs:332` → `resolve_token`/`resolve_credential` `bots/mod.rs:414/439` | keeper→core | the module imports tauri (`account_ipc.rs:71-72`) |
| ↳ session row | `session::get_session/insert_session` `session.rs:382/358`; `adopt_identity`:1459 → `session_caps`:280 | core + shell statics | no |
| ↳ messages | `store_message`:1646 → `session::append_message` `session.rs:652`; `replay`:1684; `attach_staged_images`:2163 | core | no |
| arm | `arm_turn` `bots_ipc.rs:1132-1213`: grants, `discovered_model`:2140, `tools::offer_tools` `tools.rs:613`, `default_profile_id` `tools.rs:653`, **`voice_ipc::spoken_turn`** `voice_ipc.rs:406` | mixed | no, but voice globals |
| ↳ drive | `arm_drive`:1086 (`cfg(desktop)`) → `bots_drive_ipc::arm_drive`:89 → `sync_profiles`:73 → `sync::engine` `sync.rs:379` → `Engine::list_profiles` `engine.rs:2711`; `context_files::context_targets/merge` `:290/334`; `bots_tools::load_context` `:495` | keeper/sync/core | no |
| spawn | `emit_context`:1887; `spawn_turn`:1705 (`tauri::async_runtime::spawn`; `LiveStream`:933 holds a tauri `JoinHandle`; `streams()`:950) | keeper | **[T]** |
| drive | `drive` `bots_ipc.rs:1742-1873`: `http::client` `http.rs:81`; `DesktopDrive::host` `bots_drive_ipc.rs:112` builds `DriveToolHost` + `approver`:175 (blocking `block_in_place`, polled every 250 ms) | keeper | **[T]** `Channel` |
| loop | `run_tool_loop_reporting` `tools.rs:1215` → `stream_chat` `chat.rs:989`, `run_one` `tools.rs:1359` | core | no |
| tool call | `DriveToolHost::run` `bots_tools.rs:117`: `grant::check` `grant.rs:805` → `audit::append_intent` `audit.rs:268` → `perform`:231 (`bots_fs::…` `bots_fs.rs:235/348/479/524/603/818`) → `write_through`:414 (`plan_write`:774, `write_unmanaged`:788, or `notes_vault::{vault,write_vault_file,touch,mark_dirty}` `notes_vault.rs:457/2408/514/3393`) → `audit::complete`:310 | keeper/core/sync | no (`notes_vault` imports `AppHandle`, `:69`) |
| sink | `channel.send(Delta…)`; flush every 512 B (`FLUSH_BYTES`:118) via `session::set_message_content` `session.rs:713`; voice hooks | keeper | **[T]** |
| close | `close`:1912 → `session::close_message` `session.rs:737` → `emit_closed`:1981 → voice completion | keeper | **[T]** |
| side doors | `bots_chat_stop`:1609; `bots_approval_answer` `bots_drive_ipc.rs:222`; `send_spoken`:1346 (a Rust-built `Channel` re-emitted as a Tauri event, `:1358-1370`); `bots_message_retry`:1494 | keeper | **[T]** |

**One `bot` task run:** `Engine::run` `engine.rs:3560` (started by `sync::start_supervisor` `sync.rs:478` with `tauri::async_runtime::spawn`:496 **[T]**; from `lib.rs:802`) → `run_due_tasks`:3740 / `run_task_now`:12621 → `perform_task`:4154 → `perform_bot_task`:5013 → `platform.bot_task_runner()`:5079 → `ShellSyncPlatform::bot_task_runner` `sync.rs:52-57` → `ShellBotTaskRunner::run` `bot_task.rs:41` → `prepare`:109 (a second copy of `arm_turn`) → `prompt_messages`:82, `task_host`:96 (`approve: None`) → `execute`:172 → `BotRunRecord`. **The task path is entirely tauri-free.**

### 12.2 The two platform ports a headless host must answer (D1 §2)

| `Platform` method (`keeper-core/src/platform.rs:25-101`) | Required? | A headless host |
| --- | --- | --- |
| `data_dir` | yes | an XDG directory, like syncd's `xdg_dir` (`keeper-syncd/src/platform.rs:168-175`); `keeper.db` holds providers, bots, grants, sessions, audit |
| `keychain_set/get/delete` | yes | keys `bot_provider_token/{p}`, `bot_token/{p}/{bot}`, Matrix `session/<id>`, the store passphrase; syncd's approach: env `KEEPER_SYNC_SECRET_*`, then a `0600` file checked by mode (`platform.rs:326-398`), wrapped in `SecretCache` (`:154`) |
| `open_url` | yes | `Unsupported`; only OIDC login uses it (`auth.rs:199`) |
| `start_web_auth` | default | falls through to `open_url` |
| `notify` | yes | log at `warn`, as syncd does (`platform.rs:400-405`) |
| `sidecar_path` | yes | `Unsupported` |
| `exclude_from_backup`, `set_badge_count` | yes | `Ok(())` |

- `Platform` has no clock and no HTTP method: clocks are `SystemTime` reads; HTTP clients are built inside core (`http.rs:81`); `account_ipc::http()` is a shell static; without an org account the credential path is `resolve_token`.
- **The shell's Linux keychain is keyutils: "a reboot ends the session"** — unfit for a daemon.
- **`SyncPlatform`** (`keeper-sync/src/platform.rs:423-614`): required `data_dir`, `secret_get/set/delete`, `notify`, `now_ms`, `free_space`, `git_program`, `host_label`; defaulted `bot_task_runner` → `None` (`:459`), `utc_offset_minutes`, `open_file_state` → `Unknown`. syncd's `LinuxPlatform` (`keeper-syncd/src/platform.rs:57-470`) provides XDG directories, env-or-`0600` secrets, a `tracing::warn` notifier, a `SystemTime` clock, `free_space = None` (fail-open), the `/proc` open-file probe, `GitRequest` resolution and a hostname label — in a **bin** crate, so the XDG and secret helpers move to `keeper_sync::xdg` (ruling R7).

### 12.3 matrix-sdk 0.18 in keeper-core, and what an agent's client needs (D1 §3)

- **Building the client:** phase A is a store-less probe that **requires MSC4186** (`auth.rs:569-596`); the persistent client is `Client::builder().homeserver_url(..).sqlite_store(&sdk_dir, passphrase).handle_refresh_tokens()` (`:625-630`), `sdk_dir = <data>/accounts/<ulid>/sdk`, the passphrase in the keychain only under the encryption posture.
- **Login:** password `login_username(..).initial_device_display_name("keeper")` (`auth.rs:101-131`); OIDC (`:141+`); Beeper JWT (`auth/beeper.rs:243`). The session JSON goes to the keychain and a row to `accounts` (`add_account`, `:576-700`). Tuwunel 1.8.1 offers `m.login.password` (`epic-85-…:54`).
- **Restore and sync:** `activate` (`account.rs:4849`) rebuilds the client, `restore_into` (`auth.rs:371`), registers the archive, redaction, draft and notify handlers (`account.rs:5044/5081/5127`, `notify.rs:560`), runs `SyncService::builder(..).with_offline_mode()` and enables the send queue.

| An agent client needs | Exposed today? | matrix-sdk 0.18 API (verified in the registry source) |
| --- | --- | --- |
| password login | `auth::login_password` (`auth.rs:528`), carrying the SSS gate, the registry row and a messenger activation | `matrix_auth().login_username` |
| the raw `Client` | **no**: `client_for` is private (`account.rs:1942`) | — |
| create a room | **missing**; only the bridge-internal `create_dm` (`bridges/mod.rs:76`) | `Client::create_room` `client/mod.rs:1797` |
| invite | **missing** | `Room::invite_user_by_id` `room/mod.rs:2023` |
| send text | `send_text` (`account.rs:2744`) needs an open UI timeline and applies the Undo-Send hold | `Room::send` / timeline |
| edit (`m.replace`) | `edit_message` (`:3187`) needs a UI item key and an open timeline | timeline edit or `send` with a replacement |
| custom timeline events | **missing** | `Room::send_raw` `room/mod.rs:2621` |
| send state | **missing** | `send_state_event`:3118, `send_state_event_raw`:3342 |
| read state | internal only (`bridge.rs:111`, `account.rs:5739`) | `get_state_event_static`:1415 |
| stream incoming events | UI view models only (`subscribe_timeline`:1232) | `Client::add_event_handler` |

- **`AccountManager` is a human-messenger supervisor**; driving agents through it would archive every message, post notifications through `Platform::notify` and need UI subscriptions to send. Hence ruling R6's lean `keeper_core::agents::matrix` (AD-6), and ruling R11's four APIs.
- **The MSC4186 gate** is satisfied by tuwunel ("sync v5 served", R7 §1), which settles D1's not-established item for the owner's homeserver; a plain `Client::sync` agent client avoids the gate entirely `[INFERENCE, D1]`.

### 12.4 The extraction: `keeper-agent` (D1 §4–§6, rulings R6–R8, R10)

**The crate:** `src-tauri/crates/keeper-agent`, depending on `keeper-core`, `keeper-sync`, `tokio`, `tracing`, `ulid`, `reqwest`; no tauri; a workspace member (`src-tauri/Cargo.toml:3`) and a dependency of the shell.

**What moves:**
- **`turn.rs`** from `bots_ipc.rs`: `Turn` (`:974`) without `spoken`/`bot_name`; `Armed`, `arm_turn`, the bodies of `open_turn` and the retry, taking a `&dyn TurnSink` and returning an `AgentError` (the shell keeps `bots_error`:149); the helpers `endpoint_of`, `read_timeout_of`, `capabilities`/`session_caps`/`cached_caps`, `adopt_identity`, `discovered_model`, `store_message`, `replay`, `attach_staged_images`, `now_ms`, `new_id`, `finish_word`. Arming takes `origin: TurnOrigin {Typed, Spoken{language}, Task}` instead of reading `voice_ipc::spoken_turn` — **this finally implements AD-224**; `Agent { session }` joins it with its first caller in 90.5 (ruling R27). The token comes from `resolve_credential(platform, http, account: Option<&AccountDescriptor>, …)`; agentd passes `None`.
- **`drive.rs`:** `drive`, `close`, `close_failed`, `emit_closed`, `emit_context`, `FLUSH_BYTES`, `spawn_turn`, `LiveStream` (a tokio `JoinHandle`), `streams`, `owns_turn`, `stop(subscription_id)`.
- **`host.rs`:** all of `bots_tools.rs` (`DriveToolHost`, `perform`, `write_through`, `load_context`, `limits`, `Approver`); `ArmedDrive`/`TurnHost` (`bots_ipc.rs:1015-1037`); `NoDrive` without its cfg; `DesktopDrive` → `DriveTurnHost`; `arm_drive(state: &AppState, grants, offered)` (`bots_drive_ipc.rs:89`), which reads the profiles inside. It also defines `UNATTENDED_REFUSAL`, the tool result an ask that reaches no person returns (ruling R29 F12).
- **`task.rs`:** all of `bot_task.rs`, with `prepare` rebuilt over `turn::arm_turn(origin: Task)` — the third caller, so it earns the abstraction — and its tests (`UnusedPlatform`, `:494`).

**The new ports, each implemented by both hosts** (a fifth, `GrantSource`, lands with its first caller in 90.5 — ruling R27):

```rust
pub trait TurnSink: Send + Sync {
    fn event(&self, e: BotStreamEvent) -> bool;   // false = receiver gone
    fn request_sent(&self) {}                      // voice note_sent
    fn ended(&self, end: TurnEnd) {}               // Complete | Stopped | Failed(String)
}
pub trait ApprovalPort: Send + Sync {
    fn ask(&self, req: BotApprovalRequestVm, signal: &CancelSignal) -> bool;
}
pub trait VaultWriter: Send + Sync {             // None ⇒ plain writer
    fn subfolder(&self, profile_id: &str) -> Option<String>;
    fn write(&self, profile_id: &str, rel: &str, text: &str) -> Result<(), String>;
}
pub trait ProfileSource: Send + Sync { fn profiles(&self) -> Vec<SyncProfile>; }
```

- **The shell implements** `ChannelSink`, `SpokenSink` (a `Segmenter` and the voice calls now inline at `bots_ipc.rs:1764-1795`, `1946-1957`), `EventSink(AppHandle)` for `send_spoken` (removing a serialise/deserialise round trip), `ChannelApprover` (`bots_drive_ipc.rs:147-209`), `NotesVaultWriter`, `EngineProfiles`.
- **agentd implements** a `MatrixSink` (`Opened` sends a placeholder; `Delta`s become throttled `m.replace` edits; `ToolResult`/`ApprovalAsked` go through `send_raw`; `Closed` is the final edit), a `MatrixApprover` (as D1 read it, a custom event and then a wait for the reply — **superseded by AD-394:** the run parks, the thread is released, and the decision resumes it), and `VaultWriter = None` (`classify` treats vault paths as unmanaged without a subfolder, `files_write.rs:653-657`; a subfolder without a handle refuses `VaultUnreachable`, `:528-531`).
- **Callers to update:** `lib.rs:27-48` (`bots_tools`, `bot_task` removed); `bots_ipc.rs` (`open_turn`, `bots_message_retry`, `send_spoken`, `bots_chat_stop`, `bots_session_follow`); `bots_drive_ipc.rs:58-60`; `sync.rs:52-57`; the shell's `Cargo.toml`; `lefthook.yml:47`; `ci.yml:90` (`cargo check --workspace --target aarch64-apple-ios` compiles every member, so agentd builds there or is excluded); `release.yml:241+`; the docs at `keeper-sync/src/platform.rs:395`, `:414`. The `#[tauri::command]` registrations (`lib.rs:949-1004`, `1553-1573`) keep their names.
- **Linux:** `cargo tree --offline --locked -p keeper-core -p keeper-sync --target x86_64-unknown-linux-gnu` shows no gtk, glib, webkit, tauri, wry, tao or openssl (only `openssl-probe`), and one `libsqlite3-sys v0.35.0` shared by `rusqlite` and `matrix-sdk-sqlite`; the shell fails on Linux only in `glib-sys` (`lefthook.yml:30-35`). So `keeper-agent` should build on Linux `[INFERENCE, D1: tree checked, not compiled]`.
- **iOS:** the shell already depends on keeper-sync on every target (`crates/keeper/Cargo.toml:26-33`, Epic 66, AD-198; re-read in this pass), so ruling R10's "keeper-agent builds on every target the shell builds" adds no new edge to the phone.

**Guards** (D1 §5): `check:agent-tauri-free` — `cargo tree -p keeper-agent -e normal,build` must not match `(tauri(-[a-z]+)*|wry|tao|gtk|glib-sys|webkit2gtk[a-z0-9-]*) v`; `check:agentd-lean` — the same plus `(^|\s)keeper v`, and since ruling R28 S-19 the OpenTelemetry and PostHog crates; both in `check` (`package.json:31`). `check:core-tauri-free` and `check:core-sync-free` are untouched (cargo forbids the reverse cycle); `check:syncd-lean` already forbids `keeper-core`, so syncd can never gain `keeper-agent`. The lefthook clippy fallback adds `-p keeper-agent -p keeper-agentd`. No Linux CI job exists (`ci.yml` Rust runs on `macos-latest`); agentd is gated on Linux by lefthook and a release build (DW-395).

**Risks** (D1 §6): a second `Engine` over syncd's database runs `db::recover_running` (`engine.rs:1925`, `db.rs:2393`) and would requeue syncd's in-flight work (ruling R7: agentd owns its own); `block_in_place` panics on a current-thread runtime (ruling R8); the server's `keeper.db` starts empty and nothing creates providers, bots or grants headlessly (ruling R8's `agentd.toml` and CLI verbs); the MSC4186 gate (§12.3); the shell half is provable only on macOS — but the move makes the drive host testable on Linux, which was the stated reason it was not (`bots_tools.rs:6-8`).

**D1's pushback and how it was ruled:**
- AD-224/AD-226 say syncd and the phone never run bot tasks and "the Mac runs the task"; if agentd ran bot tasks, both would need amending, and a task defined on two hosts' own `sync.db` could run twice. **Ruled (R9):** `TaskKind::Bot`, AD-224 and AD-226 stay as they are; scheduled agent work lives on board cards, guarded by the Matrix claim.
- AD-6 puts the agent Matrix client in keeper-core, not in `keeper-agent`. **Ruled (R6).**
- AD-40 / `sync.rs:31-35` make the shell's adapter "deliberately the only place that knows both"; `keeper-agent` becomes that seam and the shell routes through it. **Ruled (R6).**
- AD-52: agentd is never a syncd subcommand. **Ruled (R7).**
- **Order:** extract with the shell as the only consumer and no behaviour change, proven on hesperia; then the core Matrix module; then agentd with `keeper_sync::xdg` (stories 90.1, 90.4, 90.3/90.5).

### 12.5 The sessions runtime (D2 §1, ruling R4)

- **Pure plans in keeper-core** (`sessions/plan.rs`): `PlanStep` (`:29-73`), `Plan{verb,session,steps}` (`:78-87`); `compile_create_shaped` (`:152-193`) emits `MkDir active/<dir>`, the copies or placeholder-expanded `WriteFile`s, then the stamped files last; `compile_create_from_shaped` (`:260-283`) appends lineage with a `GuardedWrite`; `compile_archive` (`:404-428`) runs promote `CopyFile`s, `EmptyDirKeep workspace`, `MkDir archive/<year>`, and the `MoveDir` last; delete is one `TrashDir` (`:434`), unarchive one `MoveDir` (`:447`). Naming via `model::session_dir_name` (`model.rs:110`, a collision counter).
- **Effects in the shell:** `sessions_ipc::sessions_create` (`sessions_ipc.rs:851-1162`) reads the clock once, mints the id with `sync_ipc::new_ulid` (`:871`; `sync_ipc.rs:845`), chooses seeds, runs `sessions_exec::run` (`:1146`) and rescans; archive (`:1646-1689`) refuses unless active, takes the year from `today()`.
- **Journaling** (`sessions_exec.rs`): `run` (`:46-60`) writes `<zone>/.keeper/sessions-journal.json` (`:20`) before step 0; `run_from` (`:93-116`) rewrites `done` per step and clears at the end; a `Refused` step clears the journal; each step is idempotent on replay (`:120-298`). Containment is lexical only (`rel()`, `:328-339`); symlink escapes are not caught.
- **Two false claims in the module doc (`:4-10`):** no "`Mutex` per zone" exists — every command calls `run` through `spawn_blocking` independently, so two concurrent plans share one journal and the second `run` sees it and `resume()`s *the other plan's* steps (`:49-52`); and nothing outside `run` calls `resume`, so nothing "resumes on registry start".
- **Verbs are not idempotent:** a repeated `create` mints a new ULID and a counter-suffixed folder — a second session; verbs resolve `session_id` against a scan snapshot coalesced over 400 ms (`sessions_root.rs:47`, `:277`, `:305`), so an immediate follow-up can fail with "no such session".
- **A tauri-free crate can run them** only as a new crate on both keeper-core and keeper-sync (`check:core-sync-free`, `check:syncd-lean`): `sessions_exec.rs` whole (std, keeper_core, serde_json, tracing); from `sessions_ipc.rs` the create body, `taken_names` (`:824`), `pattern_files` (`:530`), `flat_kinds` (`:588`), `named_templates` (`:625`), `today`/`now_hhmm` (`:486`, `:3263`) and the other verbs; from `sessions_root.rs` the std-only scan (`register_one`:143, `scan_zone`:326, `row_for`:468, `read_zone_spaces`:967, `read_session_pool`:1054, `read_ref_sources`:1257, `markdown_rels`:1161, `scans_markdown`:1116). Tauri-bound and needing a port: `spawn_scanner` (`:172-205`), `start_tap` (`:208-248`), `refresh` (`:103`).
- **Desktop-gated today:** sessions ride the sync capability (`sessions_ipc.rs:20-31`; `palette.rs:58-62`); every command has a `#[cfg(not(desktop))]` `Unsupported` twin.
- **Ruling R4:** a per-zone mutex, journal resume at start-up, and a caller-supplied session id (idempotent create) — a prerequisite story (90.2) before any agent calls the verbs.

### 12.6 The board's vocabulary, and why run state is its own key (D2 §2, ruling R2)

- **Closed:** `STATUSES` = `in-preparation` / "In preparation", `todo` / "To do", `done` / "Done", `deferred` / "Deferred" (`shape.rs:356-416`, `:370`); `parse` trims, lowercases and accepts `in preparation`, `in_preparation`, `to-do`, `to do`; the key is `status` (`tasks.rs:36`).
- **A card** (`pool.rs:285-342`): kind from frontmatter and inline tags, exactly `task`; `id` a ULID or `path:<rel>`; `title` from frontmatter, first heading or stem; `status` absent → `Todo`, unparseable → `status: None, status_unreadable: true`; `order` via `read_order` (default 0.0, `order.rs:56-59`); `fields` = every frontmatter key, flattened.
- **Order:** `drop_order` takes the midpoint, ±1 at the ends (`order.rs:93-107`); when nothing fits, `compile_move` renumbers the column and writes the moved card last (`tasks.rs:94-118`); each write changes one key, byte-preserving (`:123-131`).
- **Defined twice:** Rust `STATUSES` and TS `BOARD_COLUMNS` (`task-board.tsx:114-119`, re-exported as `SESSION_BOARD_COLUMNS`); **no Rust↔TS drift guard** ("quoted rather than derived", `:108-112`).
- **An unknown status** maps to `status: null` (`sessions_root.rs:755`), lands in the "Not in a column" strays row with *"Fix the key in the file"* (`task-board.tsx:128-129`, `:373-374`); `sessions_task_move` refuses a fifth column (`sessions_ipc.rs:4060-4070`).
- **What `running`, `blocked`, `review` as statuses would break:** every such card shown as a stray "fix the key"; `task_status_parses_the_four_and_refuses_a_fifth` (`shape.rs:526`); `the_wire_spellings_are_stable` (`:541-552`); `session-board.test.tsx:180-185`; the AGENTS.md text that tells agents the closed set (`template.rs:110-112`, `migrate.rs:839-840`) and its tests; the four-column layout (`task-board.tsx:608`).
- **What new fields break:** nothing — `assignee:`, `host:`, `requested_by:` already sit in `PoolEntry.fields`, and `field:assignee=x` works in spaces today (`pool.rs:1088-1110`); showing them needs `SessionTaskVm` (`keeper-core/src/sessions/vm.rs:388-411`), ts-rs exported from keeper-core.
- **Ruling R2:** the four columns stay; `run:` (`queued | running | blocked | review | failed`) is a badge; card fields `assignee:`, `host:`, `requested_by:`, `schedule:`, `last_run:`. **Ruling R25** adds `waiting` to `run:`, written only by the owning host; ruling R28 adds the host-stamped `scheduled_by:` (S-21) and `integrity:` (S-02).

### 12.7 The folder flag for `[folder.agents]` (D2 §3, ruling R1)

- **How `[folder.sessions]` works:** a `[folder]` table is converted to JSON, `canonical_profile_fields` folds snake/camel keys, keys are checked against `FOLDER_FIELD_RULES` (`folder.rs:259-277`), merged into the profile and deserialised as a `SyncProfile`, then `validate()` runs and keys that did not take are rejected (`overlay`, `:565-699`). An empty `[folder.X]` means on with the default subfolder (`SessionsConfig`, `mod.rs:670-685`; default `"60-sessions"`, `:224`). The overlay applies when profiles are read and is stripped by `as_stored` before writes (`folder.rs:31-38`).
- **Only the app arms it:** `install_folder_tier` is called only from `keeper/src/lib.rs:423`; keeper-syncd never arms it (`folder.rs:40-42`). **An agent host must call it itself, or the agents flag stays invisible.**
- **Account round trip:** epic 84's `DriveRecord` has a role string per flag (`manifest.rs:72-81`), built in `drive_record` (`account_settings.rs:374-376`); epic 85's `drive_table` serialises the whole `SyncProfile` minus `LOCAL_ONLY` (`account_restore.rs:41`, `:114-150`), and `known_drive_keys` derives from the type (`:342-354`), so a new field round-trips with no code change.
- **The recipe** (the voices flag, AD-342, is the template): a default constant beside `mod.rs:224/:248`; a config type with `validate(notes, recordings, sessions, tasks, voices)` refusing overlap both ways (`:687-757`); `#[serde(default)]` field after `:1301` with its default fn (`:1331`) and a `*_root()` (`:1492`); validation last in `SyncProfile::validate` (`:1648-1665`); `None` in `new` (`:1384`); `("<name>", Allowed)` in `FOLDER_FIELD_RULES` (the test `folder_field_rules_cover_every_profile_field`, `:1061-1079`, fails until it is there); `sync_ipc.rs` VM and request fields, the apply arm (`:1197-1250`) and a helper (`:1294`); account settings (`DriveRecord`, `DriveOfferVm`, `state.rs:107-111`); TS form and store (`add-folder-form.tsx`, `stores/sync.ts:540-546`) and about ten `SyncProfileVm` fixtures. `SyncProfileVm.ts` and `SyncProfileReq.ts` are generated from the **shell** crate, so regenerating them needs the Mac build `[INFERENCE, D2]`. Optional: a write fence like `WriteScope::with_sessions` (`files_write.rs:408`).
- **D2 wrote the recipe for `[folder.bots]` / `80-bots`; ruling R1 renames it** `[folder.agents]` / `80-agents`, because `bots` is taken (`keeper-core/src/bots`; `[[provider.bot]]` in the device file).

### 12.8 Writes from an agent, and why the log has its own writer (D2 §4, ruling R3)

- **`drive_write` cannot create a file in a session.** `bots_fs` does no path arithmetic (`bots_fs.rs:6-16`); `plan_write` passes to `WriteScope::route` (`:774-781`), which checks `resolve_existing` first: a missing path is `Missing` — "Both writers change a file; neither creates one" (`files_write.rs:503-505`, `:518-541`). `WriteScope::create` works only inside a vault (`:464-466`), and a sessions zone can never overlap a vault (`mod.rs:740-747`). `browse::resolve` (`browse.rs:746-765`) canonicalises and catches symlink escapes.
- **Files are created only through the session verbs:** `files::compile_new` (`files.rs:671`) runs `check_rel`, which allows only `md`, `markdown`, `csv`, `json` (`:92-119`); **`.jsonl` is refused**, as are dotted directories and `workspace` (`:254-281`).
- **No append exists.** `write_unmanaged` rewrites the whole file through a temp file and a rename (`files_write.rs:880-900`) with a byte cap (`bots_fs.rs:788-806`); `GuardedWrite` (`plan.rs:36-44`) is the only optimistic-concurrency tool.
- **The workspace fence** (`files_write.rs:599-633`) covers `active/<s>/workspace/**` and `archive/<y>/<s>/workspace/**`, checked first in `classify` (`:641-646`), only for scopes built `.with_sessions` (as `bots_tools.rs:428-433` does). A process writing straight to the filesystem is not fenced; `exclude.rs` has no workspace rule; tgdrive's own `.gitignore:104-105` keeps workspaces unsynced (§2.5).
- **D2's pushback:** JSONL inside sessions is a poor fit (refused extension, no append, a commit per append); one markdown file per sitting, or high-frequency logs under `workspace/` or `.keeper/` with promoted summaries.
- **Ruling R3's answer:** `log/YYYY-MM-DD.<host>.<n>.jsonl` written **only** by `keeper-agent`'s session writer (O_APPEND, fsync at turn end), never through `drive_write`; chunks rotated before `min(192 KiB, ¾ × the profile's LFS threshold)` so a chunk never becomes an LFS object — tgdrive's threshold is 256 KiB, so 192 KiB on both counts (`/workspace/tgdrive/README.md:31-33`) `[REPO]`; a line body over 16 KiB stored once in `log/blobs/<sha256>.json`; the host's own torn tail truncated on open; one writer per file.
- **Two hosts writing one session produce conflict copies** (`sessions_root.rs:601`), hence one writer per file.

### 12.9 The doorbell, and the quiet-folder pull gap (D2 §5, ruling R5)

- **`wake_now` is not a doorbell.** `Engine::wake_now` (`engine.rs:13966-13985`) clears `next_scan_ms` and `next_remote_poll_ms`, then `note_watch_wake` **widens the walk to the whole index** (`:6659-6664`). keeper-syncd has no control channel (`sync [profile] --once`; SIGTERM/SIGINT only).
- **The doorbell:** `Engine::pull_now(id)` removes the `next_remote_poll_ms` entry and calls `db::enqueue_unique(Pull)` (`db.rs:2144`); `do_pull` skips its pre-fetch commit when `!local_work_may_be_pending` (`engine.rs:9128-9139`): one fetch, no walk. Across processes a Pull row in a shared `sync.db` would be drained by `claim_ready` (`db.rs:2209`), but whether two engines may share one db outside task leases is `[UNVERIFIED]` — and ruling R7 avoids it.
- **The root cause of the gap:** `tick_profile` calls `scan_due` (`:5526`); "paced" needs `scan_is_due`, re-armed at `max(poll, LIVE_WATCH_BACKSTOP_MS = 3 600 000)` while the watcher is live (`:615`, `:5671-5673`); `drain` calls `scan_and_enqueue` only when nothing is claimed and the scan flag is set (`:7852-7857`); and `remote_poll_due` is evaluated **only** inside `scan_and_enqueue` (`:16815`). In a quiet folder with a live watcher, the 5-minute `REMOTE_POLL_MS` (`:570`) is never checked, and the folder can wait up to an hour. The existing tests miss it: `a_pull_is_queued_once_per_remote_poll_when_idle…` (`:36412-36466`) calls `scan_and_enqueue` directly, and the backstop test uses `PushOnly` ("No remote leg", `:36482`).
- **The fix:** pull `:16815-16828` out into `queue_remote_poll(profile, now, reason)`; in `tick_profile`, after `scan_due`, `if !scan && profile.direction.pulls() && self.remote_poll_due(profile, now) && self.sync_poll_permits(profile)` → queue `"paced"`; leave the push-owed and wake branches; update `docs/sync.md:3478` and `:3490-3495`.
- **The pinning test** (ruling R5): `a_quiet_live_watcher_folder_asks_the_remote_every_remote_poll` — a bidirectional `committed_fixture`, `tick_profile` every 15 s for 11 minutes, at least two pulls after first sight counted by a new `EngineCounters.remote_polls`, and a `status_walks` delta of 0; it must fail before the fix.
- **What it buys** `[INFERENCE]`: the doorbell makes a peer's change visible in the time of one fetch after a `dev.keeper.agent.doorbell` event arrives (P4), instead of up to 5 min or 1 h (§2.6) — the "make sure its fast" of rounds 1 and 2 for the drive leg.

### 12.10 Adding a `TaskKind`, and why scheduled agent work is on cards (D2 §6, ruling R9)

Adding a kind touches: the variant, `as_str`, `from_stored` and the vocabulary tests (`tasks.rs:175-308`, `:1839-1877`); TS `TASK_KINDS` (`stores/sync.ts:89`) or `NEVER_OFFERED` (`tasks.rs:2230`), under the drift guard (`:2085-2290`), and per-kind fields (`task-form.tsx:1008-1115`); `perform_task`'s exhaustive match (`engine.rs:4170-4195`), with a port for anything needing keeper-core (`platform.rs:403-406`, `:459-461`); `db.rs` columns (`ensure_task_columns`, `:502-565`, nullable or defaulted), the select list, `TaskRow`, the read path, the required-target check, upsert and the column test; keeper-syncd's `TaskKindArg` (`commands.rs:933-1018`) and its one-way `From` (`:1020-1031`, so the compiler does *not* force a new arg, contrary to `:8511-8515`), the per-kind output (`:3574`, `:3710`) and the doc test requiring a §14 row (`docs/sync.md:2144-2149`); the shell's `sync_ipc.rs:2778` and `account_restore.rs:46-51`.

- **Why not a kind** `[INFERENCE]` over D1 and D2: task rows live in each host's own `sync.db` (G2 §2), so a workflow task defined on two agent hosts could run twice (D1's pushback), and the runner port defaults to `None` everywhere but the desktop shell. **Ruling R9:** a card's `schedule:` in keeper's dialect (keeper-sync's pure parser), an optional `host:` pin, `assignee:`; agent hosts evaluate due cards on their own tick (AD-62); the Matrix claim prevents a double run; `TaskKind::Bot`, AD-224 and AD-226 stay as they are.

### 12.11 Every provider-kind site (D3 inventory A, P10)

**Compile-forced (exhaustive `match`):** `as_registry_str` (`bots/mod.rs:82-87`, also the `bot_providers.kind` column), `from_registry_str` (`:97-103`, `None` for unknown), `quirks` (`quirks.rs:208-232`, the only place chat behaviour differs; tests `:244-294`), `health_route` (`discover.rs:116-121`; OpenAi health via `GET /v1/models`), `models` (`:198-201`; the `/v1/models` machinery exists on the Hermes side, the roster parse differs: OpenAI `data[]`), `probe_bot` (`:220-224`), `enumerate_bots` (`:252-259`; for OpenAi the models are the roster), `status_sentence`'s inner match (`:722-737`), `grant_offer` (`grant.rs:543-555`; OpenAi takes Ollama's semantics, keeper runs the tools), `commands::offered`'s `match ctx.kind` (`commands.rs:380-385`), `provider_default` (`voice_target.rs:160-165`; `None` is the honest arm for OpenAi).

**Silent (`==` or string compare; nothing forces a decision):** `bot_task.rs:134` and `bots_ipc.rs:1150` (`== ProviderKind::Hermes` suppressions; OpenAi falls through to the probe, probably right); `bot-grant-bar.tsx:149` (`provider.kind === "hermes"`; OpenAi falls through to `model.tools`, the desired outcome); `dev/mock-shell.ts:5520-5521` (refuses any kind but the two in dev, mirroring `account_ipc.rs:3559`).

**Fail-closed decodes:** `store.rs:448` (`UnknownProviderRow`), `account_ipc.rs:3559` and `account_restore.rs:721-722` ("This version of keeper cannot talk to a {} provider.").

**Kind-agnostic:** `egress.rs` (`compute_egress` takes base URLs as strings, one row per host, `:163-251`); `settings_sync.rs`, `manifest.rs:124-125`, `device_state.rs:65-66` (opaque strings; reference keys `provider:{kind}:{base}`, `:572-583`); the route constants `CHAT_PATH = "/v1/chat/completions"` (`chat.rs:42`) and `EMBEDDINGS_PATH = "/v1/embeddings"` (`embed.rs:10`), already OpenAI-shaped.

**Silent, no arm to add:** `Endpoint::new` (`mod.rs:355-358`) gives a prefix only to `(Hermes, Some(bot))`; a generic endpoint takes the `_ => None` arm.

**TypeScript:** `lib/ipc/gen/ProviderKind.ts:17` widens on regeneration; `bots-section.tsx:201` (`const KINDS = ["ollama", "hermes"]`) is the UI's kind toggle, so without a third entry TS still compiles but the UI cannot select it; `:817` defaults a new provider to `"ollama"`. Prose naming the kinds: `bot-empty-state.tsx:52`, `bot-grant-bar.tsx:18-20`, `:71-72`, `bot-paste.ts:57`, `bot-picker.tsx:10`, `:22`, `bot-session-list.tsx:165`, and the generated doc comments of several `Bot*Vm.ts`. Test fixtures with kind literals only: `bot-grant-bar.test.tsx`, `bots-pane.test.tsx`, `bots-phone-pane.test.tsx`, `bot-grants-section.test.tsx`, `bots-section.test.tsx`, `bot-composer.test.tsx`, `bot-slash-menu.test.ts`, `test/account-fixture.ts:91`; Rust discovery fixtures in `tests/bots_discover.rs`.

- **D-4's own revisit trigger is met:** "a third provider kind with a real endpoint to read against (the enum stays closed at two until then, DW-214)" (`docs/decisions.md:161-162`); ruling R13's endpoint is that endpoint. AD-146's "closed at two" becomes closed at three.

### 12.12 The `_models/` loader, and where turn-taking models run (D3 inventory B)

§8.7 has the facts. The seam, in one line each:
- **Distribution** reuses `transcription/models.rs` (`CONFIG_MODELS_DIR`, `ModelSet::from_toml`, per-role `required` lists, `choose()` refusing incomplete picks) and `account_ipc.rs:224-300`'s hydration — content-agnostic apart from the role lists.
- **Execution** is new: `ort` on macOS beside `transcribe_macos.rs` (one worker thread is a proven shape, though BNNS's serialisation constraint is Core ML's, not ONNX's) and a new iOS port module; dispatch lands at `transcribe_ipc.rs:133-149`'s `platform_engine()`.
- **The turn machine** takes a new event; its 1800 ms pause becomes the fallback.
- **The licence firewall** does not see `ort`'s prebuilt dylib (§5.10).

### 12.13 Conventions, ceilings and names (C1)

- **Ceilings** (C1 §1): epic 88 (`sprint-status.yaml:56`), AD-359, FR-766, NFR-111, UX-DR126 (all `epic-88-…:11`), DW-354 (`deferred-work.md:6988`), D-30 (`docs/decisions.md:1507`). This program allocates from epic 89, AD-360, FR-767, NFR-112, UX-DR127, DW-355, D-31 (program map). Epic 60 stays reserved and never built.
- **Formats:** an AD is `### AD-n — <one sentence>` with Binds / Prevents / Rule bullets and `> Coordinator note` blocks (ARCHITECTURE-BOTS.md:120-136); a D-entry is a title sentence, the owner's ask, a bolded refuser paragraph, What it is, Why, What it supersedes, What is deferred, Revisit triggers, Status / owner (D-27…D-30); a DW entry has `origin`, `location`, `reason`, `status`, `resolution`; FR/NFR tables are `id | statement | story | AD`; spec files use `<intent-contract>` with Problem / Approach / Always / Block If / Never.
- **Architecture companions** are found by path from epic files; the spine's `companions: []` is empty (`ARCHITECTURE-SPINE.md:22`); an `ARCHITECTURE-AGENTS.md` sits beside the others with the same frontmatter `[INFERENCE, C1]`.
- **Names:**
  - `agentd` / `keeper-agentd`: **zero hits** in the planning artefacts; its neighbour is `keeper-syncd` (AD-52; epics 30, 58, 78).
  - `80-bots`: zero hits; zones in use are `60-sessions` (AD-107), `70-comms/voices` (epic 87), `70-tasks` (AD-251's example). Ruling R1 makes it `80-agents`.
  - **`agent`** is pervasive and load-bearing in file-facing senses: epic 38 "your agent writes here too" (commits, provenance trailers AD-44, the tray's unread dot AD-63), `AGENTS.md`/`CLAUDE.md` service files (FR-570, FR-391), the "Changed by agent" filter, the session `AGENTS.md` navigation contract (AD-119/120), "an external write by Obsidian, an agent or a `git checkout`". The new concept must define itself against these.
  - **`persona`** collides softly with PostHog's "cohorts/personas" (`epic-71-…:66`, `:143`), market research's "target persona", and the branding refusal "Never an avatar, never a face, never a persona portrait"; ruling R1 keeps the word out of code (`soul`, `identity`).
  - **`SOUL.md`** appears only as Hermes' persona file (`research-ai-chat-2026-09-02.md:107`, `:250`; `epic-64-…:22`, "Hermes layers a request's system message over its profile prompt without touching `SOUL.md`"); ruling R1 states the relation: same role (slot #1 of the system prompt), read from the drive, never a Hermes profile.

---

## 13. Risk register

Each risk names its evidence, its mitigation as the pins and rulings have it, and the story that owns it (program map). A mitigation marked `[INFERENCE]` is this document's proposal, not a pin. Cite as `§13 #n`.

| # | Risk | Evidence | Mitigation | Owner |
| --- | --- | --- | --- | --- |
| 1 | **Prompt injection** through any ingress — notes, mail, web, another agent, an outside system | all 12 published defences bypassed (G3 §3); OS-Harm, OS-Blind (§7.5); Anthropic's 24 of 25 exfiltrations (§7.5) | labels with integrity, consequential calls under `untrusted` blocked or approved (P8); approvals (P9); a process per principal (P5); file content is data (AD-159); a drive's inbox, messages and recordings read `untrusted` whoever committed them (ruling R28 S-02) | 89.4, 92.6, 93.1, 99.2 |
| 2 | **Harmful intent hidden by splitting work across agents** | OS-Blind: 73% alone → 92.7% in a multi-agent system; splitting hides intent (§7.5) | delegation hop ≤ 3, ≤ 3 rounds per exchange, a token budget (P7); +1 tier for delegated actions (P7, P9); the sender's label travels with the brief (P8) | 92.1, 93.4 |
| 3 | **Memory poisoning** | MINJA, AgentPoison, Zombie Agents, ZombieAgent (§9.3); G3 lesson 4 | core memory written only by the consolidator or a human; frozen snapshot per session; no promotion from `untrusted`, cron or delegated sessions; > 25% loss rejected; owner review on shared drives (P12); on a private drive a promotion needs a contributing session with a person's input (ruling R28 S-13) | 95.1, 95.2 |
| 4 | **Skill supply chain** | ClawHavoc (G3 §3), Conscia's 341 (R1), OWASP AST02 (§9.3), Hermes' Skills Guard bypass #7072 (§4.2) | skills only from the drive's `_skills/`, written by the consolidator or a human (P2, P12); an agent's proposed skill is not offered until a person adopts it (ruling R28 S-12); no registry; hash pinning and quarantine are R6's and not pinned (§9.7) | 95.3 |
| 5 | **A leak across principals** | Giskard's shared-DM leak (G3 §3); "not a hostile multi-tenant security boundary" (OpenClaw); Grok Bot's shared computer (§9.6) | one `keeper-agentd` per principal under its own OS user; tgdrive never mounted for `agentd-neuraffica` (P5); a room may be written only when every member's audience ⊆ the label's readers (P8) | 90.3, 90.5, 92.6 |
| 6 | **Approval fatigue and social engineering** | Snyk: one confirmation was enough (G3 §9); 93% of prompts approved; auto mode passes ~17% of overeager actions (§7.5) | T0–T2 automatic within grants, T3+ per action with the exact payload, never "always" at T4+ (P9); durable rules over prompts (G3 §3); keeper writes the summary, not the model; a notification approves only what fits on it, never T4; a gate's tickets are rate-limited and coalesced (ruling R28 S-10, S-23) | 93.1 |
| 7 | **Replay or double consumption of an approval** | Agents SDK snapshots are unauthenticated; consume atomically (§7.5) | an immutable, digest-bound record; consume exactly once; re-check digest and preconditions (P9); the consumption is a Matrix event the homeserver accepts before the effect (ruling R28 S-01) | 93.2 |
| 8 | **A decision from the wrong person or device** | the model must not reach the confirmation channel (R1 pattern 10; Violoop, §7.4) | only a Matrix event from a verified device of a human in the label's readers counts (ruling R12) | 93.3 |
| 9 | **A resumed executor runs twice** | LangGraph re-runs the interrupted node (§7.5) | idempotent executors; ~~consumption recorded with the claim epoch~~ — **superseded by rulings R25 and R28 S-01:** the claim epoch is not a precondition, and the consumption is announced on the homeserver before the effect, where any host that resumes finds it | 93.2 |
| 10 | **Claim split-brain** | a Matrix state event is not a compare-and-set (§11.5); Kleppmann on fencing | epoch + 1 and read-back; every log line carries the epoch; readers drop superseded lines written after the takeover; renew 60 s, TTL 180 s (P6); a settle re-read, the claim event on every line, and a conflicted session refused rather than replayed (ruling R28 S-05) | 90.6 |
| 11 | **Two writers on one session file** | conflict copies (`sessions_root.rs:601`); the phone cannot merge (`docs/ios.md:737`) | one writer per file, host-suffixed chunks (ruling R3); the phone sends events, the owning host writes (P4) | 89.5 |
| 12 | **A log chunk becomes an LFS object** | the drive's threshold is 256 KiB (`/workspace/tgdrive/.keeper/keeper.toml:33`, `lfsThresholdBytes = 262144`) | rotate before `min(192 KiB, ¾ × threshold)`; bodies over 16 KiB in `log/blobs/` (ruling R3) | 89.5 |
| 13 | **Concurrent session plans corrupt each other** | no per-zone mutex; one shared journal; `resume()` of another plan (§12.5) | a per-zone mutex and resume at start-up (ruling R4) | 90.2 |
| 14 | **A retried create makes a second session**; a follow-up verb misses a just-created one | new ULID per create; 400 ms scan snapshot (§12.5) | a caller-supplied session id (ruling R4) | 90.2 |
| 15 | **A symlink escape through the session executor** | containment in `sessions_exec` is lexical only (§12.5) | resolve through `browse::resolve` when the runtime moves `[INFERENCE]` | 90.2 |
| 16 | **agentd requeues syncd's work** | `db::recover_running` on `Engine::open` (§12.4) | agentd owns its own Engine and `sync.db` (ruling R7) | 90.3 |
| 17 | **A blocking approval panics** | `block_in_place` on a current-thread runtime (§12.4) | a multi-threaded tokio runtime (ruling R8) | 90.5 |
| 18 | **Secrets lost at reboot on Linux** | keyutils: "a reboot ends the session" (§12.2) | env, `0600` files or systemd `LoadCredential` (ruling R8) | 90.3 |
| 19 | **A headless host starts with nothing**, and the agents flag stays invisible | empty `keeper.db` (§12.4); only the app calls `install_folder_tier` (§12.7) | `agentd.toml` and the `init | login | agents …` verbs (ruling R8); agentd arms the folder tier itself `[INFERENCE]` | 89.2, 90.3, 90.5 |
| 20 | **Streamed edits throttled**, and a smoke test that proves nothing | Synapse `rc_message` 0.2/s, burst 10; tuwunel's deployed config unread (§6.2, §6.7) | a per-user rate-limit override or an appservice registration with `rate_limited: false` on `keeper-test-synapse`; read electra's tuwunel config | 90.4, 90.5 |
| 21 | **A final answer over 64 KiB** | the event cap; Beeper uploads an attachment (§6.2) | ~~not pinned~~ — **settled by ruling R23:** the first 60 KiB plus a link to `artifacts/answer-<ulid>.md`, lowered in an encrypted room until the event fits (measured in 90.5, ruling R27) | 90.5 |
| 22 | **Matrix latency nobody has measured** | no published figures (§6.2) | measure p95 in the smoke run; P4's trigger is p95 > 1 s on tuwunel (§6.8) | 90.5 |
| 23 | **matrix-sdk drifts behind** | 0.19.1 is out; keeper pins 0.18 (§6.1) | stay at 0.18 for the program (ruling R11); an upgrade is its own decision | 90.4 |
| 24 | **The MSC4186 gate refuses an agent's login** | `add_account` requires MSC4186 (§12.3) | the lean agent client does not go through `AccountManager`; tuwunel serves sync v5 (R7 §1) | 90.4 |
| 25 | **Shell changes unproven on the dev host** | the shell fails on Linux in `glib-sys` (§12.4) | every shell change named in its PR as awaiting CI's macOS job / `check:rust:macos`; the hesperia gate (scope guards; ruling R13) | 90.1 and every epic touching the shell |
| 26 | **The iOS check breaks on a Linux-only binary** | `ci.yml:90` checks every member for iOS (§12.4) | agentd excluded from the iOS check; `keeper-agent` builds there (ruling R10) | 90.1, 90.5 |
| 27 | **A provider kind decided by accident** | four silent sites (§12.11) | story 89.6 decides each: `bot_task.rs:134`, `bots_ipc.rs:1150`, `bot-grant-bar.tsx:149`, `mock-shell.ts:5520`; adds the third toggle (`bots-section.tsx:201`) | 89.6 |
| 28 | **Tool calls mangled across protocols** | CLIProxyAPI #5836, #5581, #5805, #6208, #6218 (§11.1) | a smoke run with tool calls against ruling R13's endpoint; keeper keeps partial replies and caps arguments (§2.2) | 89.6 |
| 29 | **Terms-of-service exposure** of subscription proxies | Anthropic's terms and The Register; CLIProxyAPI's cloak mode (§11.2) | the endpoint is the owner's (D-4, P10); keeper ships, operates and recommends no proxy; before 91.5 the owner records the upstreams, the cloak setting and their acceptance of the risk in `docs/agents.md`, and `agents init` names no default bot (ruling R28 S-20) | 89.6 (its docs); the owner, before 91.5 |
| 30 | **Proxy release churn** | three releases on one day (§11.1) | the version is pinned where it runs (the operator's); keeper re-reads `/v1/models` on probe | operational |
| 31 | **The `run` sandbox is weaker than assumed** | `sandbox-exec`'s status `[UNVERIFIED]`; landlock restricts only the applying thread (§5.5) | landlock applied in the child before exec, with a seccomp filter (ruling R24(5)) that also closes ptrace and the parent's `/proc` (ruling R28 S-07); a scratch `HOME` (S-08); generated SBPL; argv-bound approval; with network, only the workspace mounted (S-03) (story 96.1); DW-213's reversal recorded (§7.9) | 96.1 |
| 32 | **A KVM is the weakest device on the network** | Eclypsium's 9 CVEs; NanoKVM-Go's self-signed TLS; the AES "encryption" (§7.3) | tailnet only, certificate pinned, power/ISO/BIOS at T4; ~~whether a KVM target raises every action one tier is open~~ — **settled by ruling R22:** it does, once, and nothing through a KVM is below T4 | 96.5 |
| 33 | **Peekaboo loses its permissions** | re-signing resets TCC grants; a Bridge is needed under SSH or a LaunchAgent (§7.2) | run it in the GUI session on hesperia; re-grant after an update | 96.4 |
| 34 | **An agent clicks "Allow" on its own permission dialog** | contested evidence (§7.2); Violoop's classification hazard (§7.4) | TCC dialogs at T4; editing its own grants at T5 (§7.6) | 93.1, 96.4 |
| 35 | **An ONNX runtime licence nobody checked** | `ort`'s prebuilt dylib is not a cargo dependency (§5.10) | a manual licence check recorded beside the model set | 97.1 |
| 36 | **The phone has no inference engine** | `AbsentEngine` everywhere but macOS (§8.7) | a new iOS port module running `ort` with the Core ML EP | 97.2 |
| 37 | **"Mhm" stops the agent, or a real interruption does not** | turn.rs's first rule; LiveKit's classifier is Cloud-only (§8.4, §8.8) | **settled by ruling R14:** pause first, then the utterance decides — a backchannel from the list for the person's language (ruling R24(10)) continues, anything else stops (§8.8) | 97.3 |
| 38 | **The log says the person heard more than they did** | WebSocket truncation has no aligned transcript (§8.2) | truncate at the played sentence; log `heard_until` (P13) | 97.3 |
| 39 | **Approving from a locked phone fails** | `.complete` files unreadable while locked; 30 s background budgets (§11.4) | the action's data outside `.complete`; the decision is a Matrix event from the phone's verified device | 98.1 |
| 40 | **No push path for keeper's bundle** | APNs credentials belong to one bundle; Sygnal is AGPL (§6.4) | a self-hosted gateway as a separate service (P15) | 98.1 |
| 41 | **Voice on Android meets D-5** | D-5: voice exists "on neither of the others" for want of weights; the platform recogniser is not for continuous use (§8.6, §11.6) | ~~not pinned~~ — **settled by ruling R20:** the platform's on-device recogniser in segmented sessions; continuous duplex is a documented limitation; without an `ort` runtime, no turn models (DW-413) | 98.4 |
| 42 | **The BMAD render port halts** on this install's config | duplicate `implementation_artifacts` / `planning_artifacts` (§10.2) | refuse with a sentence naming the ambiguous key, as `render_skill.py` does `[INFERENCE]` | 94.1 |
| 43 | **A port without a known licence** | BMAD-METHOD's licence not stated locally (G4 §7); OpenClaw's reported two ways (R1) | `UPSTREAM.md` per module records the licence; a module does not land until it is known (P14). **Settled by rulings R21 and R27:** BMAD-METHOD is MIT, pinned at v6.12.0 (`05bfbd46`); OpenClaw's gates are re-implemented from its documentation | 89.1, 94.1, 95.2 |
| 44 | **A workflow waits on a person forever** | BMAD's HALT and menus (§10.3) | `ask_human` through Nixi, parked like an approval (P11) | 94.2, 94.3 |
| 45 | **Indexing what must not be indexed** | LFS pointers read as notes; the inbox; client filenames (G3 §8, lessons 12, 17) | `drive_search` excludes pointer zones and the OKF exclusions; whether `80-agents/` joins an OKF bundle is the owner's drive configuration | 95.4, 89.2 |
| 46 | **An over-scoped credential found in a file** | PocketOS (§7.5) | no credentials in drives or sessions; the keychain port (AD-147); credentials are T4; secret-shaped text is redacted before a session log line is written, and a session folder is as sensitive as the drives it reads (ruling R28 S-17; D-31) | 89.3, 89.5, 96.1 |
| 47 | **Exfiltration by a link** | zero-click link previews (G3 §3, lesson 9) | labels decide which rooms an agent may post to; a recipient, path or target derived from `untrusted` data is blocked (pinned in AD-391; epic 92 reads "derived" as "not verbatim in a trusted line", DW-378) | 92.6 |
| 48 | **A sensitive agent's local model is too small** | the Ollama LXC pins 16 384 tokens; Hermes refuses agent use below 64k (§3.1) | per-agent limits in `agent.toml`; the context budget checked when a turn is armed `[INFERENCE]` | 89.3 |
| 49 | **Dr Lucyna Novak cannot write neuradrive** | the server checkout is pull-only since 2026-09-09 (G5 §6) | agentd-neuraffica's own checkout (ruling R7) pushes. **Settled by ruling R17:** switching the existing server checkout from pull-only is an operator action outside this repository | 90.3, 91.5 |
| 50 | **Epic 22's refusals return as incidents** | "nie podpinaj dysku"; Q1, Q2 (G3 §1, §3) | the mechanisms of §3.10: OS-level split, grants, labels, consolidator-only memory; the refusals stay makistack's record for Hermes (ruling R16) | 89.4, 92.6, 95.1 |
| 51 | **Two spellings of the main agent** | "nixi" (owner), `nixie` (Hermes profile, wake phrase) (§1.6 item 1) | **settled by ruling R19:** `@nixi`, `80-agents/nixi/`, "Nixi"; the wake phrase stays the person's own setting | 91.4, 91.5 |
| 52 | **AD-224's documentation keeps lying** | `TurnOrigin` absent; `platform.rs:395`'s "its own `open_turn`" (§2.12) | `TurnOrigin` implemented by the extraction; the docs corrected (D1 §4) | 90.1 |
| 53 | **Board columns drift between Rust and TypeScript** | no drift guard (§12.6) | the `run:` badge added on both sides with a guard `[INFERENCE]` | 92.2 |
| 54 | **"Talk to my main agent" names no agent on another device** | `bots.voice_target` is `UserGlobal`; bot ids are re-minted per device (§2.9) | the voice target is the agent's room, whose Matrix id is the same everywhere `[INFERENCE]` | 91.4 |
| 55 | **An MCP config executes code** | Codex CVE-2025-61260 (§4.2); `transport-child-process` impossible on iOS (§5.3) | MCP servers only from the host's own configuration (`agentd.toml` `[[mcp]]`, the desktop's device-local Settings), written by a person, never from a drive, session or workspace file (ruling R24(4)); a server keeper starts as a command is never below T2 (ruling R28 S-14); desktop and server only | 96.2 |
| 56 | **A cheap model with tools under untrusted input** | G3 lesson 5 | `agent.toml` pins the model; `untrusted` integrity blocks consequential calls (P8) | 89.3, 92.6 |
| 57 | **Agents loop with each other** | MSC4295's bot bounce limit exists for this (§6.3); story 22-8's 3-round cap (§3.4) | hop ≤ 3, ≤ 3 rounds per exchange, a token budget per delegation (P7) | 92.1 |
| 58 | **Room history bloats with edits** | every edit is a stored event; ~300 per two-minute answer (§6.2) | the ≥ 400 ms debounce and one final edit (P4); nothing further pinned | 90.5 |
| 59 | **Every streamed edit buzzes the phone** | push rules run on room events (§6.4); OpenClaw's quiet mode (§6.3) | push rules that notify on the final edit only `[INFERENCE]` | 98.1 |

---

## 14. Not established (`[UNVERIFIED]` inventory)

Every unverified item from every digest, with what was tried and where it gets settled. "None needed" means no pinned decision rests on it.

| Claim | Digest | What was tried | Where it gets settled |
| --- | --- | --- | --- |
| How Grok Bot stores sessions; whether it branches or archives | R1 | xAI's docs read; not documented | none needed |
| The design of Grok companions (Ani) | R1 | only a non-xAI page (grokani.org) | none needed |
| Codex CLI's built-in todo or plan tool; whether the CLI itself schedules jobs | R1 | Codex docs read; automations are a desktop-app feature | none needed |
| What Codex's `agent-message-board-client` talks to | R1 | crate seen; an HTTP/SSE client `[INFERENCE: a remote board]` | none needed |
| Whether dsh has messaging gateways | R1 | docs read | none needed |
| Native scheduling in pi and omp | R1 | docs read | none needed |
| How goose's memory extension works | R1 | — | none needed |
| A CVSS score for CVE-2026-25253 | R1 | R1 found none; G3's makistack hardening research records CVSS 8.8 — two lanes disagree | none needed |
| Whether any harness but LangGraph keeps a pending approval across a restart | R1 | the approval docs of all ten products | 93.2 builds it |
| OpenClaw's licence (README badge MIT; GitHub API "Other") | R1 | GitHub API and README | settled by ruling R21: re-implemented from its documentation, which `keeper-ported::openclaw`'s `UPSTREAM.md` says (95.2) |
| deepagents' cron; Codex's board | R1 | — | none needed |
| iOS builds of rig, genai, async-openai, openai-api-rs, candle, mistral.rs, lancedb, tantivy, arroy, hnsw_rs, model2vec-rs, autoagents, adk-rust, llm | R2 | nothing compiled | none needed (not taken) |
| `ort` on iOS with the Core ML EP | R2 | `ort-sys/build/download/dist.tsv` lists `aarch64-apple-ios`; not compiled | 97.2 |
| Whether `sandbox-exec` is formally deprecated, and whether it works inside an App Store-sandboxed app | R2 | not found | 96.1 on hesperia |
| Whether goose's built-in extensions run in process or as MCP subprocesses | R2 | — | none needed |
| Wayland reliability of enigo and xcap | R2 | — | none needed |
| Whether ACP v2's session resume stabilises | R2, R7 | drafts read | none needed |
| Whether duroxide leaves preview | R2 | — | none needed |
| NanoKVM-Go's firmware source, MCP tool schema, and any REST or WebSocket API beyond MCP | R3 | the repository contains only a LICENSE file | 96.5, against a device |
| Whether JetKVM's JSON-RPC runs over WebRTC; whether Comet's API matches PiKVM's | R3 | `jsonrpc.go` read; Comet inferred from its derivation | 96.5, if targeted |
| Violoop's external API, log export and chip firmware | R3 | Q&A and reviews read | none needed |
| System-wide MCP in macOS 27 | R3 | the cited WWDC26 session covers only `LanguageModelExecutor` | none needed |
| Whether synthetic input can approve TCC dialogs (contested) | R3 | MindStudio against HackTricks | 96.4 treats it as possible (T4) |
| Whether UI-TARS-2's weights are released | R3 | — | none needed |
| Primary sources for the July 2026 Hugging Face and UK AISI agent incidents | R3 | only search snippets | none needed |
| Why Qwen and Gemini CLI were removed from CLIProxyAPI | R4 | the commit messages give no reason | none needed |
| Google's terms on Antigravity or Vertex OAuth through a proxy | R4 | not found | the owner |
| A primary OpenAI source on Codex OAuth in third-party proxies | R4 | only a secondary report | the owner |
| Whether Anthropic detects CLIProxyAPI's cloaking, and ban rates | R4 | — | the owner, who records the cloak setting and accepts the risk before 91.5 (ruling R28 S-20) |
| Exact budgets for `beginBackgroundTask`, App Intent execution and `BGContinuedProcessingTask`; the Live Activity push budget | R4 | Apple's docs give no numbers | 98.1, on the device |
| Whether iOS 27 or macOS 27 changed background tasks or Spotlight actions | R4 | searched; nothing found | 98.1 |
| The protocol behind Raycast's local-subscription feature | R4 | — | none needed |
| Taskmaster's storage and claim semantics; how beads resolves two offline claims | R4 | — | none needed |
| Whether a current Alfred 6 has its own AI features | R4 | — | none needed |
| Official time-to-first-audio for the hosted APIs; pricing for Gemini Live, xAI and Nova | R5 | — | none needed (not taken) |
| GPT-Live's session limits, and LiteLLM support for it | R5 | — | none needed |
| Whether sherpa-onnx's static libraries link espeak-ng | R5 | — | 98.4, if sherpa-onnx is used |
| PersonaPlex's VRAM | R5 | a third-party guide only | none needed |
| Whether Android's on-device recognition keeps working while TTS plays | R5 | — | 98.4 |
| Moonshine's code licence, as opposed to its models' | R5 | the changelog read | 98.4, if used |
| Whether any open full-duplex model runs on a phone | R5 | only Kyutai STT on MLX shown | none needed |
| The latency budget's VAD hangover, LLM first token, Kokoro/Pocket first audio and model response | R5 | estimates | 97.2 measures end of turn on the device |
| On-device TTS quality below Kokoro or Pocket TTS | R5 | — | none needed |
| A Rust or ONNX path for Nemotron streaming STT | R5 | — | none needed |
| ClawHub's malicious-skill count: 341 (Conscia, R1), 824 and 1 100+ (R6's secondary sources), 1 184 (G3's makistack research) | R6, R1, G3 | the VirusTotal post did not load | none needed |
| Claude Code's licence | R6 | — | none needed |
| rusqlite's licence | R6 | not checked; keeper already links rusqlite under cargo-deny (§12.4) `[INFERENCE]` | none needed |
| Whether Hermes ever evaluates a skill by outcome | R6 | docs and source: usage counters only | none needed |
| Any published IFC scheme for durable inter-agent sessions | R6 | no paper found; the delegation rule is R6's inference | 92.6 |
| The size of Hermes' curator's deterministic part (~1 KLOC) | R6 | — | 95.3 |
| The cut table rows of the R6 digest file (Hermes' docs/memory URL, the review prompt's exclusion list, the staged-skill viewer, the curator's scope, OpenClaw's change log) | R6 | the digest file stops those rows at 768 characters | 95.1–95.3 re-read upstream |
| Measured Matrix delivery latency on a self-hosted homeserver, sliding sync included | R7 | none published; tuwunel #358 was an ISP | 90.5 (P4's trigger) |
| Push latency through Sygnal, APNs, FCM or ntfy | R7 | none published | 98.1, 98.4 |
| Any Matrix.org Foundation or Element statement on AI agents for 2025–2026 | R7 | only ara4n's MSC4471 comment | none needed |
| The Synapse version in which native Simplified Sliding Sync became the default | R7 | — | none needed |
| Whether Synapse rate-limits `/sendToDevice` separately | R7 | — | none needed (no to-device, ruling R11) |
| Licences of the Beeper repositories, ruma, mautrix-go and matrix-hookshot | R7 | the GitHub API rate-limited the checks | none needed (none linked) |
| Whether Conduit rate-limits message sends | R7 | — | none needed |
| Android foreground-service type names beyond those read | R7 | — | 98.3 |
| The maturity of the community Tauri push plugins | R7 | — | 98.4 |
| How ACP v2 and MCP-over-ACP will stabilise | R7 | drafts read | none needed |
| The rate limits in electra's deployed tuwunel configuration | this pass (§6.7) | R7 read the example configuration | 90.5 |
| Whether CLIProxyAPI serves `/api/tags` and `/api/version` (were it saved as `ollama`) | G1 | not checked | moot: P10 adds a kind (§12.11) |
| Whether tasks sync across devices | G1 | device restore lists drives, providers, bots, grants and Matrix accounts only | moot for agents: scheduled agent work is on cards in the drive (ruling R9) |
| CLIProxyAPI's routes, against its source | G1 | — | settled by R4 for `GET /v1/models` and `POST /v1/chat/completions` (§11.1) |
| `bmad-prd`, `bmad-spec`, `step-oneshot.md`, `compile-epic-context.md` | G4 | not read | 94.2 |
| The `bmad-loop` orchestrator's source | G4 | not on disk | none needed |
| BMAD-METHOD's own licence | G4 | only the vendored module template's MIT | settled by ruling R27: MIT, v6.12.0 at `05bfbd46`, with its trademark notice; recorded in 89.1's `bmad/UPSTREAM.md` |
| The render CLI's behaviour end to end | G4 | not run (it writes `_bmad/render`); `_resolve_replacements` run in process | 94.1 |
| Tuwunel supports MSC4186 | D1 | — | settled by R7 §1: "sync v5 served" `[SOURCE]` |
| `tokio::spawn` straight from inside tauri commands | D1 | expected yes (tauri's runtime is tokio) | settles in 90.1, on hesperia |
| What profiles look like after the folder-tier merge when read from the database | D1 | — | moot (ruling R7); agentd arms the tier itself (§12.7) |
| `keeper-agent` builds on Linux | D1 | `cargo tree` checked; not compiled | settles in 90.1 (lefthook clippy) |
| Whether `committed_fixture` has a reachable remote | D2 | — | 92.4's pinning test |
| Whether the drive's `.gitignore` excludes `workspace/` | D2 | settled for tgdrive in this pass (`.gitignore:104-105`); neuradrive not read | 91.5 |
| The TS generation pipeline for shell-crate types | D2 | — | 89.2, on the Mac |
| Whether two engines may share one `sync.db` outside task leases | D2 | — | moot (ruling R7) |
| The licence of `ort`'s prebuilt onnxruntime dylib | D3 | not a cargo dependency, outside cargo-deny | 97.1 |
| How CLIProxyAPI is deployed on electra | G5, this pass | makistack has zero hits | operational |
| Whether a push-woken iOS notification action can send a Matrix event from the phone's verified device within its budget | this pass (§11.4) | — | 98.1 |
| How a room orders two concurrent `dev.keeper.agent.claim` writes from two devices of one agent | this pass (§11.5) | — | 90.6; ruling R28 S-05 adds a settle re-read and the claim event on every line, so two winners are caught rather than ordered |

---

## 15. Sources

**External, read on 2026-10-01 by the research lanes** (grouped by digest; a URL cited by two lanes is listed under both):

*R1 — agent products and harnesses*
- xAI, Grok Bot — <https://docs.x.ai/grok-bot/overview>, <https://x.ai/news/introducing-grok-bot>, <https://docs.x.ai/grok-bot/chat-and-collaboration.md>, <https://docs.x.ai/grok-bot/skills-routines-and-automations.md>, <https://docs.x.ai/grok-bot/approvals-security-and-privacy>
- xAI, Grok Build — <https://github.com/xai-org/grok-build>, <https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-pager/docs/user-guide/17-sessions.md>, <https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-pager/docs/user-guide/16-subagents.md>, <https://raw.githubusercontent.com/xai-org/grok-build/main/crates/codegen/xai-grok-pager/docs/user-guide/22-permissions-and-safety.md>
- superagent-ai, grok-cli — <https://github.com/superagent-ai/grok-cli/blob/main/README.md>; grokani.org — <https://grokani.org/>
- DeepSeek, DeepSeek Harness — <https://github.com/deepseek-ai/deepseek-harness>, <https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/docs/subsystems/persistence.md>, <https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/docs/architecture.md>, <https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/docs/subsystems/agent-team.md>, <https://github.com/deepseek-ai/deepseek-harness/tree/master/docs/subsystems>, <https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/docs/subsystems/approval.md>, <https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/master/SAFETY.md>; Composio — <https://composio.dev/content/deepseek-harness-vd-pi-agent>
- DeepSeek-TUI — <https://lib.rs/crates/deepseek-tui-cli>, <https://www.developersdigest.tech/tools/deepseek-tui>
- Nous Research, Hermes Agent — <https://github.com/NousResearch/hermes-agent>, <https://hermes-agent.nousresearch.com/docs/user-guide/sessions>, <https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/profiles.md>, <https://hermes-agent.nousresearch.com/docs/user-guide/features/delegation>, <https://hermes-agent.nousresearch.com/docs/user-guide/features/kanban>, <https://hermes-agent.nousresearch.com/docs/user-guide/features/cron>, <https://hermes-agent.nousresearch.com/docs/user-guide/security>, <https://github.com/NousResearch/hermes-agent/issues/7072>
- OpenClaw — <https://github.com/openclaw/openclaw>, <https://docs.openclaw.ai/concepts/session>, <https://docs.openclaw.ai/concepts/multi-agent>, <https://docs.openclaw.ai/tools/subagents>, <https://docs.openclaw.ai/plugins/workboard>, <https://docs.openclaw.ai/concepts/architecture>, <https://docs.openclaw.ai/automation/cron-jobs>, <https://docs.openclaw.ai/concepts/memory>, <https://docs.openclaw.ai/tools/exec-approvals>
- OpenClaw incidents — ProArch <https://www.proarch.com/blog/threats-vulnerabilities/openclaw-rce-vulnerability-cve-2026-25253>; Oasis <https://www.oasis.security/blog/openclaw-vulnerability>; Conscia <https://conscia.com/blog/the-openclaw-security-crisis/>; AdminByRequest <https://www.adminbyrequest.com/en/blogs/openclaw-went-from-viral-ai-agent-to-security-crisis-in-just-three-weeks>
- earendil-works, pi — <https://github.com/earendil-works/pi>, <https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/docs/session-format.md>, <https://raw.githubusercontent.com/earendil-works/pi/main/packages/coding-agent/docs/sessions.md>, <https://raw.githubusercontent.com/earendil-works/pi/main/packages/durable/README.md>
- oh-my-pi — <https://github.com/can1357/oh-my-pi>, <https://raw.githubusercontent.com/can1357/oh-my-pi/main/docs/session.md>, <https://raw.githubusercontent.com/can1357/oh-my-pi/main/docs/approval-mode.md>
- LangChain — <https://github.com/langchain-ai/deepagents>, <https://docs.langchain.com/oss/python/langgraph/persistence>, <https://docs.langchain.com/oss/python/deepagents/overview>, <https://docs.langchain.com/oss/python/deepagents/human-in-the-loop>, <https://docs.langchain.com/oss/python/langgraph/interrupts>
- OpenAI, Codex — <https://raw.githubusercontent.com/openai/codex/main/codex-rs/Cargo.toml>, <https://github.com/openai/codex/tree/main/codex-rs/thread-store>, <https://learn.chatgpt.com/docs/app-server.md>, <https://learn.chatgpt.com/docs/agent-configuration/subagents>, <https://learn.chatgpt.com/docs/sandboxing/auto-review.md>, <https://github.com/openai/codex/tree/main/codex-rs/execpolicy>, <https://learn.chatgpt.com/docs/customization/memories>, <https://developers.openai.com/codex/app/automations>; Check Point — <https://research.checkpoint.com/2025/openai-codex-cli-command-injection-vulnerability/>
- goose — <https://github.com/aaif-goose/goose>, <https://goose-docs.ai/docs/guides/logs>, <https://goose-docs.ai/docs/guides/sessions/session-management/>, <https://goose-docs.ai/docs/guides/goose-cli-commands/>, <https://goose-docs.ai/docs/guides/goose-permissions/>, <https://github.com/aaif-goose/goose/tree/main/crates/goose-roaming>; Block — <https://engineering.block.xyz/blog/how-we-red-teamed-our-own-ai-agent->
- Violoop — <https://violoop.ai/blog/best-ai-agents-for-computer-automation-2026/>; MindStudio — <https://www.mindstudio.ai/blog/violoop-ai-hardware-agent-mac>

*R2 — Rust building blocks* (plus the crates.io API, `https://crates.io/api/v1/crates/<name>`, and the GitHub API, `https://api.github.com/repos/<owner>/<repo>`, for every crate)
- <https://github.com/0xPlaygrounds/rig>, <https://github.com/bosun-ai/swiftide>, <https://github.com/zavora-ai/adk-rust>, <https://github.com/jeremychone/rust-genai>, <https://github.com/64bit/async-openai>, <https://github.com/openai/codex>
- <https://github.com/modelcontextprotocol/rust-sdk>, <https://github.com/agentclientprotocol/rust-sdk>, <https://github.com/a2aproject/a2a-rs>, <https://github.com/ag-ui-protocol/ag-ui>
- Stack Overflow — <https://stackoverflow.com/questions/36531728>, <https://stackoverflow.com/questions/15893849>
- <https://github.com/microsoft/duroxide>

*R3 — computer use, KVMs, approvals*
- Anthropic — <https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool>; OpenAI — <https://developers.openai.com/api/docs/guides/tools-computer-use>, <https://developers.openai.com/api/docs/guides/tools-computer-use-integration.md>; Google — <https://ai.google.dev/gemini-api/docs/computer-use>; ByteDance — <https://github.com/bytedance/UI-TARS>; a16z — <https://a16z.com/can-agents-use-a-computer-yet-weve-got-the-data/>
- HackTricks — <https://hacktricks.wiki/en/macos-hardening/macos-security-and-privilege-escalation/macos-security-protections/macos-input-monitoring-screen-capture-accessibility.html>; Peekaboo — <https://github.com/steipete/Peekaboo>, <https://raw.githubusercontent.com/openclaw/Peekaboo/main/docs/permissions.md>; MindStudio — <https://www.mindstudio.ai/blog/violoop-ai-hardware-agent-mac>; 9to5Mac — <https://9to5mac.com/2025/09/22/macos-tahoe-26-1-beta-1-mcp-integration/>; Apple — <https://developer.apple.com/videos/play/wwdc2026/339/>
- freedesktop — <https://raw.githubusercontent.com/flatpak/xdg-desktop-portal/main/data/org.freedesktop.portal.RemoteDesktop.xml>; <https://crates.io/crates/atspi>; ydotool — <https://github.com/ReimuNotMoe/ydotool>; Apple Platform Security — <https://support.apple.com/guide/security/security-of-runtime-process-sec15bfe098e/web>
- Sipeed — <https://wiki.sipeed.com/hardware/en/kvm/NanoKVM_Go/introduction.html>, <https://wiki.sipeed.com/hardware/en/kvm/NanoKVM_Go/mcp.html>, <https://github.com/sipeed/NanoKVM-Go>, <https://github.com/sipeed/NanoKVM>, <https://raw.githubusercontent.com/sipeed/NanoKVM/main/kvmapp/picoclaw/skills/kvm-control/SKILL.md>; CNX — <https://www.cnx-software.com/2026/07/01/sipeed-nanokvm-go-an-4k-usb-c-kvm-with-recall-like-function-ai-integration/>; community API reference — <https://raw.githubusercontent.com/scgreenhalgh/nanokvm-mcp/main/API_REFERENCE.md>
- PiKVM — <https://docs.pikvm.org/api/>; JetKVM — <https://github.com/jetkvm/kvm>; GL.iNet — <https://github.com/gl-inet/glkvm>; Eclypsium — <https://eclypsium.com/blog/your-kvm-is-the-weak-link-how-30-dollar-devices-can-own-your-entire-network/>
- Violoop — <https://violoop.ai/qa/>
- LangGraph — <https://docs.langchain.com/oss/python/langgraph/interrupts>; OpenAI Agents SDK — <https://openai.github.io/openai-agents-python/human_in_the_loop/>; Codex — <https://learn.chatgpt.com/docs/agent-approvals-security>; Claude Code — <https://code.claude.com/docs/en/permission-modes>, <https://code.claude.com/docs/en/hooks.md>; Anthropic — <https://www.anthropic.com/engineering/how-we-contain-claude>; Nango — <https://nango.dev/blog/mcp-elicitation-explained>; OpenClaw — <https://docs.openclaw.ai/tools/exec-approvals>, <https://docs.openclaw.ai/tools/exec-approvals-advanced>; Hermes — <https://hermes-agent.nousresearch.com/docs/user-guide/security>
- OS-Harm — <https://arxiv.org/abs/2506.14866>; OS-Blind — <https://limelab.science/OS_Blind/>; Zenity — <https://zenity.io/blog/ai-agent-database-deletion-pocketos>

*R4 — proxy, launcher, devices, coordination*
- router-for-me, CLIProxyAPI — <https://github.com/router-for-me/CLIProxyAPI>, <https://github.com/router-for-me/CLIProxyAPI/releases/tag/v8.0.7>, <https://raw.githubusercontent.com/router-for-me/CLIProxyAPI/main/config.example.yaml>, <https://github.com/router-for-me/CLIProxyAPI/commit/8fac29631db5cbcd69f396592f4718e165464724>, <https://github.com/router-for-me/CLIProxyAPI/commit/78ba8ba731dd531437947a6e0aadda4c13817907>, <https://github.com/router-for-me/CLIProxyAPI/issues/5836>, <https://github.com/router-for-me/CLIProxyAPI/issues/5581>, <https://github.com/router-for-me/CLIProxyAPI/issues/5805>, <https://github.com/router-for-me/CLIProxyAPI/issues/6208>, <https://github.com/router-for-me/CLIProxyAPI/issues/6218>
- similar proxies — <https://github.com/Wei-Shaw/sub2api>, <https://github.com/diegosouzapw/OmniRoute>, <https://github.com/decolua/9router>, <https://github.com/ericc-ch/copilot-api>
- terms — Anthropic <https://code.claude.com/docs/en/legal-and-compliance>; The Register <https://www.theregister.com/software/2026/02/20/anthropic-clarifies-ban-on-third-party-tool-access-to-claude/5014546>; explainx.ai <https://explainx.ai/blog/codex-usage-limits-sub2api-sign-in-chatgpt-august-2026>; Raycast <https://manual.raycast.com/ai/bring-your-own-subscription>
- launchers — Apple <https://www.apple.com/newsroom/2025/06/macos-tahoe-26-makes-the-mac-more-capable-productive-and-intelligent-than-ever/>, <https://developer.apple.com/videos/play/wwdc2025/260/>, <https://developer.apple.com/videos/play/wwdc2026/8011/>, <https://support.apple.com/guide/iphone/use-and-customize-the-action-button-iphe89d61d66/ios>, <https://www.apple.com/newsroom/2026/09/major-updates-for-apples-software-platforms-are-now-available/>, <https://developer.apple.com/documentation/appintents/appintent/supportedmodes>, <https://developer.apple.com/documentation/appintents/snippetintent>; Raycast <https://manual.raycast.com/ai/quick-ai>, <https://manual.raycast.com/ai/ai-extensions>, <https://manual.raycast.com/ai/agents>; Alfred <https://alfred.app/workflows/alfredapp/openai/>
- iOS limits — <https://developer.apple.com/documentation/backgroundtasks/choosing-background-strategies-for-your-app>, <https://developer.apple.com/documentation/backgroundtasks/performing-long-running-tasks-on-ios-and-ipados>, <https://developer.apple.com/documentation/backgroundtasks/bgcontinuedprocessingtask>, <https://developer.apple.com/documentation/usernotifications/pushing-background-updates-to-your-app>, <https://developer.apple.com/documentation/usernotifications/declaring-your-actionable-notification-types>, <https://developer.apple.com/documentation/activitykit/displaying-live-data-with-live-activities>, <https://developer.apple.com/documentation/activitykit/starting-and-updating-live-activities-with-activitykit-push-notifications>
- coordination — beads <https://github.com/steveyegge/beads>, <https://raw.githubusercontent.com/gastownhall/beads/main/docs/multi-agent/coordination.md>; Claude Code <https://code.claude.com/docs/en/agent-teams>; ClaudeCodeCamp <https://www.claudecodecamp.com/p/claude-code-agent-teams-how-they-work-under-the-hood>; <https://github.com/MrLesk/Backlog.md>; <https://github.com/eyaltoledano/claude-task-master>; <https://github.com/git-bug/git-bug>; <https://github.com/automerge/automerge>; <https://github.com/loro-dev/loro>; Kleppmann <https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html>; Syncthing <https://docs.syncthing.net/users/syncing.html>

*R5 — realtime voice*
- OpenAI — <https://developers.openai.com/api/docs/guides/realtime>, <https://developers.openai.com/api/docs/guides/realtime-vad.md>, <https://developers.openai.com/api/docs/guides/realtime-conversations.md>, <https://openai.com/index/introducing-gpt-realtime/>, <https://developers.openai.com/api/docs/models/gpt-realtime-2.1>, <https://developers.openai.com/api/docs/guides/live.md>, <https://developers.openai.com/api/docs/guides/live-delegation.md>, <https://developers.openai.com/api/docs/models/gpt-live-1>, <https://developers.openai.com/api/docs/guides/live-conversations.md>; LiteLLM <https://docs.litellm.ai/docs/realtime>
- Google — <https://ai.google.dev/gemini-api/docs/live-api.md>, <https://ai.google.dev/gemini-api/docs/live-api/capabilities>, <https://ai.google.dev/gemini-api/docs/live-api/session-management>; xAI — <https://docs.x.ai/developers/model-capabilities/audio/speech-to-speech>; Amazon — <https://docs.aws.amazon.com/nova/latest/nova2-userguide/sonic-turn-taking.html>, <https://docs.aws.amazon.com/nova/latest/nova2-userguide/sonic-barge-in.html>, <https://docs.aws.amazon.com/nova/latest/nova2-userguide/sonic-async-tools.html>, <https://nova.amazon.com/sonic>; NYU RITS — <https://rits.shanghai.nyu.edu/ai/nvidia-releases-nemotronlabs-voicechat-an-open-full-duplex-voice-model>
- Kyutai — <https://github.com/kyutai-labs/moshi>, <https://github.com/kyutai-labs/moshi/blob/main/rust/moshi-server/Cargo.toml>, <https://github.com/kyutai-labs/delayed-streams-modeling>, <https://github.com/kyutai-labs/unmute>, <https://huggingface.co/kyutai/tts-1.6b-en_fr>, <https://kyutai.org/blog>
- NVIDIA — <https://github.com/NVIDIA/personaplex>, <https://arxiv.org/abs/2609.21967>, <https://huggingface.co/nvidia/nemotron-speech-streaming-en-0.6b>; Sesame — <https://huggingface.co/sesame/csm-1b>; Alibaba — <https://www.alibabacloud.com/help/en/model-studio/realtime>; Ultravox — <https://huggingface.co/fixie-ai/ultravox-v0_7-glm-4_6>; Kokoro — <https://pypi.org/project/kokoro/>; Piper — <https://github.com/OHF-Voice/piper1-gpl>; Moonshine — <https://github.com/moonshine-ai/moonshine/blob/main/CHANGELOGS.md>
- pipeline — Silero <https://github.com/snakers4/silero-vad>; Smart Turn <https://huggingface.co/pipecat-ai/smart-turn-v3>, <https://www.daily.co/blog/announcing-smart-turn-v3-with-cpu-inference-in-just-12ms>; LiveKit <https://github.com/livekit/agents/blob/main/MODEL_LICENSE>, <https://docs.livekit.io/agents/logic/turns/adaptive-interruption-handling.md>, <https://github.com/livekit/rust-sdks>; Apple <https://developer.apple.com/forums/thread/733733>; Android <https://developer.android.com/reference/android/media/audiofx/AcousticEchoCanceler>, <https://developer.android.com/reference/android/speech/RecognizerIntent>
- crates and platforms — <https://github.com/webrtc-rs/webrtc>, <https://docs.rs/sherpa-onnx>; dev.to <https://dev.to/simple_memo/ios-26s-speechanalyzer-on-a-live-mic-the-5-things-the-docs-dont-tell-you-2ng5>
- MatrixRTC — <https://github.com/matrix-org/matrix-spec-proposals/pull/4195>, <https://github.com/bunnyfu/hermes-matrix-voice-chat-bridge>, <https://github.com/scottgl9/openclaw-matrix-voice>

*R6 — memory and privacy*
- Hermes — the docs/memory page, recorded as `https://hermes-agent.nousresearch.com/docs/user-guide…` (the URL is cut in the digest file), <https://hermes-agent.nousresearch.com/docs/user-guide/sessions>, <https://hermes-agent.nousresearch.com/docs/user-guide/features/memory-providers>, <https://hermes-agent.nousresearch.com/docs/user-guide/profiles>; the source at `main@663362680b`
- Letta <https://docs.letta.com/concepts/memfs>, <https://docs.letta.com/configuration/memory>; mem0 <https://docs.mem0.ai/migration/oss-v2-to-v3>; Graphiti <https://github.com/getzep/graphiti>; Claude Code <https://code.claude.com/docs/en/memory>; Codex <https://developers.openai.com/codex/customization/memories>; Anthropic <https://platform.claude.com/docs/en/agents-and-tools/tool-use/memory-tool>; agentskills.io <https://agentskills.io/specification>; omp `omp://memory.md`, `omp://mnemosyne-memory-backend.md`
- attacks — <https://arxiv.org/abs/2503.03704>, <https://arxiv.org/abs/2407.12784>, <https://arxiv.org/abs/2602.15654>, SecurityWeek <https://www.securityweek.com/zombieagent-attack-let-researchers-take-over-chatgpt/>, OWASP <https://owasp.org/www-project-agentic-skills-top-10/ast02>, <https://arxiv.org/abs/2505.18279>
- IFC — <https://arxiv.org/abs/2506.08837>, <https://arxiv.org/abs/2503.18813>, <https://arxiv.org/abs/2505.23643>, <https://github.com/microsoft/fides>, <https://arxiv.org/abs/2608.27234>
- isolation — <https://github.com/openclaw/openclaw/blob/main/docs/gateway/multi-tenant-hosting.md>, <https://docs.x.ai/grok-bot/computer-and-apps.md>, <https://docs.x.ai/grok-bot/teams-and-enterprises>

*R7 — Matrix as the agent bus*
- matrix-rust-sdk — <https://github.com/matrix-org/matrix-rust-sdk/blob/main/crates/matrix-sdk/CHANGELOG.md>, <https://github.com/matrix-org/matrix-rust-sdk/blob/main/README.md>, <https://docs.rs/crate/matrix-sdk/latest/features>, <https://docs.rs/matrix-sdk/0.19.1/matrix_sdk/room/struct.Room.html>, <https://github.com/matrix-org/matrix-rust-sdk/blob/main/crates/matrix-sdk/src/encryption/mod.rs>, <https://github.com/matrix-org/matrix-rust-sdk/pull/6607>; Element — <https://github.com/element-hq/element-x-android>, <https://github.com/element-hq/sygnal>
- spec and servers — <https://github.com/matrix-org/matrix-spec/blob/main/content/client-server-api/modules/send_to_device.md>, <https://github.com/matrix-org/matrix-spec/blob/main/data/api/application-service/definitions/registration.yaml>, <https://github.com/matrix-org/matrix-spec-proposals/pull/4471>, <https://matrix-construct.github.io/tuwunel/development/compliance/msc.html>, <https://github.com/matrix-construct/tuwunel/blob/main/tuwunel-example.toml>, <https://github.com/matrix-construct/tuwunel/issues/531>, <https://github.com/matrix-construct/tuwunel/issues/358>, <https://github.com/element-hq/synapse/blob/develop/synapse/config/experimental.py>, <https://github.com/element-hq/synapse/blob/develop/docs/usage/configuration/config_documentation.md>
- bots — <https://github.com/beeper/ai-bridge>, <https://github.com/etkecc/baibot>, <https://hermes-agent.nousresearch.com/docs/user-guide/messaging/matrix>, <https://docs.openclaw.ai/channels/matrix/messaging.md>
- push — <https://github.com/binwiederhier/ntfy/blob/main/server/server_matrix.go>
- device tools — <https://agentclientprotocol.com/rfds/streamable-http-websocket-transport>, <https://agentclientprotocol.com/protocol/v2/migration>, <https://agentclientprotocol.com/rfds/mcp-over-acp.md>, <https://docs.ag-ui.com/concepts/tools>, <https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/changelog.mdx>, <https://developers.openai.com/api/docs/guides/realtime-server-controls.md>
- Android — <https://docs.rs/crate/tauri-plugin-background-service/latest>, <https://github.com/tauri-apps/tauri/issues/15671>, <https://developer.android.com/develop/background-work/services/fgs/changes>

*G3, G4, D3* — `https://github.com/tgorka/gitoxide` (the one `allow-git` pin in `deny.toml`, D3); `git+https://github.com/bmad-code-org/bmad-loop` (G4); GoogleCloudPlatform `knowledge-catalog` SPEC and `okf.md` (cited by G3 through the drive's OKF digest, no URL recorded).

**Digests** (files of the coordinating session): `local://digest-R1AgentProducts.md`, `local://digest-R2RustBuildingBlocks.md`, `local://digest-R3ComputerUseKvmApprovals.md`, `local://digest-R4ProxyUxDevices.md`, `local://digest-R5RealtimeVoice.md`, `local://digest-R6MemoryAndPrivacy.md`, `local://digest-R7MatrixAgentBus.md`, `local://digest-G1KeeperBots.md`, `local://digest-G2KeeperSessionsTasks.md`, `local://digest-G3PriorArtMakistack.md`, `local://digest-G4BmadWorkflowFormat.md`, `local://digest-G5KeeperFacts.md`, `local://digest-D1RuntimeExtraction.md`, `local://digest-D2SessionsForAgents.md`, `local://digest-D3ProviderAndVoiceSites.md`, `local://digest-C1Conventions.md`; the pins `local://coordinator-decisions.md` and `local://program-map.md`.

**Repository** (branch `agents-plan`; the grounding lanes' path:line, and §1.7's re-reads):
- `src-tauri/crates/keeper-core/src/{bots/{mod,chat,quirks,discover,grant,tools,commands,voice_target,store,embed,session,context_files,identity,http,sse,audit,remote,follow},sessions/{plan,shape,model,pool,tasks,order},voice/{turn,platform},transcription/{engine,models},notes/{okf,chunk,search_index},org_account/{layout,device_state,manifest,settings_sync},platform,account,auth,vm,registry,config/keys}.rs`;
- `src-tauri/crates/keeper-sync/src/{engine,db,tasks,platform,files,files_write,browse,bots_fs,profile/{mod,folder},git/commit}.rs`;
- `src-tauri/crates/keeper/src/{bots_ipc,bots_drive_ipc,bots_tools,bot_task,sync,sessions_ipc,sessions_exec,sessions_root,sync_ipc,account_ipc,account_restore,voice_ipc,voice_macos,voice_ios,transcribe_ipc,transcribe_macos,ipc,lib,egress,palette}.rs`, `crates/keeper/Cargo.toml`;
- `src-tauri/crates/keeper-syncd/src/{platform,commands}.rs`; `src-tauri/Cargo.toml`, `src-tauri/deny.toml`; `tools/fluidaudio-rs/`;
- `src/components/{bots,settings,sessions,notes}/…`, `src/lib/ipc/gen/ProviderKind.ts`, `src/lib/stores/sync.ts`, `dev/mock-shell.ts`, `package.json`, `lefthook.yml`, `.github/workflows/{ci,release}.yml`;
- `docs/{decisions,sessions,sync,notes,ios,egress,constraints-and-limitations}.md`, `AGENTS.md`;
- `_bmad-output/planning-artifacts/{architecture/architecture-keeper-2026-07-03/ARCHITECTURE-*.md,prds/prd-keeper-2026-07-03/prd.md,epic-61-…,epic-62-…,epic-63-…,epic-64-…,epic-69-…,epic-85-…,epic-88-…,research-ai-chat-2026-09-02.md,research-transcription-2026-09-28.md}`, `_bmad-output/implementation-artifacts/{deferred-work.md,sprint-status.yaml}`, `_bmad/`;
- the drive mirror `/workspace/tgdrive/{README.md,.gitignore,AGENTS.md,.okf/}`; the operator's makistack as G3 and G5 read it.
