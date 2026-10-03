# Coordinator decisions — keeper agents program (2026-10-02)

Status: pinned by the coordinator from three owner rounds. Lanes cite this file; they do not relitigate it.
AD/FR/NFR/DW/D numbers are assigned in the architecture doc (next free numbers from C1).

## Owner's asks (verbatim, English and Polish as written)

Round 1 (2026-10-01), excerpts:
> i want to explore topic of creating an agents system in keeper - similar to grok bot or hermes bot for work with having different sessions (active/archived), different persons will hav diffrent tools to use (and/or access to local drive or notes or use the keeper itself or the whole computer) - it could connect to model provider cliproxy like this omp. there will be one main person (like nixi for me) to talk to on everyday basis
> i want bmad style personalities and different purposes (coding, exploring, designing, marketing, hr, psychologist etc)
> This persons could connect to each other (sent a message, or use the kaban board - look grok bot, hermes)
> Use sessions in the drives as a working place of the agent with all the data he needs, scripts he needs to use etc logs but also the message history and actions taken - so the session can be used after. - also after sync by drive the work can be continued on other device that will sync the data in sessions (data is all he needs) - but make sure its fast to operate.
> I want also the scheduled job on the kaban to work on (tasks on keeper).
> In the future I want to include kvm (like NanoKVM-Go+) to use other devices as a connectors.
> I want to have connectors for dangerous actions before proceed (to review data in sessions to proceed after programaticly). - style like in Violoop
> I dont want sidecar. I want my mac, iphone, sever on linux to use it.

Round 2 (2026-10-01), excerpts:
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

Round 3 (2026-10-02), verbatim:
> - Dr Lucyna Novak instead of Dr Lucyna Nowak
> - Dr Tola Grey instead of Teo
> - **Private option** (keeps D-5 - yes for the option
> - for now skip drawing - later state will decide
> - memory, skills, soul, etc bot data find a right place in the drives fro this files (tgdrive i neuradrive)
> - rewriting to rusr recommended parts (add separate source module)
> - naia - omit for now
> Rest plan the bmad and proceed with the implementation (bmad research, architectire, epics etc then review and implement one after another one)
> create prs on the stack using gh stack for all the work

## Pinned decisions

### P1 Names and roles
| Bot | Role | Home drive | Audience (readers) |
|---|---|---|---|
| Nixi | tgorka's proxy — the only door; free conversation, no workflow | tgdrive | {tgorka} |
| Dixi | Marta's proxy | Marta's drive (not in this repo's scope beyond config) | {marta} |
| Dr Tola Grey | steward of tgdrive: plans, decides, dispatches, harvests knowledge | tgdrive | {tgorka} |
| Dr Lucyna Novak | steward of neuradrive (neuraffica), shared | neuradrive | {tgorka, marta} |
| BMAD specialists (Mary, John, Winston, Amelia, Sally, Murat, Paige) + BMB-built (marketing, HR, psychologist) | workers | one instance per home drive (e.g. `amelia` in tgdrive ≠ `amelia` in neuradrive) | the home drive's readers |
Naia: omitted. Heavy vs light is a property of the tools a bot's work needs, not a bot kind.

### P2 A bot is (persona, home drive); its data lives in a new zone `80-bots/`
Both drives share one zone layout (10–70 content, 90/99 service, 80 free; `/workspace/tgdrive/README.md:9-24`). Bot homes get their own zone because a bot is a persistent identity with its own lifecycle, review rules and OKF bundle — not a session.
```text
80-bots/
  README.md  AGENTS.md          zone contract (written by `keeper-agentd bots init`, then the owner's)
  _drive.toml                   drive identity + readers (Matrix user ids of the humans who may read this drive)
  _template/                    skeleton for a new bot home
  _skills/<name>/SKILL.md       agentskills.io format; shared by this drive's bots
  _workflows/<name>/            BMAD-format workflow (SKILL.md, steps/, templates) + workflow.toml header
  <bot>/
    bot.toml                    machine config: id, matrix user, model, tools, skills, workflows (menu), host needs, limits
    SOUL.md                     persona: BMAD fields (role, identity, communication_style, principles, persistent_facts) — humans only
    USER.md  MEMORY.md          core memory, hard char caps, written only by the consolidator or a human
    journal/YYYY-MM-DD.md       episodic memory, bot append-only
    proposals/<ulid>.md         staged memory/skill changes awaiting consolidation/review
```
keeper adopts this layout through a folder flag `[folder.bots]` (default subfolder `80-bots`), mirroring `[folder.sessions]`.

### P3 Session = existing `60-sessions` flat contract + agent files
```text
60-sessions/active/YYYY-MM-DD-<slug>/
  AGENTS.md README.md            existing
  agent.toml                     owner bot, requested_by, parent session, drives in scope, label, room id, placement needs
  log/YYYY-MM.<host>.jsonl       append-only event log, one writer per file (the host that wrote it)
  approvals/<ulid>.json          pending action (never edited after creation)
  approvals/<ulid>.decision.json decision written by the owning host on receipt of the human's Matrix decision
  <task>.md (tag task)           existing board cards + new fields assignee/host/requested_by
  artifacts/ workspace/          existing
```
The session log is the truth for agent sessions (flips AD-154 for agent sessions only; the ⌘9 direct-provider chats stay in keeper.db). `.keeper/` holds a rebuildable index; logs are never re-read on the hot path.

### P4 Transport: Matrix only (tuwunel on electra); no hub, no listening socket
- One Matrix room per session; a bot's main session is the DM with its human (Nixi↔tgorka).
- Token streaming = debounced `m.replace` edits (≥400 ms apart) + one final edit. tuwunel rate-limits only login (R7 §2).
- Custom events `dev.keeper.agent.*`: status, approval request/decision, surface request/result, doorbell; state events: claim (per session room), host manifest + presence (per principal control room).
- This reverses round 2's "hub" recommendation: the private voice option sends only text; Matrix gives addressed, queued, E2EE, pushable delivery; a hub would be the first listening socket keeper ships (DW-215 threat model). Revisit trigger: measured p95 Matrix delivery > 1 s on tuwunel, or server-side voice.
- The phone/tablet never write session logs; they send Matrix events; the owning host writes the log (phone cannot merge: `docs/ios.md:737`).

### P5 Hosts and copies
- `keeper-agentd`: headless Linux binary, one process per principal (own OS user + systemd unit): `agentd-tgorka`, `agentd-marta`, `agentd-neuraffica`. tgdrive is never mounted for agentd-neuraffica.
- The keeper desktop app hosts bots in-process (no launchd agent; D-3 asymmetry kept; "no sidecar").
- Phone/tablet: clients only.
- One bot user on Matrix per bot; one Matrix device per (bot, host) = a "copy" (nixi@electra, nixi@hesperia).
- Host manifest = Matrix state event `dev.keeper.agent.host` (state_key = host): tools, drives present + materialized, always_on, version.

### P6 Claims and placement
- Claim = state event `dev.keeper.agent.claim` in the session room: {host, epoch, expires_at}; renew 60 s, TTL 180 s; takeover writes epoch+1 then reads back; log lines carry the epoch; readers drop lines from a superseded epoch written after the takeover.
- Placement (pure, keeper-core): required tools ∩ data present ∩ live hosts ∩ principal; prefer always-on unless the needs pin a host; otherwise `waiting: <host>`.
- Nixi's main session is owned by the always-on host (electra); hesperia takes over only after claim expiry.
- Clean shutdown flushes, pushes and releases claims; sudden loss → expiry → takeover or waiting.

### P7 Delegation is a session, not a sub-agent
`delegate(bot, brief, drives)` → the delegator creates a room and invites the target bot; the target's host creates the session folder in the target's home drive (the target owns it), recording `requested_by`, parent session, label, task card. Reply = final message + card status + artifact links. Limits: hop ≤ 3, ≤ 3 rounds per exchange, token budget per delegation. In-turn helpers (review passes) never own work. Actions inside a delegated session need one tier more approval.

### P8 Privacy
- Layer 1: process per principal (OS user). Layer 2: grants (no grant ⇒ no access). Layer 3: labels.
- Label = (readers: set of human Matrix ids, integrity: owner ⊐ peer ⊐ agent ⊐ untrusted). Join = readers ∩, integrity min.
- Session label = join of everything read. Send/write/delegate to a sink whose audience ⊄ label.readers is blocked unless the owning human declassifies (audit row). Consequential calls decided under `untrusted` integrity are blocked or need approval.
- Sending into a room requires every human member and every bot audience ⊆ label.readers.
- Sensitive bots (psychologist, tgdrive-sensitive work) pin a local model.

### P9 Approvals (Violoop-style prepare/commit)
Tiers T0 observe … T5 forbidden (R3 taxonomy); +1 tier when delegated, unattended, or after untrusted input. Pending action = immutable `approvals/<ulid>.json` (digest-bound: tool, canonical args, exec binding, checkpoint hash, preconditions) + Matrix event; the human decides from any device (Matrix event from a verified device); the owning host writes `<ulid>.decision.json`, re-checks the digest and preconditions, consumes exactly once, resumes the parked run. Never "always" at T4+.

### P10 Provider
Third `ProviderKind::OpenAi` — generic OpenAI-compatible endpoint (CLIProxyAPI). D-4 unchanged (the endpoint is yours). AD-146's closed set gains one member with a real endpoint to read against (its own revisit trigger).

### P11 Workflows
BMAD skills as-is (SKILL.md + steps + customize.toml) + `workflow.toml` header (inputs, outputs, tools, drives, trigger). Rust ports of BMAD's deterministic helpers. Runtime tool surface = BMAD's assumed capabilities (G4 §5). `ask and wait` → `ask_human` (via the proxy). Scheduled workflows = new `TaskKind::Workflow` run on agent hosts.

### P12 Memory
Hermes-style: caps, frozen snapshot per session, proposals, nudges (10 user turns / 15 tool iterations), nightly consolidation on the always-on host under a lease with OpenClaw dreaming gates (no promotion from untrusted/cron/delegated sessions; >25% loss rejected), weekly curator (stale 14 d, archive 30 d, never delete), git trailers `Memory-Origin`/`Source-Session`. Shared drives: owner review. Knowledge harvest: sessions → OKF notes `human_reviewed: false` → promote panel.

### P13 Voice — private option (D-5 kept)
On-device only. Silero VAD + Smart Turn v3 (ONNX) loaded from the owner's config repo `_models/` (D-29 precedent; nothing bundled, nothing downloaded). Backchannel rule before barge-in stops speech. Truncate the assistant turn at the played sentence and log `heard_until`.

### P14 Rust ports live in a separate crate `src-tauri/crates/keeper-ported/`
Pure (no keeper deps, no network, no tauri), one module per upstream with an `UPSTREAM.md` (repo, commit, licence, what was ported/changed). Modules: `bmad`, `hermes`, `openclaw`, `agentskills`, `okf`, `smart_turn` (feature extraction only), `nanokvm` (protocol encoding only). Each module lands with its first consumer.

### P15 Platforms
D-1 reopened: paid Apple account assumed → APNs via a self-hosted push gateway, notification actions for approvals, sessions board on the phone (DW-237). Android: platform start (Platform port impl, build, sideload, UnifiedPush/ntfy, AEC).

## Scope guards
- No drawing / Excalidraw (owner: later). No Naia. No hub. No hosted voice. No keeper-as-MCP-server listening socket.
- No change to ⌘9 direct-provider chats' storage.
- The keeper shell crate is changed by inspection only on Linux; every shell change is named in the PR as awaiting CI's macOS job / `check:rust:macos`.

## Stack order (one stack, bottom → top)
plan rung (research + architecture + epics + D-entries + sprint-status) → epic 89 → 90 → 91 → 92 → 93 → 94 → 95 → 96 → 97 → 98 → 99 (each epic 1–3 rungs; every rung compiles alone).

## Rulings on the deep dives' pushback (D1, D2, C1) — binding, supersede P2/P3 where they differ

- **R1 Vocabulary: "agent", not "bot".** `bots` is taken (`keeper-core/src/bots`, `[[provider.bot]]` in the device file; D2 §7). The new concept is an **agent**: zone `80-agents/`, folder flag `[folder.agents]` (default subfolder `80-agents`, mirroring AD-342's voices flag recipe, D2 §3), core module `keeper_core::agents`, seam crate `keeper-agent`, daemon `keeper-agentd`, UI word "Agents". An agent *runs on* a provider bot (model). The persona file is `SOUL.md` — same role as Hermes' `SOUL.md` (slot #1 of the system prompt); keeper reads ours from the drive and never touches a Hermes profile. Code avoids the word "persona" (C1 §4: telemetry/brand collisions); fields are `soul` / `identity`. The brainstorm line "bots are instruments, never characters" is overridden by the owner's explicit ask for named personalities (coordinator note in the architecture doc).
- **R2 Run state is its own key.** The board keeps its four columns (`in-preparation`, `todo`, `done`, `deferred`, `shape.rs:356-416`). Agent run state is `run:` (`queued | running | blocked | review | failed`) shown as a badge; new card fields `assignee:`, `host:`, `requested_by:`, `schedule:`, `last_run:` (already carried in `PoolEntry.fields`; `SessionTaskVm` gains them). No widening of `TaskStatus`.
- **R3 Session log format.** `log/YYYY-MM-DD.<host>.<n>.jsonl` chunks, rotated before `min(192 KiB, 3/4 × the profile's LFS threshold)` so a chunk never becomes an LFS object; a line whose body exceeds 16 KiB stores the body in `log/blobs/<sha256>.json` (immutable) and references it. Written only by keeper-agent's session writer (O_APPEND, fsync at turn end), never through `drive_write`. Torn tail of the host's own current chunk is truncated on open. One writer per file = the claim holder's host.
- **R4 Session verbs are made safe before agents call them.** Per-zone mutex, journal resume at startup, caller-supplied session id (idempotent create) — a prerequisite story when the sessions runtime moves into keeper-agent (D2 §1).
- **R5 Doorbell = `Engine::pull_now(profile_id)`** (one fetch, no walk), plus the paced-remote-poll fix in `tick_profile` with the pinning test `a_quiet_live_watcher_folder_asks_the_remote_every_remote_poll` (D2 §5). `wake_now` is not a doorbell.
- **R6 Crate roles.** `keeper-agent` is the core×sync seam only (turn loop, drive tool host, session runtime, task runner). The lean Matrix agent client lives in `keeper_core::agents::matrix` (AD-6: new Rust defaults into keeper-core). Extraction lands first with the shell as the only consumer and no behaviour change (D1 recommended order).
- **R7 agentd owns its own Engine and sync.db** (never opens keeper-syncd's database — `db::recover_running` would requeue syncd's work, D1 §6). One agentd per principal, its checkouts under its own XDG data dir. XDG + secret helpers move from the syncd bin into `keeper_sync::xdg` and are shared. agentd is never a syncd subcommand (AD-52).
- **R8 Headless configuration.** `agentd.toml` (principal, homeserver, drives, providers, agents to host) + secrets from env or 0600 files (syncd pattern) / systemd `LoadCredential`. CLI verbs `keeper-agentd init | login | agents list | agents init | run | status`. agentd needs a multi-threaded tokio runtime (approvals' `block_in_place`).
- **R9 Scheduled agent work lives on board cards**, not in `TaskKind`: a card's `schedule:` (keeper's task schedule dialect, reusing keeper-sync's pure parser), optional `host:` pin, `assignee:`. Agent hosts evaluate due cards on their own tick (AD-62: one clock per host process); the Matrix claim prevents a double run. `TaskKind::Bot`, AD-224 and AD-226 stay as they are (Mac housekeeping).
- **R10 CI targets.** `keeper-agent` builds on every target the shell builds (iOS included — the shell already depends on keeper-sync there). `keeper-agentd` is excluded from the iOS `cargo check` and gated on Linux by lefthook + a release job; new guards `check:agent-tauri-free`, `check:agentd-lean`.
- **R11 matrix-sdk stays at 0.18** for this program; agent events use `Room::send_raw`, `send_state_event_raw`, `get_state_event_static`, `Client::add_event_handler` (D1 §3). No to-device dependency.
- **R12 A human decision is a Matrix event from a verified device** of a human in the session label's readers; the host checks the device's verification through matrix-sdk before writing `<ulid>.decision.json`.
- **R13 Smoke endpoints available to this program:** CLIProxyAPI at `https://electra.siren-alsephina.ts.net:8452` (`/v1/models`, `/v1/chat/completions`; token `~/.omp/cliproxyapi.token`, never logged); the Synapse test homeserver `keeper-test-synapse` on delectra (on-demand) for Matrix agent smoke tests; hesperia over `ssh mac` for the macOS gate.

## Rulings on L1's contradictions (2026-10-02)

- **R14 Barge-in becomes pause-first (amends AD-208, keeps its intent).** `SpeechDetected` while `Speaking` pauses the synthesiser at once (nothing talks over the person); the utterance then decides: a backchannel (one word from a closed list, e.g. "mhm", "yeah", "aha", "tak", "no", or under 600 ms of speech) continues the paused speech; anything else stops it and becomes the next question; the stop phrase ends the turn. Lands in epic 97.
- **R15 The `run` tool keeps DW-213's letter.** argv only, never a shell string; OS-sandboxed; never a `TaskKind`; Epic 60 stays reserved and untaken. It gets its own D-entry (D-33).
- **R16 makistack epic 22's refusals scoped Hermes, not keeper.** "nie podpinaj dysku", Q1, Q2 were decided for a Python gateway with a CVE stream and chat ingress. Keeper agents get drive access only through grants, labels, process-per-principal and approval tiers; the first write still asks (AD-158). Q1 survives: Paseo `create_agent` is a T3 action (approval each time). Recorded as a coordinator note, not a reversal of makistack's record.
- **R17 neuradrive must push from agentd-neuraffica's own checkout.** keeper-agentd's checkouts are bidirectional by construction; switching the server's neuradrive from pull-only is an operator action, owed outside this repo and named in the epic.
- **R18 Streaming edits are adaptive.** The sink coalesces deltas, honours a 429's `retry_after_ms`, and always delivers the final edit; smoke tests against Synapse use the admin rate-limit override for agent users.
- **R19 Names.** Matrix user `@nixi:<server>`, home `80-agents/nixi/`, display name "Nixi"; `@tola-grey` / `80-agents/tola-grey/` "Dr Tola Grey"; `@lucyna-novak` / `80-agents/lucyna-novak/` "Dr Lucyna Novak". The voice wake phrase stays the person's own setting (D-5 default unchanged); the Hermes profile `nixie` is untouched.
- **R20 D-5 gets a D-29-style amendment** for the turn models from `_models/` (on-device, nothing bundled, nothing downloaded). Android voice (98.4) uses the platform's on-device recogniser in segmented sessions; continuous duplex on Android is a documented limitation, not a promise.
- **R21 Upstream licences decide what is ported as code.** `nanokvm` encodes the HID/WebSocket protocol from the MIT community reference (scgreenhalgh/nanokvm-mcp), never the GPL firmware; OpenClaw's gates are re-implemented from its documentation (licence reported both MIT and "Other"), and UPSTREAM.md says so; BMAD-METHOD's licence is checked before 94.1 lands and recorded in UPSTREAM.md.
- **R22 A target reached through a KVM raises the tier by one** (the fourth +1 condition).
- **R23 A final answer over 60 KiB** is sent as its first 60 KiB plus a link to `artifacts/answer-<ulid>.md`; the full text is in the session log and that artifact.

## R24 — E4's open questions: the readings are accepted (2026-10-02)
(1) every configured MCP server and KVM carries a `readers` list and `Sink::External` carries it; (2) a `role = "paseo"` MCP server gets four fixed tier rows; (3) KVM tools are `kvm_snapshot` and `kvm_act`; (4) `[[mcp]]` gains `command`, `readers`, `role`, `fingerprint`; `agent.toml` gains `[[gate]]`; `delegates` copied into a gate session's `agent.toml`; (5) Linux sandbox = landlock + a seccomp filter (`seccompiler`); (6) element actions are re-checked through the accessibility path, coordinate/KVM actions with a difference hash; (7) MCP tools travel as `mcp__<server>__<tool>`; (8) epic 96 does touch the shell (MCP Settings commands) — the epic map is corrected; (9) a gate's reply to the requester in its own room is exempt while `check_sink` allows it, and every delegation from a gate waits for a person; (10) backchannel lists are per language; (11) `_models/` hydration by role, so the phone takes only the turn models; (12) DW-237's location is updated to `ipc.rs:1481`; (13) 98.3 adds the Android CI job; (14) the iPhone notification extension's slim static-library crate `keeper-nse` joins the crate topology; (15) a networked `run` is approved per run, and that approval is the configuration, with a row in `docs/egress.md`.

## R25 — E2's open questions: the readings are accepted (2026-10-02)
Status and scope render as a header beside the timeline's item stream (matrix-sdk-ui 0.18 filters custom types); 91.1 draws approval cards once 93.3 exists; declassification is routed in 92.6 and decided in 93.3; epic 92 has three rungs (delegate / board / stewards); harvest runs in the steward's own session; `run:` gains `waiting`, and only the owning host writes it; a scheduled card's runs are owned by its assignee's session host; session `kind` gains a value for a person-started proxy conversation and the status schema names the DM; a claim epoch is not an approval precondition (takeovers must not drift); delegate/reply/card_update get tier rows; canonical JSON (RFC 8785 subset: sorted keys, serde_json numbers, no floats in digested fields) is implemented in core, no crate; 93.3 adds the master-key fingerprint display and `[[trust]].master_key` is optional, and an unpinned person's decisions are not accepted (the TOFU reading was rescinded by S-06/F3: a person writes the pin after comparing fingerprints); 91.5 owns `agents init`, 90.6 owns copy sign-in; `[folder.agents]` waits for every syncing machine's upgrade (operator action). **Every Nixi→shared-agent hand-off is a declassification:** the proxy's context holds private core memory, so a brief to a wider audience is shown to the person and released with one tap; the release is the recorded declassification (stricter than the D-34 draft wording, which is corrected to match).

## R26 — E3's open questions: the readings are accepted (2026-10-02)
The promote panel is FR-243/FR-244 (not FR-229/235/236/241 — those stay unbuilt, DW-E95-7; AD-404 and the research §9.4 citation are corrected); harvest writes into the steward's own harvest session; harvested notes are signed `agent:<agent>@<host>` and a person's tick writes both `verified[]` and `human_reviewed`; the BMAD port refuses duplicated config keys byte-identically to `render_skill.py` (OA-94-1 fixes the operator's plugin template, DW-E94-3); workflow paths are drive-relative with outputs under the session's `artifacts/`; `.memlog.md` is one named exception to the Hidden refusal; 95.1 moves 89.3's cap primitives behind `keeper-ported::hermes` and the cap counts `\n§\n` delimiters as Hermes does; OpenClaw's defaults (0.75 / 3 / 3) stand (DW-E95-8); the OKF port reproduces the drive's inert workspace exclusion and keeper excludes session workspaces by its own rule; consolidation's 03:00 is the host's local offset; a due workflow card opens a new session per window; a review layer's model comes from an overlay `bot` key; a shared drive's USER.md change is approved by the source session's requester; the BMAD port pins tag v6.12.0 @05bfbd46 (four config layers).

## R27 — E1's open questions: the readings are accepted (2026-10-02)
The file-level log writer lives in `keeper-core` and 90.5's runtime wraps it; 89.3 does read-side cap checks in core and 95.1 moves them behind `keeper-ported::hermes`; stewards' default tools follow AD-389; 89.3 touches the shell by one call (the agents write fence); `agents init` is 91.5's; a fifth port `GrantSource` lands in 90.5 (grants per agent); `TurnOrigin::Agent` lands with its first caller in 90.5; the shell computes a ⌘9 turn's origin exactly as today; BMAD-METHOD is MIT, pinned at v6.12.0, trademark notice recorded; agentskills validator Apache-2.0; persistent-fact file walks land in 90.5; agentd's secrets directory is its own; the desktop's AD-377 "that person" is the signed-in account's login; 90.5 measures the Megolm-safe final-message cut on Synapse.

## R28 — Security review (Fable 5.1, S-01…S-35): every finding accepted (2026-10-02)
Full text: `_bmad-output/planning-artifacts/agents-review-security-2026-10-02.md`. Rulings, where the review offered a choice:
- **S-01** consume-once: before any effect the owning host sends `dev.keeper.agent.approval.consumed {id, epoch, host}` and waits for the server's event id; the log line mirrors it; resume on any host checks the room first. 93.2 gains the crash-before-push takeover test.
- **S-02** `_drive.toml` gains `[integrity] untrusted = [globs]` (defaults `00-inbox/**`, `70-comms/**`, `recordings/**`); OKF `sources` of external origin read `untrusted`; a card made from untrusted content opens its delegated session `untrusted`.
- **S-03** a networked `run` mounts only `workspace/` (no drives, no `.keeper/`); the declassified object is the SHA-256 set of `workspace/` at consume time, re-checked as a precondition.
- **S-04** the model provider is a sink: `Sink::Model { local }`. A provider is a processor the person chose, not an audience — D-34/NFR-115 say so and why — and `local_only` is enforced per call: `Label` gains `local_only: bool` (join = OR, set by reading any `local_only` drive or agent home); `Sink::Model { local: false }` is refused while it is set; embeddings and review-layer bots go through the same check.
- **S-05** claims: re-read after a settle (≥ 2× RTT or one sync round); fence key = (epoch, claim event id) on every line; two `acquired` lines for one epoch mark the session conflicted and refuse replay.
- **S-06** TOFU withdrawn (R25 corrected): `[[trust]]` pins are written by a person after a fingerprint comparison; keeper never pins by itself.
- **S-07/S-08** sandbox: landlock denies `/proc`; seccomp denies ptrace, process_vm_*, kcmp, pidfd_getfd, perf_event_open; `PR_SET_DUMPABLE=0`; secret env vars scrubbed after reading; `LoadCredential` first; SBPL denies process-info* and mach-lookup; `HOME` is a fresh empty temp dir; `GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 -c core.hooksPath=/dev/null`; root dotfiles and `.git/hooks/*` are code the session holds (T4, hashed operands).
- **S-09** a `main` session's integrity is per turn (reset at each `user` line); its readers stay cumulative.
- **S-10** `summary` is composed by keeper from per-tool templates, never model text; a notification offers *Approve once* only when the whole payload fits (short argv, no newline, never a Paseo prompt); T4 never from a notification.
- **S-11** `main` sessions offer `once` only; `session` scope elsewhere lasts ≤ 24 h.
- **S-12** an agent-proposed skill is not offered until a person adopts it.
- **S-13** private-drive promotion needs ≥ 1 contributing session with `owner`/`peer` input, else the review card.
- **S-14** MCP annotations lower a tier only with `trust_annotations = true` or per-tool tier rows; default T3; `command` servers never below T2.
- **S-15** `agentd.toml [[drives]]` (and the desktop's device-local profile) pin `readers`/`owner`; a differing `_drive.toml` makes the zone host nothing, naming the difference.
- **S-16** every status/scope edit passes `check_sink(Room)`; progress carries counts, never paths; when the label is narrower than the room, one fixed sentence in the room and details to the requester's proxy DM.
- **S-17** a secret scan redacts every `tool_result`/`assistant` body before it is logged (`[REDACTED secret-like: sha256:…]`); D-31 says a session folder is as sensitive as the drives it reads.
- **S-18** screenshots and KVM frames live under `<zone>/.keeper/previews/` (never committed) and travel only as the encrypted Matrix attachment; records keep the hash.
- **S-19** keeper-agent/agentd register no observability sink; the desktop excludes agent-host spans from export by target prefix (pinned by a test); `check:agentd-lean` forbids the PostHog/OTLP crates.
- **S-20** an operator action before 91.5 records the upstreams behind CLIProxyAPI and its cloak-mode setting and the owner's acceptance of the terms risk in `docs/agents.md`; `agents init` requires `--bot` (no default); tests read the endpoint from env, never a literal.
- **S-21** `schedule:`/`workflow:` through `card_update` are T3 at every kind; a card whose schedule an agent wrote needs a person's tick before its first run.
- **S-22** a T4 decision must come from a device other than the owning host's own process; D-33 records the residue.
- **S-23** gates get a per-peer token bucket (`max_tickets_per_hour`, default 10) and one coalesced card per peer per window.
- **S-24…S-35** minors accepted as written (digest covers id/session/agent; scheduled-run window in the claim anchor; passphrase guarantee stated; proxy leaves after a relay; T4 requires the requester in `dispatch_chain`; Paseo readers default `*`; agentd releases minisign-signed; `owner` means "committed by a reader's keeper"; harvested notes ≤ 64 KiB; manifests publish provider ids; secrets dir 0700/owned/no symlinks; clobber check by git blob id).

## R29 — Consistency review (Fable 5.1, F1…F25): every finding accepted (2026-10-02)
Full text: `_bmad-output/planning-artifacts/agents-review-consistency-2026-10-02.md`. Rulings where a choice was offered:
- **F1** session rooms whose `kind` is `main` set per-type power levels `m.room.message: 0` and `dev.keeper.agent.scope: 0` so the person can talk; observers elsewhere stay at 0 for decisions only; 90.4 and 91.5 assert both shapes.
- **F2** 90.5 keeps `keeper_agent::agent::SessionContext { messages, memory_snapshot, label, … }` per (session, claim), loaded by `replay` on open/takeover/restart and appended by the writer; acceptance: a served session's second turn opens no file under `log/`.
- **F3** TOFU is rescinded (same as S-06): a pin is written by a person after comparing fingerprints; until then that host accepts no decision from that person.
- **F4** 95.2's gate *skips* (leaves pending) `gate`-origin proposals and the curator expires them at 30 days (`verdict = "expired"`); 95.1 turns the nudge pass's `memory_propose` off for gate sessions.
- **F5** 90.5 joins a `dev.keeper.agent.session` room on invite when the inviter is an agent user of a drive this host knows (checked by `check_sink`) or a proxy's `human`; an invite from anyone else stays pending; 92.1 orders join → read the delegate event → place → create (idempotent on the event id); the delegator sends the brief only after it sees the target's join.
- **F10** NFR-113 gets an assertion in 90.5's Synapse harness (anchor ≤ 1 s, final edit ≤ 1 s after stream end, p95 over ≥ 50 turns) with tuwunel as the published figure; NFR-114 names its clock points (VAD speech end → `UtteranceEnd` → send) and the log fields 97.3 writes.
- **F11** digested fields hold integers only: a float anywhere in `{tool, args, exec_binding, preconditions}` refuses the record; keys sorted by UTF-16 code units; RFC 8785 string escapes.
- **F12** 90.1 defines `keeper_agent::host::UNATTENDED_REFUSAL`, returned as the tool result; every later story cites it.
- **F14** FRs that only a device can prove are reworded to what the repo proves, with a device-run acceptance whose recorded artifact (`docs/agents.md` § Measured) is the gate.
- **F17** the architecture's `bot:openai:` example stands (89.6 lands in the same epic).
- **F19** a drive mounted by several principals rings every control room whose `agentd.toml` lists it.
- **F20** UX-DR137 says what it adds to UX-DR90 (the knowledge notes and the staleness badge) and cites it.
- **F21** agentd's system unit names its directives (`ProtectSystem=strict`, `ReadWritePaths=` its XDG dirs, `PrivateTmp`, `NoNewPrivileges`, `LoadCredential=`; `ProtectHome` not set because its data lives under its own home — stated).
- **F22** the coordinator's decisions and the program map are committed under `_bmad-output/planning-artifacts/` and cited by path; headers say "deferred items in DW-x…DW-y" instead of "allocates no number".
- **F23** 89.5's measurement is kept as an `#[ignore]` test.
- **F6–F9, F13, F15, F16, F18, F24, F25** are corrections applied as written.

**R30 — session rooms are encrypted, so the host enforces who speaks (2026-10-02):** every session room allows `m.room.encrypted` at power 0 and keeps `state_default` 50: the homeserver sees a person's every event as `m.room.encrypted` and cannot tell a decision from free text (measured on Synapse), so people decide everywhere, talk in a proxy's rooms and write no state, and the agent host — which decrypts — keeps a person's free text out of any session that is not their proxy's conversation (logged as an observer note, never a turn), accepts a decision only from a reader's verified device, and ignores a `dev.keeper.agent.*` event or an edit from anyone but the agent's own user (AD-380; 90.4's power levels, 90.5's `keeper_agent::rooms::classify`).

**R31 — a spoken send to an agent (2026-10-03):** a spoken utterance addressed to the person's own `main` or `conversation` agent room is a user-initiated dispatch: a third `SendTrigger::SpokenToAgent`, legal only for a room whose agent-room kind is `main` or `conversation` and whose proxy's human is the signed-in user, skipping the Undo-Send hold; the AD-13 guard tests move to the new exact count and assert the room restriction, AD-13 carries a note citing R31, and no other trigger is added.

**R32 — status is a stream of events, not edits (2026-10-03):** each status update is a `dev.keeper.agent.status` event carrying `content.anchor`; 91.1 AC3 and the architecture's Matrix-events table say so, and the device's reader mirrors `trail_of` (group by `content.anchor` else the event id; the sender is `content.agent`, power ≥ 50 and not the own user; the newest `origin_server_ts` wins; an unknown `run`, `kind` or `v` is *unreadable*, never dropped).

**R33 — the header's source (2026-10-03):** option B: the SDK's default timeline filter, plus dropping `dev.keeper.agent.claim` and `.host` state from agent-room timelines; status and scope are read beside the item stream from the room's event cache, paging back like `runtime::latest_status` when the in-memory chunk holds none. The first test measures whether the event cache holds decrypted custom events after a restart; if it does not, option A, said so. The header travels as `TimelineBatch.header?: AgentRoomHeaderVm` (`#[ts(optional)]`), not a second channel, and shows on the phone too.

**R35 — the run vocabulary (2026-10-03):** the event's `RunState` (`idle | running | blocked | waiting | done`) is the device's vocabulary; UX-DR129 is corrected to it.

**R36 — a new conversation is made by the proxy's host (2026-10-03):** the person's device sends `dev.keeper.agent.conversation.request {v, title?}` in its main DM, accepted only from the proxy's `human` sealed by a verified device (R30); only the current claim holder of that main session acts — it creates the room (`RoomKind::Session(Conversation)`), creates the session folder with `create_agent_session` under its claim (idempotent on the request's event id), invites the person and answers in the DM — so people never hold state power and two hosts never race.

**R37 — presence (2026-10-03):** `control_power_levels` gains `dev.keeper.agent.presence: 0`; agentd brings existing control rooms up to date on start when it holds the power to (their creator), else logs one sentence naming the room.

**R38 — surface tools are agent-only (2026-10-03):** ⌘9 bots are never offered them (`tool_specs` for ⌘9 unchanged, pinned by a test); the narrowest mechanism is chosen (an agent tool vocabulary in keeper-agent wrapping the drive verbs, or a gated `ToolName` extension) and the epic's As built says which and why.

**R39 — surface results bypass the turn (2026-10-03):** `dev.keeper.agent.surface.result` is intercepted in `register_handlers` before routing and delivered through an approval-style map keyed by the request id (sender the proxy's human, sealed, verified device); it never queues behind the turn.

**R40 — proposal ranges (2026-10-03):** Rust translates file lines to body lines (frontmatter excluded) and refuses a range inside the frontmatter; `surface.request` carries the exact `expected` text the agent read for the range; the device applies only while the live buffer still holds it, else answers `unavailable`. Apply is one unannotated CodeMirror transaction through the editor runtime (undo-able, through autosave with the buffer's own rev); Rust never writes the note and nothing calls `notes_save` with proposal text.

**R41 — focus (2026-10-03):** the host keeps `focus {drive, path, heading?}` in `SessionContext` in memory (not logged) and states it in the prompt frame of the next turn.

**R42 — answers are not truncated (2026-10-03):** a message carrying `dev.keeper.agent.turn` in an agent room is capped at `FINAL_CUT_BYTES`, not `MAX_BODY_CHARS`.

**R43 — the identity mark (2026-10-03):** the seeded agents use one glyph each (`N`, `T`, `L`), within the existing bound and the 20 px cell.

**R44 — the stop phrase (2026-10-03):** stopping a spoken agent answer stops speech locally; no cancel event exists, and a turn-cancel event is deferred work in the agents block (epic 93 or later).

**R45 — notifications (2026-10-03):** the notify handler does not notify for an agent's `…` anchor or its edits; a notification when an agent's answer completes is deferred to 98.1.

**R46 — 91.5 AC5 restated (2026-10-03):** as R30 says, AC5 asserts the power-level content and the host's `classify`; only a person's *state* write is refused by the server, because a person's encrypted message or scope reaches it as `m.room.encrypted`, allowed at 0.

**R47 — who may set a scope or ask for a conversation (2026-10-03):** `dev.keeper.agent.scope` and `dev.keeper.agent.conversation.request` count from the proxy's `human` on a device signed by that person's own cross-signing identity — `Verified` or `Unverified(UnverifiedIdentity)` — and are ignored from an `UnsignedDevice`, an unknown device (`None`), a `MismatchedSender` or a `VerificationViolation`; an approval decision stays strict `Verified` (epic 93 adds person-pinned trust). Both only act within the agent's own allowed drives or make a room for the proxy's human, so an unpinned but self-signed identity is enough.
