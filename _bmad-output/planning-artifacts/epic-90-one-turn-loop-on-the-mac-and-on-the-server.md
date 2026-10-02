# Epic 90 — One turn loop, on the Mac and on the server

created: '2026-10-02'
status: planned 2026-10-02; build follows in story order on the agents stack
source: the owner's rounds of 2026-10-01/02 (excerpts verbatim below, from `_bmad-output/planning-artifacts/agents-coordinator-decisions-2026-10-02.md` § *Owner's asks*). Other inputs:
- the binding architecture, `architecture/architecture-keeper-2026-07-03/ARCHITECTURE-AGENTS.md`: this epic builds AD-367, AD-368, AD-370…AD-379, with AD-372 and AD-377 shared with Epics 91 and 92;
- the research, `research-agents-2026-10-02.md` (§12.1–§12.5, §6, §13; cited `§n.m`);
- digests D1 (runtime extraction), D2 (sessions for agents) and G5 (infrastructure);
- rulings R4–R13, R17–R18 and R23–R29 (`_bmad-output/planning-artifacts/agents-coordinator-decisions-2026-10-02.md`);
- the two reviews of 2026-10-02, accepted in full by R28 and R29: `_bmad-output/planning-artifacts/agents-review-security-2026-10-02.md` (S-01…S-35) and `_bmad-output/planning-artifacts/agents-review-consistency-2026-10-02.md` (F1…F25). *The reviews' amendments*, below, says where each one lands in this epic;
- the program map (`_bmad-output/planning-artifacts/agents-program-map-2026-10-02.md`);
- this repository's `deploy/companion-stack/` (the Synapse that runs on delectra as `keeper-test-synapse`);
- makistack's `docs/runbooks/matrix-bot-channel.md` (tuwunel users).

Line numbers are in the current worktree, branch `agents-plan`: origin/main `86dee5b7` plus the plan files and Epic 89 as planned. The citations corrected after the consistency review (F16) were checked on 2026-10-02 against this worktree, with epic 89's code in progress in it.
binds: FR-777…FR-783; NFR-112, NFR-113 (90.5's share), NFR-115 (90.3's and 90.5's share), NFR-116 (90.5's share), NFR-117 (90.2's share), NFR-120, NFR-121 (90.5's and 90.6's share); AD-367, AD-368, AD-370, AD-371, AD-372 (90.5's share), AD-373, AD-374, AD-375, AD-376, AD-377 (90.3's share), AD-378, AD-379. All are allocated in the architecture. Deferred items in DW-361…DW-369 and DW-431…DW-432 (DW-361…DW-364 are the architecture's; DW-366 is merged into DW-395 and DW-368 is closed, both after the reviews); UX decision UX-DR128.
- **The previous ceilings:** as Epic 89's header. Every FR, NFR, AD and D number this epic binds is the architecture's.
- **No earlier allocation.** On 2026-10-02 a grep over `_bmad-output`, `docs`, `src`, `src-tauri/crates`, `dev`, `AGENTS.md`, `README.md` and `CLAUDE.md` looked for `epic-90`, `epic90-`, `DW-E90-` and `UX-DR-E90`. It found only the architecture's DW-361…4 (`ARCHITECTURE-AGENTS.md:1241-1244`) and the D-entries citing them (`docs/decisions.md` § D-31 and § D-32).
see-also:
- D-3 (no launchd agent: the desktop hosts in-process), D-4, D-21, D-32 (Matrix is the agents' only live channel; `docs/decisions.md` § D-32), D-34 (labels and a process per principal; `docs/decisions.md` § D-34);
- AD-24, AD-40, AD-52 (syncd is its own binary), AD-53 (egress is derived), AD-62/AD-136 (one clock per host process), AD-158, AD-224/AD-226 (unchanged: `TaskKind::Bot` stays the Mac's);
- Epic 89 (every `keeper_core::agents` grammar this epic runs).

## The owner's ask

Verbatim (round 1, 2026-10-01):

> I dont want sidecar. I want my mac, iphone, sever on linux to use it.
> Use sessions in the drives as a working place of the agent … - also after sync by drive the work can be continued on other device that will sync the data in sessions (data is all he needs) - but make sure its fast to operate.

Round 2 (2026-10-01):

> nixi etc are everywhere - because session is sync - they can have tags (like nixi with electra or hesperia tag) to know what materialization of nixi is used. but for example electra (because is always on) can configue work of hesperia once this one is goes off.
> i would prefer to communicate and cooperate and delegate work for different bots instead of sub-agents - to avoid confustion and make one point of true - also want to make suere its fast
> i own the server infrastructure and tgrive is only for tgorka - make sure the sensitive part goes only to the private bots/drives

## The verdict, ask by ask

| # | The ask (verbatim) | Verdict | How this epic meets it | Mechanism |
| --- | --- | --- | --- | --- |
| 1 | "I dont want sidecar" | **planned** | The Mac app hosts agents in-process (no launchd agent, D-3 kept). The server runs `keeper-agentd`, its own binary, never a syncd subcommand (AD-52). | AD-367, AD-374, AD-375; 90.1, 90.5, 90.6 |
| 2 | "I want my mac, iphone, sever on linux to use it" | **planned for the Mac and Linux** | One turn loop, `keeper-agent`, is shared by both hosts. The phone is a client (Epic 98). | AD-367; 90.1, 90.5 |
| 3 | "(like nixi with electra or hesperia tag) to know what materialization of nixi is used" | **planned** | A copy is one Matrix device per (agent, host), named `nixi@electra`. Every log line and status names the host. | AD-374; 90.4, 90.6 |
| 4 | "electra (because is always on) can configue work of hesperia once this one is goes off" | **planned** | Claims with epochs, and takeover after expiry. Placement prefers the always-on host, and shows `waiting: hesperia — <need>` for what only hesperia can do. | AD-378, AD-379; 90.6 |
| 5 | "after sync by drive the work can be continued on other device" | **planned** | Either host continues a session from its folder (89.5's replay) once it holds the claim. | AD-365, AD-378; 90.5, 90.6 |
| 6 | "make sure its fast" | **planned, measured** | p95 delivery on the homeserver (NFR-112) and streamed edits paced at ≥ 400 ms, adapting to the server (NFR-113, R18). Both are measured and published in `docs/agents.md`. | AD-370, AD-373; 90.4, 90.5 |
| 7 | "tgrive is only for tgorka - make sure the sensitive part goes only to the private bots/drives" | **planned, the process layer** | One `keeper-agentd` per principal under its own OS user, and a mount rule a misconfigured drive cannot pass. Labels at sinks are 92.6's. | AD-375, AD-377; 90.3 |
| 8 | "communicate and cooperate … instead of sub-agents" | **foundation only** | Rooms, copies and claims are what delegation (92.1) is built on. | AD-370, AD-372 |

## What the triage found

| Need | Verdict | Evidence |
| --- | --- | --- |
| A turn loop outside the shell | **absent; in the shell, tauri-bound** | `open_turn`/`arm_turn`/`spawn_turn`/`drive`/`close` (`keeper/src/bots_ipc.rs:1132-1984`), `LiveStream` holding a tauri `JoinHandle` (`:931-933`), the tool host (`bots_tools.rs`, which "does not build on a Linux developer machine", `:6-7`), and a second copy of arming in the task runner (`bot_task.rs:109`; D1 §1, §12.1). |
| Which turns are spoken | **decided inside `arm_turn` from voice state** | `crate::voice_ipc::spoken_turn(dir)` (`bots_ipc.rs:1175`, `voice_ipc.rs:406`) asks the voice turn whether it awaits a send, whatever the entry point. `TurnOrigin` has no code (AD-224; D1 §1). |
| A safe sessions runtime | **absent; two false claims** | `sessions_exec.rs:4-10` promises a per-zone `Mutex` and resume on registry start. Neither exists: `run` calls `resume()` of whatever journal it finds (`:46-60`), and nothing else calls `resume` (D2 §1). |
| An idempotent create | **absent** | `sessions_create` mints its own ULID (`sessions_ipc.rs:873`). Rows come from a scan coalesced over `COALESCE_WINDOW` (400 ms, `sessions_root.rs:47`) in the scanner's loop (`spawn_scanner`, `:172-205`); `rescan` (`:316`) asks for one. |
| XDG and secrets for a daemon | **present, in a bin crate** | `keeper-syncd/src/platform.rs:56-257`: `xdg_dir`, `home_dir`, `env_var_name`, `secret_file_name`, `trim_secret`, `check_secret_permissions`; `SECRET_ENV_PREFIX = "KEEPER_SYNC_SECRET_"` (`:38`), with secrets under `$XDG_CONFIG_HOME/keeper-sync/secrets/` (`:163-166`). |
| Two engines over one `sync.db` | **unsafe** | `Engine::open` runs `db::recover_running` (`engine.rs:1925`, `db.rs:2393`; D1 §6). |
| The folder tier on a daemon | **absent** | Only the app calls `install_folder_tier` (`keeper/src/lib.rs:423`; `profile/folder.rs:40-42`; the installer is `:792`, `FolderTier::new` `:359`). |
| A Matrix client an agent can drive | **absent** | `client_for` is private (`keeper-core/src/account.rs:1942`). Sending needs an open UI timeline. Create, invite, raw and state sends are missing (D1 §3; §12.3). The SDK can do all of it (0.18, `src-tauri/Cargo.toml:61-62`), and core already sends raw requests with `Client::send` (`bridges/discovery.rs:193`, `:323`). |
| A test homeserver | **present** | Synapse `v1.156.0` from `deploy/companion-stack/docker-compose.yml`, running on delectra as `keeper-test-synapse-1`. Client API only on `:8008`; registration through the shared secret (`deploy/companion-stack/README.md` § *Create your first user*). |
| A blocking approver | **present; panics off a multi-threaded runtime** | `block_in_place` polled every 250 ms (`bots_drive_ipc.rs:175-209`; §12.4 *Risks*). |
| Linux CI | **absent** | Rust CI runs on `macos-latest`. The iOS check builds every member (`.github/workflows/ci.yml:90`). syncd's release job is the model for a Linux artifact (`.github/workflows/release.yml:229-322`, the file's end; the job itself is `:241-322`). |

## The one sentence

**keeper's only turn loop lives in a Mac-only crate and talks to its window, so an agent cannot run on the always-on server, move to the Mac when it must, or be reached by anyone but the person at the keyboard.** The fix has six parts:
- **one loop in a crate both hosts link** (90.1);
- **a sessions runtime that cannot corrupt itself** (90.2);
- **a headless host** with its own engine and secrets (90.3);
- **a lean Matrix client** (90.4);
- **a daemon that answers in rooms** and writes the log (90.5);
- **claims, manifests and placement**, so exactly one host writes each session (90.6).

## The reviews' amendments

The two reviews of 2026-10-02 were accepted in full (rulings R28 and R29). Where each finding lands in this epic:

| finding | what changes | where |
| --- | --- | --- |
| F1 | A proxy conversation's room (session kind `main` or `conversation`) lets people send `m.room.message` and `dev.keeper.agent.scope` at power level 0; every other session room keeps people at 0 for decisions, `heard` and surface results only. Both shapes are asserted. | 90.4 |
| F2 | `keeper_agent::agent::SessionContext { messages, memory_snapshot, label, … }` is loaded once per (session, claim) by `replay` and appended by the writer; a served session's second turn opens no file under `log/`. | C2, 90.5 |
| F5 | `invite_decision`: a session room is joined on invite from a known agent user or a proxy's person; any other invite stays pending. | 90.5 |
| F7 (R25) | The status event carries the session's `kind`; `kind` gains `conversation`. | 90.4 |
| F10 | NFR-113 is asserted in the Synapse harness (p95 over ≥ 50 turns); tuwunel's numbers are the published figure. | 90.5 |
| F12 | `keeper_agent::host::UNATTENDED_REFUSAL` is defined, with its exact text, and returned as the tool result. | 90.1, C4, 90.5 |
| F15 | The iOS check's one form; `agentd.toml`'s `[[mcp]]` and `[[kvm]]` follow R24. | 90.3, 90.5 |
| F16 | Drifted code citations corrected against this worktree. | triage, 90.1, 90.3 |
| F21 | agentd's system unit names its hardening directives. | 90.5 |
| F22 | Committed records cited by path; the header lists the deferred items. | header, *What stays out*, *Sprint-status entry* |
| F24 | DW-366 is merged into DW-395. | *What stays out* |
| F25 | `RequestConfig::disable_retry` is the retry switch, settled; the send calls return builders. | 90.4 |
| S-04 | Before every model request, the turn asks `label.may_use_model(local)`. | 90.5 |
| S-05 | A taker re-reads the claim after a settle; every line carries the claim id; a conflicted session is served by no host. | 90.6 |
| S-06 (F3) | A `[[trust]]` pin is written by a person after comparing fingerprints, never by keeper. | 90.3 |
| S-07 | Systemd credentials come first; agentd scrubs its secret variables and is not dumpable. | 90.3, 90.5 |
| S-09 | The per-turn integrity reset for proxy conversations hooks into `SessionContext::on_user_line`; the rule is 92.6's. | 90.5 |
| S-15 | `agentd.toml`'s `[[drives]]` pins `readers` and `owner`; the mount rule runs on the pins before any checkout; a differing `_drive.toml` hosts nothing. The desktop pins at sign-in. | C8, 90.3, 90.6 |
| S-16 | Tool progress on the status anchor carries counts, never paths; the `check_sink` rows are 92.6's. | 90.5 |
| S-17 | `SessionWriter` writes through 89.5's redacting `ChunkWriter`. | 90.5 |
| S-19 | keeper-agent and agentd register no observability sink; `check:agentd-lean` forbids the OTLP and PostHog crates; the desktop never exports an agent host's records. | 90.5, 90.6 |
| S-20 | Live tests read the provider URL from the environment. | 90.5 |
| S-25 | The claim content gains `window`, the scheduled-card window the holder runs. | 90.6 |
| S-26 | `docs/agents.md` says what the store passphrase protects; `LoadCredential` is the way to hand it over. | 90.3 |
| S-30 | agentd's release artifacts are minisign-signed with the app's key. | 90.5 |
| S-33 | The host manifest publishes bot ids, never provider base URLs. | 90.6 |
| S-34 | agentd's secrets directory must be `0700`, owned by its user and not a symlink, and no secret file may be a symlink. | 90.3 |

Every other finding of the two reviews lands outside this epic.

## Requirements

Copied from the architecture's table, as amended after the reviews (2026-10-02). This epic binds these rows and allocates none.

| id | statement | story | AD |
| --- | --- | --- | --- |
| FR-777 | The turn loop, the drive tool host and the task runner live in `keeper-agent`; typed, spoken and scheduled bot turns in the app behave exactly as before. | 90.1 | AD-367 |
| FR-778 | Creating, archiving and writing sessions is safe when several things do it at once: one plan at a time per zone, an interrupted plan resumed at start, and a create retried with the same id makes one session. | 90.2 | AD-368 |
| FR-779 | A Linux host is set up from `agentd.toml` plus secrets from the environment, systemd credentials or `0600` files, runs as its principal's own OS user with its own sync engine, and mounts only drives every reader of its homes may read. | 90.3 | AD-375, AD-376, AD-377 |
| FR-780 | Each copy of an agent signs in to the homeserver as the agent's user with its own device and creates rooms, invites, sends and edits messages, sends custom and state events and receives events, without the messenger's archive or notifications. | 90.4 | AD-370, AD-371 |
| FR-781 | `keeper-agentd init`, `login`, `agents list`, `run` and `status` (90.5) and `agents init` (91.5): a Linux host answers in an agent's rooms, streams the answer as edits, and writes the session's log. | 90.5, 91.5 | AD-370, AD-372, AD-373, AD-375 |
| FR-782 | Every host publishes what it can do and which drives it has; every session has exactly one owning host by claim; a session no live host can serve shows `waiting:` and what it waits for. | 90.6 | AD-374, AD-378, AD-379 |
| FR-783 | When the owning host disappears, another eligible host takes the session over after the claim expires, and two hosts never write one session at once. | 90.6 | AD-378 |
| NFR-112 | **Matrix is fast enough to be the only wire.** The p95 delay from an agent's send to the other copy's event handler is measured on tuwunel over the tailnet (1 000 events, published in `docs/agents.md`) and is at most 1 s; above 1 s is AD-370's revisit trigger, reported, not hidden. | 90.4, 90.5 | AD-370 |
| NFR-113 | **The answer starts quickly and grows smoothly.** p95 over ≥ 50 turns: the anchor appears within 1 s of the request reaching the owning host; the first streamed text appears within 400 ms plus delivery after the provider sends it; edits are never closer than 400 ms, and further apart when the homeserver asks; the final edit lands within 1 s of the stream's end unless the homeserver asks the sender to wait, and it is always delivered. 90.5's harness asserts the anchor and final-edit bounds against the Synapse test homeserver; the figure on tuwunel is published in `docs/agents.md` § Measured. | 90.5, 91.1 | AD-373 |
| NFR-115 | **No byte crosses principals.** No content from a drive reaches a process, room, drive, memory file, MCP server, KVM or command whose audience is not within that drive's readers, except by a recorded declassification, and none reaches a model that is not local while the session's label is `local_only`. A model provider is a processor the person chose for the agent, not an audience (D-34). Proved per sink (send, invite, status and scope edits, delegate, write, propose, promote, MCP, KVM, `run`, model calls including embeddings and review-layer helpers) by tests that try. | 89.4, 90.3, 92.6 | AD-377, AD-390, AD-391 |
| NFR-116 | **A log never becomes a large file, and is never read on the hot path.** No chunk reaches `min(192 KiB, 3/4 × lfs_threshold_bytes)`; no line exceeds 64 KiB; a body over 16 KiB is a blob; the board, the list and a turn read the index and the in-memory context only: the second turn of a served session opens no file under `log/` (AD-366's `SessionContext`). | 89.5, 90.5 | AD-366 |
| NFR-117 | **A crash loses nothing and repeats nothing.** A torn last line is truncated on open and the rest reads; a turn's lines are `fsync`ed at its end; an interrupted session plan resumes at start; an approval is consumed at most once across crashes, restarts and takeovers: the owning host acts only after the homeserver has accepted its `dev.keeper.agent.approval.consumed` event and that event is the first for the approval in the room, and a host resuming a session reads the room before its own log. | 89.5, 90.2, 93.2 | AD-366, AD-368, AD-394 |
| NFR-120 | **One writer per session.** At most one host writes a session per epoch; a holder that cannot renew stops writing at least 60 s before another host may take over; every line carries the epoch and the claim event it was written under, and a late line from a superseded epoch is dropped by every reader; two hosts that acquired one epoch with different claim events mark the session conflicted, and nothing replays it until a person resolves it. | 89.5, 90.6 | AD-378 |
| NFR-121 | **No destination the person did not configure.** The agents add only the homeserver, provider base URLs, MCP servers, KVMs and the push gateway the person configured, each derived into the egress list (AD-53) and diffed at release; a networked `run` reaches only what its approval shows, and a `docs/egress.md` row says so. `keeper-agent` and `keeper-agentd` register no observability sink; the desktop's export never reads a span or event under the targets `keeper_agent` and `keeper_core::agents`, pinned by a test; `check:agentd-lean` forbids `opentelemetry*` and `posthog*` crates. | 89.6, 90.5, 90.6, 96.1, 96.2, 96.5, 98.1 | AD-369, AD-375, AD-405, AD-406, AD-409, AD-412 |

**This epic's share of a shared NFR.**
- NFR-113: 90.5 proves pacing and the final edit, and asserts the anchor and final-edit bounds over ≥ 50 turns on Synapse (acceptance 21); tuwunel's figure is the operator's recorded run. The person's rendering is 91.1's.
- NFR-115: 90.3 proves the process layer and the mount rule on pinned readers; 90.5 refuses a remote model while the label is `local_only` (acceptance 18), which 92.6 folds into `check_sink`.
- NFR-116: 90.5 proves the `SessionContext` half (acceptance 16); 89.5 proves the chunks and the index.
- NFR-117: 90.2 proves plans. 90.5 adds "repeats nothing" for turns (acceptance 9 there).
- NFR-120: 90.6 proves the claim, the settle and the conflicted session; 89.5 proves the line-level fence.
- NFR-121: 90.5 proves agentd's destinations and its lean dependency tree; 90.6 proves the desktop's telemetry filter.

**Held, not restated:** AD-24, AD-40, AD-52, AD-53 (the homeserver URL and provider base URLs are derived into egress, already kind-agnostic), AD-62 (one clock per host process), AD-158, D-3.

## Choices this plan makes where the architecture left room

Each is flagged for the coordinator; none is silent.

- **C1 — `TurnOrigin` lands with three arms; `Agent` comes with its caller.**
  - 90.1 adds `TurnOrigin { Typed, Spoken { language }, Task }`, as D1 §4 lists. `Agent { session }` (AD-367's fourth arm) is added in 90.5 with its first caller: an unused arm in a no-behaviour-change rung is dead code. Epic 91–93 cites it as 90.5's.
  - **The shell computes the origin at each entry from `voice_ipc::spoken_turn(dir)`.** That is exactly what `arm_turn` reads today (`bots_ipc.rs:1175`), because spokenness is the voice turn's answer, not the entry point's. **As built (ruling D7):** the shell passes an `origin_of` closure that `arm_turn` calls at that same point, so the read keeps its timing (after the endpoint, the identity probe and the row writes).
- **C2 — agent turns do not go through `open_turn`.**
  - `open_turn` stores and replays `keeper.db` rows (AD-154 kept for ⌘9).
  - An agent turn (90.5) uses `turn::arm_turn(origin: Agent{session})` and `run_tool_loop_reporting`, as the task runner does. Its lines go to the log.
  - **Its history is held, not re-read** (F2). `keeper_agent::agent::SessionContext { messages, memory_snapshot, label, … }` is loaded once per (session, claim) by 89.5's `replay` — when the host opens the session, takes it over or restarts — and the writer appends every line it writes to it. A turn reads the context, never a chunk: AD-365's and NFR-116's "never re-read a log on the hot path".
  - So no `Transcript` port is needed, and the ⌘9 path is untouched.
- **C3 — `GrantSource`, a fifth port, in 90.5.**
  - **The gap.** AD-377 says agents get their grants from `[tools].drives` through the host, but `grant::check` reads `keeper.db` rows keyed by (provider, bot) (`grant.rs:805-830`). Two agents on one CLIProxyAPI model would share them.
  - **The fix.** 90.5 adds `keeper_agent::grants::GrantSource { fn grants(&self) -> Result<Vec<Grant>, AgentError> }`, called per tool call.
  - **Its two implementations.** `StoreGrants` is the existing per-call `grant::check` semantics, for the shell and ⌘9. `AgentGrants` derives `GrantScope::Profile` grants from the agent's `[tools].drives` ∩ the session's current scope: write when `drive_write`/`drive_edit` are allowed, else read.
  - **The shell's construction moved with `arm_drive` in 90.1**, so 90.5 does not touch the shell.
- **C4 — before Epic 93, every ask is refused.** The agent's `ApprovalPort` answers `false`, and the tool result the model reads is `keeper_agent::host::UNATTENDED_REFUSAL` (90.1 defines its text, F12). Today `bots_tools.rs:110-114`'s `ask` returns a bare `false` and the words exist only in the module doc (`:43-45`), so 90.1 gives them a constant. A write under a profile-wide grant, which AD-158 makes an Ask, is refused, and the refusal is in the log.
- **C5 — epoch 0 before claims.** 90.5 writes `epoch: 0` with no `claim` id (89.5's A3) and runs on one host only (agentd). 90.6 starts epochs at 1. The reader's fence (89.5) treats 0 as the era before claims.
- **C6 — an interrupted turn is not re-run.**
  - After a crash, a session whose last `user` line has no `assistant` after it gets an `error` line, and a new room message: "My answer was cut off when electra restarted. Ask again if you still need it." That is NFR-117's "repeats nothing": tool calls may already have had effects.
  - DW-365 records the alternative.
- **C7 — secrets.**
  - agentd's secret files live in `$XDG_STATE_HOME/keeper-agentd/secrets/`, as *Data formats* says, while syncd's stay in `$XDG_CONFIG_HOME/keeper-sync/secrets/` (`platform.rs:163-166`). `keeper_sync::xdg::SecretStore` takes the directory, so both hold.
  - **agentd's order is systemd credentials first** (S-07): `$CREDENTIALS_DIRECTORY`, then `KEEPER_AGENTD_SECRET_*`, then the `0600` file. Once agentd has read its secrets it removes every `KEEPER_AGENTD_SECRET_*` from its own environment and makes itself non-dumpable, before any thread or child starts (90.3, 90.5). Its secrets directory must be `0700`, owned by agentd's user and not a symlink (S-34).
  - `$CREDENTIALS_DIRECTORY`, the directory check and the scrub are agentd's only. syncd's behaviour is unchanged.
- **C8 — the mount rule runs on pinned readers, before any checkout** (S-15, superseding the first plan's "after the first checkout").
  - `_drive.toml` is a file every reader of the drive can edit, so it cannot be the root of the label lattice. Each `agentd.toml` `[[drives]]` entry pins the drive's `owner` and `readers`, written by the operator from the forge's collaborator list (the real access list).
  - The mount rule (AD-377) compares the pins, so a drive that fails it is never fetched: agentd exits `2` naming the readers missing, before any checkout and before any agent runs.
  - After the checkout, a zone whose `_drive.toml` names different readers or a different owner from the pin hosts nothing, and `status` and `agents list` name each difference. The pin is never rewritten from the file.
  - DW-368, which recorded the bytes a misconfigured drive put on disk before the check, is closed by this.
- **C9 — the claim read-back** (the architecture's Ambiguity **5**; the coordinator's scope called it 7, which is the control-metadata ambiguity, also honoured in 90.6).
  - A takeover reads the session room's current state from the server with `Client::send(ruma::api::client::state::get_state_events::v3::Request::new(room_id))`, which is `GET /_matrix/client/v3/rooms/{roomId}/state`.
  - It selects the `dev.keeper.agent.claim`/`""` event and proceeds only if its `event_id` is the one `Room::send_state_event_raw` returned. That event's `origin_server_ts` is the claim's server time.
  - **Then it settles and reads again** (S-05). Two hosts can each read back their own write within one round trip. So the taker waits a settle — twice the longest round trip it has measured to the homeserver, or one completed `/sync` round, whichever is longer — reads the state from the server once more, and proceeds only if the claim is still its own event; otherwise it yields. Every line it then writes carries the claim's epoch and event id (89.5's A3), and two `acquired` lines at one epoch mark the session conflicted.
  - AD-371's "read state, cached" path therefore has one named exception, the claim read-back (F25; the architecture states it).
- **C10 — handing back.**
  - P6 owns Nixi's main session by the always-on host, and lets hesperia take it over only after expiry. AD-379 does not say what a holder does when placement later prefers another live host.
  - **The plan's reading:** the holder releases the claim (`released: true`) at its next idle moment, with no turn running, so electra takes Nixi back when it returns. Q3 asks.
- **C11 — a desktop copy needs a Matrix device, and the person signs it in.** 90.6 ships `agents_copy_sign_in` and one Settings › Agents row (UX-DR128). 91.5's *Set up agents* seeds zones and points at that row, as agreed with Epic 91's lane.
- **C12 — FR-781's `agents init` is built in 91.5**, agreed with Epic 91's lane. A verb that seeds nothing would be a stub, and the seeding (README, AGENTS.md, `_drive.toml`, `_template`, the three agents, the proxy's DM) is FR-788's. 90.5 builds `init`, `login`, `agents list`, `run` and `status`.

## Open questions for the coordinator

Each has the reading this plan builds to, so no lane is blocked.

- **Q1. A fifth port, `GrantSource` (C3).** **Settled by R27:** it lands in 90.5 (grants per agent), and AD-367's list is amended.
- **Q2. FR-781 names `agents init`, but 91.5 builds it (C12).** **Settled by R25 and R27:** `agents init` is 91.5's; FR-781's story column reads "90.5, 91.5".
- **Q3. Hand-back (C10).** **Plan's reading:** the holder releases when it is idle and placement prefers another live host. DW-369 records the alternatives.
- **Q4. Who the desktop hosts for.** **Settled by R27:** the agents whose drive's `principal` equals the signed-in account's login, AD-377's "that person".
- **Q5. A turn cut off by a crash (C6).** **Plan's reading:** never re-run. The person is told, and DW-365 records the alternative.
- **Q6. The claim read-back.** The coordinator's scope cites "arch ambiguity 7" for the server read-back. In the architecture that is Ambiguity **5**; Ambiguity 7 is that control metadata is not labelled. Both are honoured: C9, and 90.6's acceptance 8.

## Stories

Conventions as Epic 89's *Stories*:
- every shell-touching story is gated by `bun run check:rust:macos` on hesperia and named in its PR as awaiting CI's macOS job;
- new behaviour tests are mutation-proved;
- bindings are regenerated, never hand-edited.

**Live tests** are `#[ignore = "live: …"]` integration tests that read their endpoints from the environment, never from a file in the repository. Each names its run command. Their secrets are read from files and never printed.

**Operator actions** are named as such, with the exact commands. Nothing outside this repository is assumed done.

### 90.1 — Extract `keeper-agent` (turn loop, drive tool host, task runner; TurnSink/ApprovalPort/VaultWriter/ProfileSource; TurnOrigin) — no behaviour change

**Intent:** "I dont want sidecar. I want my mac, iphone, sever on linux to use it." **Rung:** **epic90-extract**. AD-367; FR-777.

**Files (the move list is D1 §4's, verbatim):**
- **New `src-tauri/crates/keeper-agent/`.**
  - `Cargo.toml`:
    - deps `keeper-core`, `keeper-sync`, `keeper-ported`, `tokio`, `tracing`, `ulid`, `reqwest` (workspace);
    - no tauri;
    - `[lints] workspace = true`.
  - `src/lib.rs`.
  - **`src/ports.rs`**, with the ports exactly as D1 §4 prints them:
    - `TurnSink { fn event(&self, e: BotStreamEvent) -> bool; fn request_sent(&self) {} fn answer_text(&self, text: &str) {} fn ended(&self, end: TurnEnd) {} }` — `answer_text` carries model prose only, so the round separator never reaches the spoken turn's segmenter (ruling D8);
    - `TurnEnd { Complete, Stopped, Failed(String) }`;
    - `ApprovalPort { fn ask(&self, req: BotApprovalRequestVm, signal: &CancelSignal) -> bool }`;
    - `VaultWriter { fn subfolder(&self, profile_id: &str) -> Option<String>; fn write(&self, profile_id: &str, rel: &str, text: &str) -> Result<(), String> }`;
    - `ProfileSource { fn profiles(&self) -> Vec<SyncProfile> }`.
  - **`src/turn.rs`**:
    - `Turn` (from `bots_ipc.rs:974`, without `spoken`/`bot_name`), `Armed`, `arm_turn(env, row, bot, model, messages, origin: TurnOrigin)`, the bodies of `open_turn` and the retry;
    - `TurnOrigin { Typed, Spoken { language: String }, Task }`;
    - the helpers `endpoint_of`, `read_timeout_of`, `capabilities`/`session_caps`/`cached_caps`, `adopt_identity`, `discovered_model`, `store_message`, `replay`, `attach_staged_images`, `now_ms`, `new_id`, `finish_word`;
    - `AgentError`;
    - the token from `resolve_credential(platform, http, account: Option<&AccountDescriptor>, …)`.
  - **`src/drive.rs`**: `drive`, `close`, `close_failed`, `emit_closed`, `emit_context`, `FLUSH_BYTES` (512), `spawn_turn` over `tokio::spawn`, `LiveStream` with a tokio `JoinHandle`, `streams`, `owns_turn`, `stop(subscription_id)`.
  - **`src/host.rs`**:
    - all of `bots_tools.rs` (`DriveToolHost`, `perform`, `write_through` with 89.3's `.with_agents`, `load_context`, `limits`, `Approver`);
    - **`pub const UNATTENDED_REFUSAL: &str = "This needs a person's approval, and there is no one here to ask, so keeper did not do it. Nothing was changed.";`** (F12). A host built with no approver answers every Ask with it: `DriveToolHost::run` returns `BotsError::GrantDenied { reason: UNATTENDED_REFUSAL }`, which the tool loop turns into the tool result the model reads (`bots/tools.rs:1404-1416`), and the audit row closes `refused`. A host whose approver says no keeps the grant layer's own sentence, as today. Every later story that refuses an action for want of a person (90.5, 92.1, 92.3, 93.2, 93.4) cites this constant;
    - `ArmedDrive`/`TurnHost` (`bots_ipc.rs:1015-1037`);
    - `NoDrive` without its `cfg`;
    - `DesktopDrive` → `DriveTurnHost`;
    - `arm_drive`, today `arm_drive(state: &AppState, grants: &[Grant], offered: bool)`, which reads the profiles from the app state inside (`bots_drive_ipc.rs:89-133`), moved as `arm_drive(profiles: &dyn ProfileSource, grants, offered)`.
  - **`src/task.rs`**: all of `bot_task.rs`, with `prepare` rebuilt over `turn::arm_turn(origin: Task)`, and its tests (`UnusedPlatform`, `:492`).
- **Shell.**
  - `keeper/Cargo.toml` depends on `keeper-agent`.
  - `lib.rs:27-48` drops `mod bots_tools;` and `mod bot_task;`.
  - **New `keeper/src/agent_ports.rs`**:
    - `ChannelSink(Channel<BotStreamEvent>)`;
    - `SpokenSink`, holding a `Segmenter` and the voice calls now inline at `bots_ipc.rs:1764-1795` and `:1946-1957`;
    - `EventSink(AppHandle)`, for `send_spoken`, removing its serialise/deserialise round trip (`:1358-1370`);
    - `ChannelApprover`, the existing `asks()`/`approver`, `bots_drive_ipc.rs:147-209`, unchanged, `block_in_place` included. **As built:** it is `keeper_agent::approval::SinkApprover` over an `Arc<dyn TurnSink>`, with `approval::answer` behind `bots_approval_answer`; every entry point builds it over the sink its turn streams into, so a spoken turn's ask still goes down `SPOKEN_STREAM_EVENT` to the pane's sheet, as before the move;
    - `NotesVaultWriter`, over `notes_vault`;
    - `EngineProfiles`, over `sync::engine`.
  - **`bots_ipc.rs`**: `bots_chat_send`, `bots_message_retry`, `send_spoken`, `bots_chat_stop` and `bots_session_follow` call `keeper_agent`. Each computes `TurnOrigin` from `voice_ipc::spoken_turn(dir)` (C1). `bots_error` (`:149`) maps `AgentError`.
  - **`bots_drive_ipc.rs:58-60`** imports.
  - **`sync.rs:52-57`** installs `keeper_agent::task::TaskRunner` with the shell's ports.
- **Gates and docs.**
  - `src-tauri/Cargo.toml:3` gains the `keeper-agent` member.
  - `package.json` gains `check:agent-tauri-free` (`tree=$(cargo tree … -p keeper-agent -e normal,build --prefix none) && ! printf '%s' "$tree" | grep -qE '(^|[[:space:]])(tauri(-[a-z]+)*|wry|tao|gtk|glib-sys|webkit2gtk[a-z0-9-]*) v'`), appended to `check` (`:32`).
  - `lefthook.yml:47` gains `-p keeper-agent`.
  - `keeper-sync/src/platform.rs:391-398` says the runner is `keeper_agent::task` over `arm_turn(origin: Task)`, which is now true.

**Acceptance:**
1. **The crate is tauri-free, and the guard bites.** `bun run check:agent-tauri-free` passes. Mutation: adding `tauri` to `keeper-agent/Cargo.toml` fails it.
2. **It builds and tests on Linux.** `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-agent` is green on this host. That settles research §14's "keeper-agent builds on Linux". The lefthook fallback lints it.
3. **Moved tests keep their names and assertions.** Every test from `bot_task.rs` (`bots_tools.rs` has none, ruling D6) and `bots_ipc.rs`'s `replay` test runs in `keeper-agent` with an unchanged body (only `use` paths change), now on Linux. The PR lists them side by side.
4. **The origin changes nothing.**
   - `a_spoken_origin_adds_the_answer_instruction_after_the_context_and_a_typed_one_adds_nothing`: the request's system message is the context prompt, then `speech::answer_instruction(language)` (`bots_ipc.rs:1179-1188`'s order), and `Typed` adds nothing.
   - The shell's three entry points pass `Spoken` exactly when `spoken_turn(dir)` answers `Some`, so a typed send while the voice turn awaits its send is still spoken, as today. (Shell, by inspection.)
5. **Characterisation goldens from the code before the move.** The risk is a moved line that changes behaviour, and only hesperia runs the old code.
   - **How the goldens are captured.** The rung's first commit adds a throwaway shell test that drives three scenarios against a localhost OpenAI-shaped stub on hesperia:
     - a typed turn with one `drive_read` under a subtree read grant;
     - the same turn spoken (`Spoken{"pl-PL"}`);
     - a `TaskKind::Bot` run.
   - It writes, per scenario, the request bodies the stub received, the `BotStreamEvent` kinds in order, the `bot_messages` rows (role, partial, finish_reason, tool_call_count, content) and the `bot_audit` rows (tool, effect, verdict, outcome), with ids and times masked, to `keeper-agent/tests/fixtures/characterisation/*.json`.
   - The second commit moves the code and deletes the capture test.
   - `the_extracted_turn_reproduces_the_goldens` (keeper-agent, Linux) drives the same scenarios through the moved code and the stub, and must equal them.
   - The PR records the capture commit and the hesperia run.
6. **The crash contract and the flush cadence hold.** In the characterisation's typed scenario:
   - the assistant row exists with `partial = 1` before the stub receives the request (`bots_ipc.rs:1276-1284`);
   - content is flushed at 512-byte boundaries;
   - a `Stopped` end keeps the partial text.
7. **The names hold.** `#[tauri::command]` names (`lib.rs:949-1004`, `:1553-1573`) are unchanged, and `bindings:check` produces no TypeScript diff.
8. **iOS still builds.** `cargo check --workspace --target aarch64-apple-ios` (`ci.yml:90`) passes with `keeper-agent` a member (R10). (90.5 adds `--exclude keeper-agentd`; that is the form every later story quotes.)
9. **hesperia, end to end.** `bun run check:rust:macos` is green. On the installed build, before and after the rung, on the same `~/Library/Application Support/dev.tgorka.keeper/keeper.db` (the bundle id, `src-tauri/crates/keeper/tauri.conf.json:5`):
   - a typed ⌘9 turn with a drive read, a spoken turn, and a `TaskKind::Bot` *Run now*;
   - then `sqlite3 keeper.db "select role, partial, finish_reason, tool_call_count from bot_messages order by rowid desc limit 6"` and `"select tool, effect, verdict, outcome from bot_audit order by id desc limit 3"`, and the task's `task_runs` row;
   - the two runs are identical modulo ids and times.
   - research §14's "`tokio::spawn` inside tauri commands" is settled by this run.
10. **The one named change: the unattended sentence** (F12). This rung changes no behaviour but this, and its PR names it. `a_host_without_an_approver_refuses_an_ask_with_the_unattended_sentence` (keeper-agent, real fixture drive): a `drive_write` under a profile-wide write grant (an Ask, AD-158) on a host built with no approver gives the model a tool message whose content is exactly `"Refused: " + UNATTENDED_REFUSAL` (the tool loop's `render_result` prefix, `bots/tools.rs:1009`; ruling D9), changes no byte on disk, and closes its audit row `refused`; the same call on a host whose approver answers no gives the grant layer's sentence, unchanged. Mutation: returning the grant's sentence on the no-approver path fails it. None of acceptance 5's characterisation scenarios asks, so the goldens hold.

**Shell crate:** yes, every call site. Awaiting CI's macOS job and `check:rust:macos`.

**binds:** FR-777, AD-367

### 90.2 — The sessions runtime moves into keeper-agent, made safe (zone mutex, resume, caller id)

**Intent:** "Use sessions in the drives as a working place of the agent … (active/archived)". **Rung:** **epic90-extract**. AD-368; FR-778; NFR-117 (its share); research §13 #13–#15.

**Files:**
- **New `keeper-agent/src/sessions/`.**
  - `exec.rs`: all of `sessions_exec.rs`, with its tests (`:380-814`).
  - `verbs.rs`, holding:
    - the bodies of `sessions_create` (`sessions_ipc.rs:851-1162`), archive (`:1646-1689`), delete (`:1707`), unarchive (`:1742`), the file verbs (`:3437-3964`) and the task move (`:4049-4135`);
    - `taken_names` (`:824`), `pattern_files` (`:530`), `flat_kinds` (`:588`), `named_templates` (`:625`), `today`/`now_hhmm` (`:486`, `:3263`).
  - `scan.rs`: `register_one` (`sessions_root.rs:143`), `scan_zone` (`:326`), `session_dirs`, `row_for` (`:468`), `walk_freshness`, `read_zone_spaces`, `read_session_pool`, `read_ref_sources`, `markdown_rels`, `scans_markdown`.
  - `lock.rs`: `ZoneLock`.
  - `write.rs`: `session_write`.
- **Shell.**
  - The `sessions_*` commands keep their names and become calls.
  - `sessions_root.rs` keeps `spawn_scanner` (`:172-205`), `start_tap` (`:208-248`) and `refresh` (`:103`) behind a port, `ScanNotifier`.
  - `sessions_exec.rs` is deleted.
  - App start calls `keeper_agent::sessions::resume_all` for every sessions zone before the first verb.
- **Dependencies.** `keeper-agent/Cargo.toml` gains `fs4` (workspace, `src-tauri/Cargo.toml:103`, already used for marker locks in `keeper-sync/src/lfs/store.rs:355`).

**The guarantees:**
- **One plan at a time per zone.** `ZoneLock::acquire(zone)` takes a process-wide `Mutex`, keyed by the zone's canonical root, and an `fs4` exclusive lock on `<zone>/.keeper/sessions.lock`, so a second process on the machine waits.
- **Resume at start.** `resume_all(zones)` runs before any verb, on both hosts.
- **The caller supplies the id.** `create(CreateReq { id: Ulid, … }) -> Created | Existed`, found by the README record's id in `active/` and `archive/`.
- **A verb finds a session by id.** The `session_id` comes from the plan's result, not the 400 ms scan.
- **Containment is canonical.** Every step's target is joined through `keeper_sync::browse::resolve` under the zone root as well as lexically (`rel()`, `exec.rs:303-314`).
- **`session_write(session, rel, content)`** creates or replaces through the journaled executor:
  - under `artifacts/`, with `files::compile_new`'s extensions;
  - under `workspace/`, any extension;
  - or a card.
  - It refuses `log/`, `approvals/`, `agent.toml`, `README.md` and `AGENTS.md`.
  - It is the function agents' `session_write` tool and R23's answer artifact use. The tool is registered with the agent tool host when offered (90.5 offers the drive tools only; Epic 94).

**Acceptance:**
1. **Two plans never interleave.** `two_plans_on_one_zone_never_interleave`: two threads, each running a 40-step plan on one real zone, released by a barrier.
   - Neither journal ever names the other plan's steps, and the tree holds both results.
   - It fails on today's code: the second `run` resumes the first plan (`exec.rs:49-52`). Mutation: removing the mutex fails it.
2. **A second process waits.** `a_second_process_waits_on_the_lock_file`: the test re-executes its own binary (`std::env::current_exe`, with an env flag) to hold the lock for 500 ms. The parent's plan starts only after the child's ends, by timestamps.
3. **An interrupted plan resumes at start.** `an_interrupted_create_is_resumed_at_start`: a journal with `done = 2` of 5 is completed by `resume_all` and the journal cleared. The existing crash-resume tests move and pass.
4. **A retried create makes one session.** `a_create_retried_with_the_same_id_makes_one_session`:
   - two creates with one id give `Created`, then `Existed`;
   - there is one folder;
   - a retry with a different title returns the existing session unchanged.
5. **A verb finds a just-created session.** `a_verb_right_after_create_finds_the_session_by_id`: archive straight after create succeeds with no scan in between.
6. **A symlink cannot escape.** `a_symlinked_session_folder_cannot_redirect_a_step_outside_the_zone`: a real symlink `active/s/artifacts → /tmp/elsewhere` is refused. It fails before the canonical join (§13 #15).
7. **`session_write` refuses keeper's files.** `session_write_refuses_log_approvals_and_keepers_files`. It accepts `artifacts/answer-01J….md` and `workspace/run.jsonl`, and refuses `log/x.jsonl` and `artifacts/x.jsonl`.
8. **Agent files are invisible to the flat contract.** `a_session_with_agent_files_scans_to_the_same_row_and_pool` (89.5's acceptance 10, the scan half): adding `log/*.jsonl`, `log/blobs/*.json`, `approvals/*.json` and `agent.toml` to a fixture session leaves its `SessionRowVm` and pool unchanged, and adds no untagged residue.
9. **The module doc's two claims become true.** The doc at `exec.rs:4-10` is now true, and the PR quotes both claims beside the tests that prove them (acceptances 1 and 3).
10. **The desktop is unchanged.** The shell's sessions tests pass on macOS. `sessions_create` passes its minted id (`:873`) as the caller id. `bun run check:rust:macos` is green. Smoke on hesperia: create, archive and unarchive a session in a scratch zone, and quit mid-create (`kill -9` between steps, timed by a debug sleep in a dev build). The next start resumes it.

**Shell crate:** yes: `sessions_ipc.rs`, `sessions_root.rs`, `lib.rs`.

**binds:** FR-778, NFR-117, AD-368

### 90.3 — `keeper_sync::xdg` + headless platforms + `agentd.toml`

**Intent:** "sever on linux"; "i own the server infrastructure and tgrive is only for tgorka". **Rung:** **epic90-agentd**. AD-375 (its configuration), AD-376, AD-377 (the process layer and the mount rule); FR-779; NFR-115 (its share); rulings R7, R8, R17; S-06, S-07, S-15, S-26, S-34, F15 (R28, R29).

**Files:**
- **New `keeper-sync/src/xdg.rs`.**
  - `XdgDirs { config, data, state }`, with `XdgDirs::resolve(app: &str) -> Result<XdgDirs>` and `with_dirs`. The rules move from `keeper-syncd/src/platform.rs:173-201`: an empty or relative `XDG_*` falls back to `$HOME`, and a missing `HOME` gets its sentence.
  - `SecretStore { env_prefix: &'static str, credentials_dir: Option<PathBuf>, dir: PathBuf, strict_dir: bool }`, with `get`/`set`/`delete`.
    - **The lookup order** (S-07): with a credentials directory (agentd), `credentials_dir` first, then the environment, then the `0600` file; without one (syncd), the environment, then the file, as today.
    - **`strict_dir`** (agentd only, S-34): the secrets directory must be mode `0700`, owned by the process's effective uid and not a symlink, and a secret file that is a symlink is refused (opened with `O_NOFOLLOW`). The facts are checked by a pure `check_secrets_dir(DirFacts { mode, uid, is_symlink }, euid)` and read from the real directory.
    - `scrub_env(&self)` removes every variable starting with `env_prefix` from the process's environment, so no child the process starts later inherits one.
  - `env_var_name`, `secret_file_name`, `trim_secret` and `check_secret_permissions` move verbatim (`:203-257`), with their tests.
- **`keeper-syncd/src/platform.rs`** uses them, with no behaviour change: app dir `keeper-sync`, prefix `KEEPER_SYNC_SECRET_`, secrets under `$XDG_CONFIG_HOME/keeper-sync/secrets/`, no credentials dir.
- **New `keeper-core/src/agents/agentd.rs`**, the `agentd.toml` grammar of *Data formats*. `AgentdConfig::parse(text) -> Result<AgentdConfig, ConfigRefusal>`:
  - every credential is `secret:<name>`;
  - `[[agents]].drive` must be a `[[drives]].id`;
  - `kind` is a registry string;
  - `base_url` goes through `bots::url::parse_base_url`;
  - the `host` slug is `[a-z0-9-]{1,32}`;
  - an unknown key is refused naming it.
  - **`[[drives]]` pins `owner` and `readers`** (S-15), both required, checked as `_drive.toml`'s are (Matrix ids, the owner among the readers, readers reported sorted). The operator writes them from the forge's collaborator list for the drive's repository, which is the real access list; keeper never rewrites them from the drive.
  - `[[trust]]` and `[[mcp]]` are parsed and validated now, and used by 93.3, 96.2 and 90.5.
  - In `[[trust]]`, `user` is required. `master_key` is optional, agreed with Epic 93's lane: when absent the user is "not pinned yet" and is never trusted. A pin is written by a person after comparing fingerprints, never by keeper (S-06, F3: R25's first-decision pinning is withdrawn). 93.3's `status` prints `matches`, `differs` or `not pinned` against the fingerprint the homeserver publishes, so the operator writes `user` first, compares the fingerprint with the person's device, then adds the key.
  - `[[trust]]` may name `proxy`, the Matrix user of that person's proxy agent, also written by a person. 90.5's `invite_decision` uses it to join a hand-off from a proxy this host does not host (F5).
  - `[[mcp]]` and `[[kvm]]` follow *Data formats* after R24(4) and F15: `[[mcp]]` has `name`, `url` or `command`, `credential`, `readers`, `role` and `fingerprint`; one `[[kvm]]` table owns each KVM's `readers`, `fingerprint` and credential, and an `[[mcp]] role = "kvm:<id>"` names a `[[kvm]] id`, refused when there is none.
  - Nothing in the file is written by keeper except `[homeserver].control_room` (90.5's `init`, through `toml_edit`).
- **New `keeper-core/src/agents/mount.rs`.**
  - `check(mounted: &[(String, Readers)], homes: &[String]) -> Result<(), MountRefusal>` is AD-377's rule over the **pinned** readers: every mounted drive's readers ⊇ the readers of every drive this process homes agents in. It runs before any checkout (C8).
  - `pin_matches(decl: &DriveDecl, pin: &DrivePin) -> Result<(), PinDifference>` compares a drive's `_drive.toml` with its pin; a zone whose declaration differs hosts nothing, and `PinDifference`'s sentence names each difference ("_drive.toml names the readers @marta, @tgorka, @x; this host pinned @marta, @tgorka"). A zone with no valid `_drive.toml` hosts nothing, as 89.2 says. 90.6 uses the same function on the desktop.
- **New `keeper-agent/src/headless/`.**
  - `HeadlessPlatform` implements `keeper_core::platform::Platform`:
    - `data_dir` = `$XDG_DATA_HOME/keeper-agentd`;
    - the keychain goes to `SecretStore`, through a `SecretMap` from keychain keys (`bot_provider_token/{p}`, `sync/<pid>/credential`, `agents/<user>/session`, `agents/<user>/sdk-passphrase`) to `secret:<name>` or a store file. The store passphrase is best handed over as a systemd credential (S-26);
    - `open_url` and `sidecar_path` are `Unsupported`;
    - `notify` logs at `warn`;
    - `exclude_from_backup` and `set_badge_count` are `Ok(())`.
  - `harden_process(store: &SecretStore) -> Result<(), HardenError>` (S-07): reads every secret agentd needs into memory, calls `scrub_env`, and sets `prctl(PR_SET_DUMPABLE, 0)`, so another process of the same user cannot read agentd's memory or its `/proc` files, and no child inherits a secret variable. Removing a variable is safe only while the process has one thread, so agentd's `main` calls it before it builds the tokio runtime (90.5).
  - `HeadlessSyncPlatform` implements `keeper_sync::platform::SyncPlatform` over the same, plus:
    - `now_ms` from `SystemTime`, `free_space` `None`;
    - `git_program` through `GitRequest::search`;
    - `host_label` = `agentd.toml`'s `host`;
    - `bot_task_runner` `None`, so `TaskKind::Bot` keeps `NO_BOT_RUNNER_SENTENCE`, `platform.rs:414` (AD-224/AD-226 unchanged, R9).
  - `open_engine(config, platform)`:
    - agentd's own `sync.db` under its data dir, beside a marker file `.keeper-agentd`. A `sync.db` without the marker is never opened (R7);
    - one profile per `[[drives]]`, its id stored by remote URL, `local_path` = `…/drives/<drive-id>/`, `direction` both, `lfs_mode` materialise;
    - `[folder.agents]` and `[folder.sessions]` armed;
    - `install_folder_tier(FolderTier::new(host, None))` called at start (`profile/folder.rs:792`; `FolderTier::new` is `:359`);
    - a virtual pattern that would cover the agents or sessions zone refused, naming the pattern (AD-376).
  - `apply_providers(config, data_dir)` upserts `bot_providers` rows idempotently, keyed by `ProviderRef` (`settings_sync.rs:558-584`). No secret enters `keeper.db`.
  - `enforce_mounts(config, engine)` applies C8: the mount rule on the pins before any profile is added, then `pin_matches` on each zone after its checkout.
- **Docs.** `docs/agents.md`, chapter *A Linux host*: the directories; the secret order, with systemd credentials (`LoadCredential=`) as the recommended way and environment variables as the fallback; that agentd scrubs its secret variables and is not dumpable; the secrets directory's rule; `agentd.toml`; the pins and the mount rule. It states plainly (S-26) that the Matrix store's passphrase protects a stolen data directory only when the secrets directory is not stolen with it, and that `LoadCredential=` keeps it outside agentd's own directories.

**Acceptance:**
1. **syncd is unchanged.** syncd's platform tests (`keeper-syncd/src/platform.rs:590-690`) pass. `check:syncd-lean` is green. The moved tests pass in `keeper_sync::xdg`.
2. **Secrets.**
   - `systemd_credentials_win_then_the_environment_then_a_0600_file` (agentd's store, S-07). Precedence is tried with all three present, then two, then one. syncd's store, with no credentials directory, keeps environment-then-file, and its existing tests are unchanged.
   - A `0644` file is refused with "it must be 0600 — run: chmod 0600 …" (`platform.rs:246-257`).
   - A `secret:<name>` with `..` cannot escape (`secret_file_name`).
   - The credentials dir is never read for syncd.
   - **The strict directory** (S-34): `a_secrets_directory_must_be_0700_owned_and_real` — on real files, a `0755` directory and a symlinked directory are refused naming the path and the reason, and a symlinked secret file is refused; over `check_secrets_dir`, a directory owned by another uid is refused. syncd's store, built without `strict_dir`, accepts the `0755` directory as today.
   - **The scrub and the dump flag** (S-07): `harden_leaves_no_secret_variable_and_no_dump` re-executes the test binary (`std::env::current_exe`, an env flag, as 90.2's acceptance 2) with `KEEPER_AGENTD_SECRET_X=v`. After `harden_process`, the store still answers `secret:x` with `v`, `std::env::var("KEEPER_AGENTD_SECRET_X")` is absent, a child it spawns (`env`) prints no `KEEPER_AGENTD_SECRET_` variable, and `prctl(PR_GET_DUMPABLE)` answers 0. Mutation: skipping the scrub fails it.
3. **`agentd.toml` is a contract.**
   - The architecture's example parses.
   - `credential = "ghp_…"` is refused with "a secret never goes in agentd.toml; write `secret:<name>` and put the secret in the environment, a systemd credential or a 0600 file".
   - An `[[agents]] drive` not in `[[drives]]` is refused.
   - `host = "Electra"` is refused.
   - An unknown key is refused.
   - `[[providers]] kind = "omp"` is refused.
   - A `[[trust]]` without `master_key` parses as not pinned. A `master_key` that is not `ed25519:<unpadded base64>` is refused, and so is a `proxy` that is not a Matrix id.
   - A `[[drives]]` entry without `owner` or `readers`, or whose owner is not among its readers, is refused naming the drive (S-15).
   - An `[[mcp]] role = "kvm:desk"` with no `[[kvm]] id = "desk"` is refused naming both (F15).
4. **The mount rule** (NFR-115's process share), on the pins. `neuraffica_never_mounts_tgdrive`: the homes are neuradrive (pinned `{tgorka, marta}`), and mounting tgdrive (pinned `{tgorka}`) is refused naming `@marta`. Also:
   - `tgorka_may_mount_neuradrive` (Nixi "can multiple");
   - mutation: checking ⊆ instead of ⊇ fails.
   - **The pin wins over the file** (S-15): `a_declaration_that_differs_from_its_pin_hosts_nothing_naming_the_difference` — a `_drive.toml` that adds `@x` to the readers, or names another owner, makes that zone host nothing, and `agents list` and `status` name each difference; the same file with the pin's readers hosts its agents. Mutation: taking the readers from the file fails it.
5. **agentd's own engine.** `agentd_never_opens_a_sync_db_it_did_not_create`: a `sync.db` placed there by hand is refused with its path.
   - `a_profile_per_drive_with_agents_and_sessions_armed`: a real local bare repository with `80-agents/_drive.toml` (matching its pin) and `60-sessions/` is cloned by `gix` through agentd's engine, and the profile has both flags and both zones materialised.
   - `a_virtual_pattern_over_the_agents_zone_is_refused_naming_it`: the folder file sets `virtualPatterns = ["80-agents/**"]`.
6. **A violating drive is never fetched** (C8, S-15). `a_drive_failing_the_mount_rule_is_never_fetched`: agentd exits `2` with the sentence before any checkout; the engine has no profile for the drive, and nothing exists under `drives/<drive-id>/`.
7. **Providers are rows, not secrets.** `apply_providers` twice gives one row. `bot_provider_token/{p}` resolves through `secret:cliproxy`. `sqlite3 keeper.db "select * from bot_providers"` holds no token. Restarting with a changed `base_url` updates the row.
8. **The headless platforms answer every method.** Each `Platform`/`SyncPlatform` method answers as listed (§12.2), and a keychain set, get and delete round-trips through a `0600` file.
9. **Operator actions, owed outside this repository, named in the PR:**
   - **OS users on electra**, one per principal: `sudo useradd --system --create-home --home-dir /var/lib/agentd-<p> --shell /usr/sbin/nologin agentd-<p>` for `tgorka`, `marta` and `neuraffica`.
   - **Forgejo tokens with `write:repository`**, one per principal's drives (tgdrive for `agentd-tgorka`, and neuradrive for `agentd-tgorka` and `agentd-neuraffica`). They can be minted only in Forgejo's UI (makistack `README.md:198`), and go into `/etc/keeper-agentd/<p>/<name>`, mode `0600`, root-owned, for `LoadCredential=`.
   - **The pins** (S-15): each `[[drives]]` entry's `owner` and `readers` are copied from the drive repository's collaborators in Forgejo (the repository's *Collaborators* settings), mapped to the people's Matrix ids.
   - **R17:** electra's existing `keeper-syncd` neuradrive and tgdrive checkouts stay pull-only mirrors (makistack `README.md:200`, `config/drives.yml`). Dr Lucyna Novak's sessions push from `agentd-neuraffica`'s own checkout with the write token above. Whether the mirrors change is the operator's decision.

**Shell crate:** no. keeper-sync, keeper-syncd, keeper-core, keeper-agent; Linux-gated by lefthook.

**binds:** FR-779, NFR-115, AD-375, AD-376, AD-377

### 90.4 — A lean Matrix client for agents (`keeper_core::agents::matrix`)

**Intent:** "they can have tags (like nixi with electra or hesperia tag) to know what materialization of nixi is used"; "also want to make suere its fast". **Rung:** **epic90-agentd**. AD-370, AD-371; FR-780; NFR-112 (its share); rulings R11, R13; F1, F7, F25 (R29).

**The APIs (D1 §3's table, R11's set, matrix-sdk 0.18 — no upgrade).** Every send below returns a builder that implements `IntoFuture` (`Room::send_raw`, `room/mod.rs:2621`; `Room::send_state_event_raw`, `:3342`; `Client::send`, `client/mod.rs:1960`), so each is awaited after its request config is set (F25):

| need | API |
| --- | --- |
| build | `Client::builder().homeserver_url(url).sqlite_store(<data>/agents/<user>/sdk, passphrase)` (as `keeper-core/src/auth.rs:625-630`, with no MSC4186 probe: §13 #24). **Retries are disabled per send** so a `M_LIMIT_EXCEEDED` reaches the caller: `.with_request_config(RequestConfig::new().disable_retry())` (`matrix-sdk-0.18.0/src/config/request.rs:117`) on each send builder (`SendRawMessageLikeEvent`, `room/futures.rs:169`; `SendRawStateEvent`, `:369`; `SendRequest`, `client/futures.rs:67`). Settled against the 0.18 sources (F25); the behaviour is acceptance 6. |
| password login | `matrix_auth().login_username(user, password).device_id(<stored>).initial_device_display_name("<agent>@<host>")` (`keeper-core/src/auth.rs:126-127`'s shape) |
| store and restore | `StoredSession::to_json`/`from_json`/`restore_into` (`keeper-core/src/auth.rs:257`, `:342`, `:354`, `:371`), under the keychain keys `agents/<user>/session` and `agents/<user>/sdk-passphrase` |
| create room | `Client::create_room`, with `creation_content.type` `dev.keeper.agent.session` or `dev.keeper.agent.control`, `m.room.encryption` in `initial_state`, and power levels from *Matrix events*, in **two shapes** (F1). Every session room: creator 100, other agents 50, people 0, `events_default` 50, and `…approval.decision`, `…heard`, `…surface.result` and receipts allowed at 0. **A proxy conversation's room** (session kind `main` or `conversation`) adds per-type `events` entries `m.room.message: 0` and `dev.keeper.agent.scope: 0`, so the person can talk to their proxy and set its scope; in every other session room a person reads and decides, and cannot post. |
| invite | `Room::invite_user_by_id` |
| text and custom events | `Room::send_raw(type, content)` |
| edit | `send_raw("m.room.message", {m.relates_to: {rel_type: m.replace, event_id}, m.new_content, body ≤ 1 KiB})` |
| state | `Room::send_state_event_raw(type, state_key, content)` |
| read state, cached | `Room::get_state_event_static`, for everything but the claim read-back (C9) |
| read state, from the server | `Client::send(get_state_events::v3::Request::new(room_id))`, filtered by type and key (C9) |
| incoming | `Client::add_event_handler` for `m.room.message`, `m.room.member` invites and the `dev.keeper.agent.*` types |
| sync | a plain `Client::sync` loop (no `SyncService`, no `RoomListService`) |

**Files:**
- `keeper-core/src/agents/matrix.rs`: `AgentClient` holds one `matrix_sdk::Client` per copy, with the methods above. `AgentMatrixError` has `RateLimited { retry_after_ms: Option<u64> }`, `TooLarge`, `Forbidden`, `NotFound`, `Network` and `Other`.
- `keeper-core/src/agents/events.rs`: the content structs of *Matrix events* that 90.4–90.6 send (the status, `dev.keeper.agent.turn`, the claim, the host manifest), the room-type constants, and `power_levels(kind) -> PowerLevelsContent` for the two shapes above. The status content carries the session's `kind` (R25, F7): `main` is the proxy's DM, `conversation` a further proxy conversation the person started, so a device tells a proxy conversation from a session it only watches. Later epics add their contents.
- `keeper-core/tests/agents_matrix_live.rs`.
- `docs/agents.md`, chapter *Matrix*.

**Acceptance (live tests run against the Synapse on delectra; the risk lives in a real homeserver with E2EE).** The run command:
`KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --test agents_matrix_live -- --ignored --nocapture`
1. **One device per copy.** `login_twice_keeps_one_device`: logging in, storing, restoring, then logging in again with the stored device id leaves the user with exactly one device, displayed `nixi@smoke`.
2. **The room is typed, encrypted and has its power levels, in both shapes** (F1). `a_session_room_is_typed_encrypted_and_has_its_power_levels`, read back from the server (`GET /rooms/{id}/state`):
   - `m.room.create`'s `type` is `dev.keeper.agent.session`, which verifies AD's `[INFERENCE]` for Synapse;
   - `m.room.encryption` is present;
   - the power levels are as the table says, for a `main` room and for a `delegated` room.
   - `the_person_talks_only_in_a_proxy_conversation`: `tgorka-smoke` (power 0) sends an `m.room.message` and a `dev.keeper.agent.scope` event; both are accepted in the `main` room and both are refused by the server (`M_FORBIDDEN`) in the `delegated` room, where the same user's `dev.keeper.agent.approval.decision` is accepted. Mutation: building the `main` room with the plain shape fails it.
3. **Everything round-trips to a second client, decrypted.** `send_edit_custom_and_state_reach_a_second_client`: the person's client receives the agent's text, the edit (relation and `m.new_content`), a custom `dev.keeper.agent.status` and a state event, each decrypted.
4. **The server read-back names the writer's event.** `the_server_read_back_names_the_event_the_writer_sent`: a claim state event sent, then read back through C9's call, gives the same `event_id` and an `origin_server_ts`. Mutation: reading through `get_state_event_static` before the next sync shows the stale value. That is why C9 rejects it.
5. **No re-delivery after a restart.** `an_event_handled_before_a_restart_is_not_handed_out_again`: an event the handler saw, then the client dropped and restored from its store, is not delivered again by the next sync. (90.5 adds the log-level dedupe.)
6. **A 429 reaches the caller.** `a_429_reaches_the_caller_with_its_retry_after`: user `nixi-paced`, with no override, sends 15 events in a burst against Synapse's default `rc_message` (0.2/s, burst 10, §6.2), and at least one is `RateLimited { retry_after_ms: Some(_) }`.
7. **Latency, measured** (NFR-112). `p95_delivery_between_two_copies`: 1 000 events from copy A to copy B's handler. p50, p95 and p99 are printed and written to `docs/agents.md` for Synapse.
8. **No new crates, no new door.** `check:core-tauri-free` and `check:core-sync-free` are green, and `Cargo.lock` gains no package. `AccountManager` is untouched, `client_for` stays private, and no messenger handler (archive, notify, drafts) is registered on an agent client.
9. **Operator action, on delectra (makistack runbook `delectra-dev-vm.md`).** If delectra is down, `task vm:start -- delectra` is the operator's. Then on delectra (`ssh delectra`):
   - `docker exec keeper-test-synapse-1 register_new_matrix_user -c /data/homeserver.yaml -u keeper-smoke-admin -p <pw> --admin http://localhost:8008`;
   - the same with `--no-admin` for `nixi-smoke`, `nixi-paced`, `tgorka-smoke`;
   - the admin's token from `POST /_matrix/client/v3/login`;
   - `curl -X POST -H "Authorization: Bearer <admin token>" http://localhost:8008/_synapse/admin/v1/users/@nixi-smoke:<server_name>/override_ratelimit -d '{"messages_per_second":0,"burst_count":0}'` (R18). `nixi-paced` keeps the default limit, on purpose.
   - The passwords go in `~/.config/keeper-smoke/synapse.env` (mode `0600`) on the machine running the tests, never in the repository.
10. **Operator action, on tuwunel** (NFR-112 is measured on tuwunel). Two users, `keeper-smoke-a` and `keeper-smoke-b`, are registered through a registration-token window: makistack `docs/runbooks/matrix-bot-channel.md` § (b), steps 1–3, `POST https://electra.siren-alsephina.ts.net/_matrix/client/v3/register` with `m.login.registration_token`, then registration closed again.
    - Acceptances 2 and 7 then run with `KEEPER_AGENTS_SMOKE_HOMESERVER` pointed at tuwunel.
    - p95 ≤ 1 s is asserted there, and above it is reported as AD-370's revisit trigger, never hidden.
    - Whether tuwunel stores an arbitrary room type is recorded.
    - The rate limits in electra's deployed tuwunel config (research §14) are read in `docker/tuwunel/.env.template` and recorded in `docs/agents.md`.

**Shell crate:** no.

**binds:** FR-780, NFR-112, AD-370, AD-371

### 90.5 — `keeper-agentd`: init, login, run — rooms in, streamed edits out, the log written

**Intent:** "sever on linux"; "also want to make suere its fast"; "the message history and actions taken". **Rung:** **epic90-agentd**. AD-370, AD-372 (its share), AD-373, AD-375; FR-781 (all but `agents init`, C12); NFR-113 (its share); rulings R8, R10, R18, R23; F2, F5, F10, F12, F15, F21, S-04, S-07, S-16, S-17, S-19, S-20, S-30 (R28, R29).

**Files:**
- **New `src-tauri/crates/keeper-agentd/`** (`[[bin]]`):
  - `Cargo.toml`: deps `keeper-agent`, `keeper-core`, `keeper-sync`, `clap`, `tokio` (multi-thread), `tracing`, `tracing-subscriber`, `toml_edit` (workspace);
  - `src/main.rs`: a plain `fn main()` that parses the command line, calls 90.3's `harden_process` while the process still has one thread (S-07), and only then builds the multi-thread runtime (`tokio::runtime::Builder::new_multi_thread`). Not `#[tokio::main]`, which starts the runtime's threads before the body runs. Logging is `tracing-subscriber`'s fmt layer to stderr, which journald keeps, and nothing else: agentd registers no observability sink (S-19). keeper-core's own `telemetry` module, the desktop's consented export, is compiled in and never constructed;
  - `src/cli.rs`:
    - `keeper-agentd [--config <path>] init | login <drive>/<agent> [--password-credential <name>] | agents list | run | status [--session <drive>/<session path>]`;
    - the config defaults to `$XDG_CONFIG_HOME/keeper-agentd/agentd.toml`, or `KEEPER_AGENTD_CONFIG`;
    - exit codes `0`, `1` (runtime), `2` (configuration), `3` (no git): syncd's;
  - `packaging/keeper-agentd@.service`, a system template unit (its directives are acceptance 15's, F21).
- **keeper-agent.**
  - `turn.rs` gains `TurnOrigin::Agent { session: SessionRef }` (C1).
  - `grants.rs`: `GrantSource`, `StoreGrants`, `AgentGrants` (C3).
  - `zone.rs`: reads each drive's zone through `browse::resolve` into Epic 89's pure functions (C2 of Epic 89).
  - `agent.rs`:
    - **`SessionContext { messages, memory_snapshot, label, epoch, claim, open, … }`** (F2): what a served session's turns read. `SessionContext::load` runs 89.5's `read_session` and `replay` once per (session, claim): when this host opens the session, takes it over (90.6) or restarts. The context lives in memory while this process serves the session and is dropped when the claim is lost or released. A conflicted log (89.5's A3) loads nothing, and the session is not served. The log is read only here, the cold path; a turn never opens a chunk (AD-365, NFR-116).
    - **The label** (AD-390). `label` starts at the session `agent.toml`'s label joined with the log's `label` lines. Every drive read joins 89.4's `label_drive_read`, with `ReadFacts` from the path, the file's OKF facts (`okf_label_facts`) and an author this host cannot name (`Unknown`, so a read is at most `agent`: DW-431); every incoming message joins its label (`label_person_message`, `label_agent_message`). Each change is a `label` line naming its cause. `SessionContext::on_user_line` is where 92.6 adds the per-turn integrity reset of a proxy conversation (S-09); in 90.5 it only joins.
    - `run_agent_turn(ctx: &mut SessionContext, …)` composes the prompt (89.3) from `ctx.memory_snapshot`, arms with `arm_turn(origin: Agent)`, and runs `run_tool_loop_reporting` with a reporter that writes `tool_call`/`tool_result` lines.
    - **The model is a sink** (S-04). Before every model request — each round of the tool loop — the turn asks `ctx.label.may_use_model(local)`, where `local` holds only for a bot of the kind 89.3's `local_only` rule accepts (`ollama`), answered by one exhaustive match in keeper-core, never a `==`. A refused round sends nothing to the provider; the turn ends with an `error` line and the final edit "This conversation has read something that may go only to a model on your own machines, and this agent's model is not one. Nothing more was sent." 92.1 and 92.6 make the same check `check_sink`'s `Sink::Model { local }` row.
  - `writer.rs`: `SessionWriter`, over 89.5's `ChunkWriter`, so every line's free text is redacted before it is written (S-17). It adds `epoch` and the claim's event id (`claim`; none before 90.6), `rotate_at` from the profile's `lfs_threshold_bytes`, the index's `apply` and `seen_events`. Every line it writes is also pushed into the session's `SessionContext` through `message_for`, as written, so the context always equals a fresh replay of the files.
  - `rooms.rs`: **`invite_decision(invite, known) -> Join | Pending`** (F5), below under *Start*.
  - `matrix_sink.rs`: `MatrixSink: TurnSink`.
  - `runtime.rs`: the 1 Hz host tick (AD-62) and the sync loops.
- **Gates.**
  - `src-tauri/Cargo.toml:3` gains the member.
  - `.github/workflows/ci.yml:90` becomes `cargo check --workspace --exclude keeper-agentd --target aarch64-apple-ios`. That is the iOS check's one form from here on, and every later story quotes it; 98.1's `keeper-nse` is a workspace member it compiles (F15).
  - `release.yml` gains an `agentd` job modelled on `syncd` (`.github/workflows/release.yml:241-322`): Linux `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`, `-p keeper-agentd` only, a `.sha256` sidecar, `scripts/check-client-secrets.ts` run on the artifact, and `systemd-analyze verify` run on the unit file.
    - **Signed** (S-30). agentd holds every agent's Matrix session and the drives' write tokens, so each artifact also gets a minisign signature, `keeper-agentd-<target>.sig`, made with the app's updater key (`TAURI_SIGNING_PRIVATE_KEY`, the key whose public half is the updater's `pubkey` in `src-tauri/crates/keeper/tauri.conf.json`). `docs/agents.md` gives the command that verifies a download against that public key before it is installed.
  - `package.json` gains `check:agentd-lean`: the agent pattern, plus `(^|[[:space:]])keeper v`, plus the observability crates `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp`, `tracing-opentelemetry` and `posthog-rs` (S-19); appended to `check`.
  - `lefthook.yml:47` gains `-p keeper-agentd`.
- **Docs.** `docs/agents.md`, chapters *keeper-agentd* (with the signature check) and *A streamed answer*. `docs/egress.md` gains agentd's row: the homeserver, the provider base URLs and the drives' remotes its `agentd.toml` names, and no telemetry destination.

**The run loop:**
- **Start.**
  - Parse the config; `harden_process` (90.3) before the runtime starts; build `HeadlessPlatform` (90.3); `apply_providers`; the mount rule on the pins (C8); open the engine; arm the folder tier; first sync; the pin check on each zone.
  - `resume_all` the sessions zones (90.2); rebuild each `.keeper/agents.db` (89.5); read the zones.
  - Restore the copies (90.4).
  - Serve the rooms named by a session `agent.toml` of an agent hosted here.
  - **An invite** (F5). An invited client sees only the room's stripped state (`matrix-sdk-base-0.18.0/src/response_processors/room/sync_v2.rs:226`), so a host cannot read a brief or a first message before it joins. `invite_decision` joins a room whose stripped `m.room.create` is typed `dev.keeper.agent.session` when the inviter is:
    - **(a)** the `human` of a proxy hosted here (a new proxy conversation, which 91.2 makes a session);
    - **(b)** the `matrix_user` of an agent homed in a drive this host mounts, when the invited agent's opening label may reach the inviter's home readers (89.4's `Label::may_reach`, over the pins; 92.1 routes it through `check_sink(Sink::Room)`);
    - **(c)** the `proxy` named by a pinned `[[trust]]` entry (90.3) whose person is a reader of the invited agent's home drive: a hand-off from a proxy that another principal's process hosts.
  - Any other invite stays pending: never joined, never declined. After a join the host reads the room's timeline; a joined room with no session waits for the story that makes one (91.2 a proxy conversation, 92.1 a delegated session).
- **A turn begins** with an `m.room.message` in a proxy's session room, from that proxy's `human`. Then:
  1. **dedupe** by event id (index `seen_events`, and the `matrix_event` of logged lines);
  2. **log the `user` line**, with `matrix_event`;
  3. **send the anchor** (`…`, with `dev.keeper.agent.turn {session, line}`);
  4. **run the turn**, under the rules below;
  5. **close.** The final edit, then the `assistant` line with `anchor_event`, `fsync`, the index updated, the engine committing on its settle and pushing.
- **The turn's rules.**
  - **History:** `SessionContext.messages` (F2). The log is read only when the context loads.
  - **System:** `compose`, with the frame `<agent>@<host>`, the session path and kind, the drives in scope, and 89.4's sentence over `SessionContext.label`.
  - **Tools:** `[tools].allow` ∩ the tools this host implements (`drive_list`, `drive_read`, `drive_glob`, `drive_grep`, `drive_stat`, `drive_write`, `drive_edit`).
  - **Grants:** `AgentGrants`. **Approvals:** refused with `UNATTENDED_REFUSAL` (C4, F12).
  - **The model:** asked only while `may_use_model` holds (S-04).
  - **Stream:** `MatrixSink`.
  - **The frozen snapshot:** core memory is read once, when the context loads, and reused while this process serves the session (AD-364).
- **Pacing (R18).**
  - The first edit comes no sooner than 400 ms after the anchor, and each later edit no sooner than 400 ms after the previous one, carrying the whole text so far.
  - On `RateLimited`, the sink waits `retry_after_ms`, then sends the whole text once, never a backlog.
  - The final edit is retried, with backoff, until accepted, and a stop or shutdown still sends it.
  - Tool progress goes out as edits of the session's status anchor, carrying counts only ("reading 3 files", "2 tool calls"), never a path, a title or a heading (S-16). Passing every status and scope edit through `check_sink(Sink::Room)` is 92.6's.
  - The fallback `body` is ≤ 1 KiB.
- **The cut (R23).** An answer longer than `FINAL_CUT_BYTES` is sent as its first `FINAL_CUT_BYTES`, on a `char` boundary, plus "The full answer is in artifacts/answer-<line ulid>.md". The artifact is written through `session_write` (90.2), and the log holds the whole text. `FINAL_CUT_BYTES` is measured (acceptance 13).
- **Ignored and not logged:** text from anyone but the proxy's `human`, and any text in a session whose agent is not a proxy or whose kind is not `main` or `conversation` (AD-380).
- **`epoch: 0`** throughout (C5).
- **An interrupted turn** is handled as C6 says.
- **SIGTERM:** each running turn ends with a final edit, "… (stopped: <host> is shutting down)". Then `fsync`, and the engine's bounded finalise, then exit `0`.
- **`status`** prints the host, each drive's engine state and mount verdict, each copy (signed in or not), the sessions served, and the tools offered or not offered on this host. With `--session`, it prints the session's "what the agent was told" (89.3's `told`, recomposed) and whether its digest matches the last `open` line (FR-771's person-facing half on the server).
- **`init`** writes the `agentd.toml` skeleton if absent, never overwriting, and says which file it left. When `control_room` is empty and a copy is signed in, it creates the principal's control room (type `dev.keeper.agent.control`) and sets `[homeserver].control_room` with `toml_edit`, every other byte kept.
- **`login`** prompts without echo, or reads `--password-credential`. It stores the session and the store passphrase, and reuses the device id.
- **`agents list`** prints every zone and home with its verdict (Epic 89's sentences), refused skills, and copies.

**Acceptance (unit and integration, Linux):**
1. **Edits are paced.** `edits_are_never_closer_than_400_ms` (fake clock, fake `EditPort`): 200 deltas over 2 s give an anchor, at most 5 edits at ≥ 400 ms, and 1 final edit carrying the whole text.
2. **A 429 is honoured, without a backlog.** `a_429_waits_retry_after_then_sends_the_whole_text_once`: a `RateLimited{2000}` delays the next edit to ≥ 2000 ms, and exactly one edit follows the wait.
3. **The final edit always arrives.** `the_final_edit_is_retried_until_accepted_and_carries_the_whole_answer`: three failures (429, 502, timeout), then success. Mutation: one attempt only fails it.
4. **The edit's shape.** `the_fallback_body_is_at_most_1_kib_and_the_new_content_is_whole`.
5. **The cut.** `an_answer_over_the_cut_is_cut_with_a_link_and_the_artifact_holds_all_of_it`: a 200 KiB answer gives a message of `FINAL_CUT_BYTES` plus the link. The artifact equals the log's `assistant` text.
6. **One event, one turn.** `the_same_event_twice_makes_one_user_line_and_one_turn` (NFR-117's "repeats nothing", by event id).
7. **Others are ignored.** `a_message_from_someone_else_is_ignored_and_not_logged`, and `free_text_in_a_non_proxy_session_is_ignored`.
8. **The agent's own grants, and no asks.** `an_agent_reaches_only_its_drives_in_scope`. `a_write_needing_approval_is_refused_before_epic_93`: the `tool_result` line is `outcome: "refused"` and its content is exactly `keeper_agent::host::UNATTENDED_REFUSAL` (90.1, F12).
9. **An interrupted turn is not re-run.** `an_interrupted_turn_is_not_rerun_after_a_restart` (C6): there is exactly one `user` line, an `error` line, the new message, and no second anchor.
10. **The memory snapshot does not move.** `the_memory_snapshot_does_not_move_during_a_session`: `MEMORY.md` is edited on disk between two turns of one session; the second turn's `prompt_sha256` is unchanged, and a new session sees the edit (NFR-118).
11. **The CLI.** `init_never_overwrites_agentd_toml_and_says_so`; `init_sets_the_control_room_and_keeps_every_other_byte`; `login_reuses_the_device_id`.
12. **The gates.**
    - `bun run check:agentd-lean` passes, and fails when `keeper` is added as a dependency, and fails when `opentelemetry` or `posthog-rs` is (S-19).
    - The iOS check passes with the exclusion.
    - The release job builds both targets in a dry run (`act` is not required: a branch push to a fork, or the job's `workflow_dispatch`). Each artifact's `.sig` verifies against the updater's public key with the command `docs/agents.md` gives, and the same check fails on a copy with one byte flipped (S-30). `systemd-analyze verify` passes on the unit file.
    - `check:rust:macos` is green: CI's macOS workspace check compiles agentd.

**Acceptance (live; the risk is a real homeserver, a real proxy and a real git remote):**
13. **Smoke on delectra's Synapse with CLIProxyAPI.** `keeper-agentd/tests/live_turn.rs`, `#[ignore = "live: Synapse on delectra + CLIProxyAPI"]`. It starts `keeper-agentd run` as a child, with a temporary XDG tree and a local bare drive holding a fixture zone (`_drive.toml` naming `@tgorka-smoke`, a proxy `nixi` whose `[model].bot` is built from `KEEPER_OPENAI_SMOKE_BASE_URL` at run time, never a literal in the repository (S-20), and one session folder whose room the harness created). The person's client (`tgorka-smoke`) asks "Read notes/hello.md and tell me its first line." It asserts:
    - the anchor arrives, and its delay is recorded;
    - edits are ≥ 400 ms apart by `origin_server_ts`;
    - the final edit quotes the line;
    - the chunk `log/<date>.smoke.1.jsonl` holds `user` (with `matrix_event`), `assistant`, `tool_call` and `tool_result` lines;
    - the chunk reached the bare remote (`git -C <bare> log --name-only` names it after the settle);
    - a `kill -9` and restart produce no second answer;
    - while it runs, agentd's outbound connections (`ss -tnp`, filtered to the child's pid) go only to the homeserver, the provider and the bare remote's host (S-19, NFR-121).

    Two runs:
    - **with `nixi-smoke` (the override):** the turn's anchor delay and final-edit delay are printed; acceptance 21 asserts them over 50 turns;
    - **with `nixi-paced` (no override, R18):** at least one 429 is observed in the agent's log, and the final edit still lands with the whole answer.

    Run command: `KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env KEEPER_OPENAI_SMOKE_BASE_URL=<CLIProxyAPI's base URL> KEEPER_OPENAI_SMOKE_TOKEN_FILE=$HOME/.omp/cliproxyapi.token cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_turn -- --ignored --nocapture`.
14. **The Megolm cut** (the architecture's Ambiguity 8). `the_largest_final_message_that_fits_encrypted`, against the same Synapse, binary-searches the largest plaintext whose encrypted event the server accepts. `FINAL_CUT_BYTES` is set to it, rounded down to 1 KiB, and recorded in `docs/agents.md`. R23's 60 KiB stands only if it fits; the measured number replaces it.
15. **Operator actions** (production rollout, owed after 91.5 seeds the zone; not required for this story's acceptance):
    - **The binary and the unit.** `sudo install -Dm755 keeper-agentd /usr/local/bin/keeper-agentd`, `sudo install -Dm644 keeper-agentd@.service /etc/systemd/system/`, with credentials under `/etc/keeper-agentd/<p>/`.
    - **Agent users on tuwunel**, per agent: open registration with `TUWUNEL_REGISTRATION_TOKEN`, `POST /_matrix/client/v3/register` with `m.login.registration_token`, close registration (makistack `matrix-bot-channel.md` § (b)).
    - **Configuration and sign-in.** `sudo -u agentd-<p> keeper-agentd init`, edit `agentd.toml`, then `sudo -u agentd-<p> keeper-agentd login <drive>/<agent>`.
    - **Start.** `sudo systemctl enable --now keeper-agentd@<p>`.
    - **The unit** (F21) is a system template unit, unlike syncd's user unit (`keeper-syncd/packaging/keeper-syncd.service`, whose only hardening is `NoNewPrivileges` and `PrivateTmp`). It has `User=agentd-%i`, `ExecStart=/usr/local/bin/keeper-agentd run`, one `LoadCredential=<name>:/etc/keeper-agentd/%i/<name>` per secret (the store passphrase among them, S-26), `Restart=on-failure`, `RestartPreventExitStatus=2 3`, `KillSignal=SIGTERM`, `TimeoutStopSec=30`, and listens on nothing (AD-370). Its hardening, named: `NoNewPrivileges=yes`, `PrivateTmp=yes`, `ProtectSystem=strict`, and `ReadWritePaths=` agentd's XDG data and state directories (`/var/lib/agentd-%i/.local/share/keeper-agentd` and `/var/lib/agentd-%i/.local/state/keeper-agentd`, which `init` creates), so everything else on the machine, its own configuration included, is read-only to it. `ProtectHome` is not set: agentd's data lives under its own home, and `ReadWritePaths=` names exactly the part of it that may change.
    - The operator records `systemd-analyze security keeper-agentd@tgorka`'s exposure score in `docs/agents.md` § Measured.

**Acceptance added by the reviews (R28, R29; unit and integration, Linux unless named):**
16. **A served session reads no log** (F2, NFR-116). `a_served_sessions_second_turn_opens_no_file_under_log`: the log reader is counted through an injected file-open counter on `log/` (an inotify watch on the directory in the Linux test). The first turn after the host opens the session loads the context and opens the chunks; the second turn opens no file under `log/`; a restart loads once more. Mutation: replaying per turn fails it. `the_context_equals_a_fresh_replay`: after three turns with tool calls, one refused, `SessionContext.messages` equals `replay(read_session(..))` of the files.
17. **Who may bring an agent into a room** (F5). `invite_decision_table` (pure): (a) the proxy's `human`, (b) an agent of a mounted drive whose home readers the invited agent may reach, and (c) a pinned person's `proxy` who reads the invited home all join; an unknown user, an agent of a drive this host does not mount without (c), an agent whose home readers are wider than the invited agent's (Dr Lucyna Novak inviting a tgdrive specialist), a `[[trust]]` person without a `master_key`, and a room not typed `dev.keeper.agent.session` all stay pending. Live, in acceptance 13's harness: `an_invite_from_an_unknown_user_stays_pending` — after an invite from a fresh test user and two sync rounds, the agent's membership is still `invite`.
18. **The model is a sink** (S-04). `a_remote_model_is_never_sent_a_local_only_label`: a session that reads a file from a `local_only` fixture drive, on an `openai` stub, sends no further request after the read; its `error` line and final edit carry the sentence. The same session on an `ollama` stub proceeds. Mutation: skipping the check fails it.
19. **Every read and message joins the label.** `every_read_and_message_joins_the_session_label`: a read of `00-inbox/x.md` lowers the session to `untrusted` with a `label` line naming the read (89.4's A1); a read of a `local_only` drive sets `local_only`; the session's next composed frame states the narrowed readers.
20. **Progress carries no content** (S-16). `tool_progress_carries_counts_not_paths`: during a turn that reads `notes/secret-plan.md` and greps `10-notes/`, no status edit holds `secret-plan`, `notes/` or any path segment the tools named; the counts are there.
21. **NFR-113, asserted** (F10). `nfr_113_holds_over_fifty_turns`, in acceptance 13's harness with `nixi-smoke` (the rate-limit override) and the crate's local stub provider streaming a fixed answer over 2 s: over ≥ 50 turns, the p95 from the host's receipt of the request event to the server's acceptance of the anchor is ≤ 1 s, and the p95 from the provider's stream end to the server's acceptance of the final edit is ≤ 1 s, both on the host's monotonic clock; edits stay ≥ 400 ms apart by `origin_server_ts`. The numbers are printed and recorded in `docs/agents.md` § Measured for Synapse. **On tuwunel** (a device run, the published figure): the same test, pointed at tuwunel with 90.4's acceptance 10 users, is run by the operator and its numbers recorded in `docs/agents.md` § Measured; that record is the gate for NFR-113's tuwunel figure.

**Shell crate:** no. CI's macOS job compiles agentd.

**binds:** FR-781, NFR-113, NFR-116 (the `SessionContext` half), NFR-121 (agentd's share), AD-370, AD-372, AD-373, AD-375

### 90.6 — Hosts, claims and placement (manifest state, epoch claims, takeover, waiting)

**Intent:** "nixi etc are everywhere … (like nixi with electra or hesperia tag) … electra (because is always on) can configue work of hesperia once this one is goes off." **Rung:** **epic90-hosts**. AD-374, AD-378, AD-379; FR-782, FR-783; NFR-120, NFR-121 (the desktop's share); UX-DR128; S-05, S-15, S-19, S-25, S-33 (R28).

**Files:**
- **`keeper-core/src/agents/claim.rs`**, pure:
  - `RENEW_EVERY = 60 s`, `TTL = 180 s`, `STOP_WITHOUT_RENEWAL = 120 s`;
  - `ServerClaim { event_id, sender, origin_server_ts, content: ClaimContent { v, host, device, agent, epoch, acquired_at, renewed_at, expires_at, released, window } }`. `window` (S-25) is optional: the RFC 3339 start of the scheduled-card window the holder is running, written by 92.3's winner when it claims for a due window and absent otherwise; a taker that finds the previous claim's `window` equal to the current one treats that window as "ran on <host>, effect unknown" and does not run it (92.3);
  - `pub fn may_acquire(current: Option<&ServerClaim>, server_now) -> bool`: no claim, a released claim, or expired by `origin_server_ts + TTL`;
  - `pub fn next_epoch(current) -> u64` (current + 1, starting at 1);
  - `pub fn holder_must_stop(last_confirmed_renewal, now) -> bool`;
  - `pub fn settle(longest_rtt, sync_round) -> Duration` (S-05): the larger of twice the longest round trip the host has measured to the homeserver and one completed `/sync` round. The taker reads the claim again after it (C9).
- **`keeper-core/src/agents/host.rs`**, the host manifest content of *Matrix events*:
  - `HostManifest { v, host, principal, version, always_on, tools, drives: Vec<{id, present, materialized}>, bots: Vec<BotId>, agents, renewed_at, expires_at }`. **`bots` holds bot ids, never a reference or a base URL** (S-33): `bot_id(reference)` is the first 16 hex digits of the SHA-256 of each `bot:{kind}:{base}#{target}` reference the host resolves, so the unencrypted state names no provider's address;
  - `is_live(server_now)`;
  - `pub fn accept(state_key, sender, content, principal_agent_users) -> Result<HostManifest, Rejected>`: `content.host == state_key`, and the sender is one of the principal's agent users.
- **`keeper-core/src/agents/placement.rs`**:
  - `place(needs, pin, drives, bot, principal, hosts, now) -> Placement { Host(slug) | Waiting { host: Option<slug>, missing: Vec<Need> } }`, as AD-379 says;
  - candidates are live, of the principal, offering every need, with every drive present and its zone materialised, and resolving the bot: the agent's `[model].bot` is compared with the manifest's `bots` by the same digest;
  - the pin restricts;
  - order: always-on first when `prefer_always_on`, then the most recent claim holder, then the lowest slug;
  - `Waiting` names the pin or the first missing need.
- **`keeper-agent/src/claims.rs`**: acquire (send, read back from the server, wait `settle`, read again, proceed only if the claim is still its own: C9, S-05); renew; stop; release on clean shutdown; hand back (C10). `claim` lines `acquired`/`released`/`lost`, never renewals, each with `claim_event` and `server_ts`. `SessionWriter` writes only under a confirmed claim, stamping every line with the claim's epoch and event id (89.5's A3).
  - **A conflicted session** (89.5's A3: two `acquired` lines at one epoch with different claim events) is served by no host. The host that finds it loads no context, edits the session's status anchor once to `run: blocked` with "Two hosts wrote this session at once (epoch <n>). It waits for you.", and `status` and `agents list` name both claim events. `docs/agents.md` § *Which host answers* says how a person resolves it: move the losing host's chunks of that epoch out of `log/` and commit; the next load is clean. DW-432 records the missing resolve action.
- **`keeper-agent/src/runtime.rs`**:
  - `HostRuntime` is shared by agentd and the desktop;
  - each 1 Hz tick renews the manifest (60 s, expiry 180 s), renews claims, applies `holder_must_stop`, places the sessions and due work it can see, and claims what it wins;
  - the status anchor's edit shows `waiting: <host> — <need>`.
- **Desktop wiring** (shell, by inspection), new `keeper/src/agents_host.rs`:
  - it starts `HostRuntime` at app start when this Mac has a signed-in copy;
  - the host slug is the account's device slug (`org_account/layout.rs:484-491`), and without an account the desktop hosts nothing, and Settings › Agents says why (DW-367);
  - the desktop hosts only agents whose home drive's `principal` (the `_drive.toml` slug) equals the account's login, the `<login>` of `<login>/device.<device>.toml` (D-27). That is R27's reading of "that person" in AD-377 layer 1 (Q4). A shared principal's agents (`neuraffica`) are never hosted on a desktop.
  - **The desktop's pin** (S-15). The Mac hosts a drive's agents only under readers and an owner the person pinned on this Mac. The first "Sign in on this Mac" for a drive shows its `_drive.toml` readers (by display name and Matrix id) and owner, and signing in pins them in `keeper.db` (device-local, never synced). A later `_drive.toml` that differs makes that zone host nothing on this Mac, through 90.3's `pin_matches`; the row names each difference and offers *Review readers*, which shows the pinned and the new values side by side and re-pins only when the person taps it.
  - a desktop copy's sync loop and state use the keychain (`agents/<user>/…`).
  - **No agent record leaves the Mac as telemetry** (S-19). keeper-core's `telemetry` builds its own closed records and has no `tracing` subscriber (`keeper-core/src/telemetry/mod.rs:1-2`); it also drops, before queueing, any record whose target is under `keeper_agent` or `keeper_core::agents`, so the desktop's consented export can never carry an agent host's spans, tool arguments, paths or provider errors.
- **Commands:**
  - `agents_copy_sign_in({profileId, agent, password}) -> AgentCopyVm` (stores the session and passphrase in the keychain, reuses the device id);
  - `agents_copies() -> Vec<AgentCopyVm>`;
  - `AgentCopyVm { drive, agent, matrixUser, device, host, signedIn, problem? }`, from keeper-core, ts-rs exported.
- **Front: Settings › Agents (UX-DR128).**
  - One row per agent found in this principal's drives that carry `[folder.agents]`: "Sign in on this Mac" (a password field), or "Signed in as nixi@hesperia". The first sign-in for a drive shows the readers and owner it pins (S-15); a pinned drive whose `_drive.toml` changed shows the difference and *Review readers*. The wording is UX-DR128's.
  - The section is absent where no folder carries the flag (AD-27).
  - 91.5's *Set up agents* links to these rows.
  - `bmad-design` lane.
- **Docs.** `docs/agents.md`, chapter *Which host answers*.

**Acceptance (pure, mutation-proved):**
1. **The claim arithmetic, exhaustively.** `the_claim_arithmetic_holds`:
   - no claim → may acquire, epoch 1;
   - a released claim → may acquire;
   - a live claim → may not;
   - expired by `origin_server_ts + 180 s` (the server's clock, never the host's) → may acquire;
   - `next_epoch` is +1;
   - a holder without a confirmed renewal for 120 s must stop, and at 119 s may continue.
2. **Placement, as a table.** `placement_table` covers:
   - each missing need; a pin; a pin to a dead host (`Waiting{host: Some}`); a drive present but its zone not materialised; an unresolvable bot; another principal's host;
   - always-on preferred, the claim holder second, the lowest slug last.
3. **Two hosts decide alike.** `two_hosts_place_alike`: one set of facts gives one answer, whatever order the hosts are listed in.
4. **Manifests are checked before they are believed.** `a_manifest_whose_host_is_not_its_state_key_is_rejected` and `a_manifest_from_a_foreign_sender_is_rejected`.
5. **A holder that cannot renew stops in time** (NFR-120). `a_holder_that_cannot_renew_stops_writing_before_another_may_take_over`, on a fake clock, with renewals failing from T:
   - the writer refuses lines from T + 120 s;
   - takeover is allowed from T + 180 s;
   - the 60 s margin is asserted.
6. **A stale writer's late line is dropped.** `a_lost_claims_late_line_is_dropped_by_every_reader`: a forced write after losing the claim is dropped by 89.5's fence.
7. **The handback.** `the_holder_hands_back_when_idle_and_placement_prefers_another_live_host` (C10): never during a turn.
8. **Control metadata carries no content** (the architecture's Ambiguity 7). `claims_and_manifests_carry_no_content`: the serialised claim and manifest hold exactly their schema keys (the claim's `window` among them, a timestamp), with no title, path or text, so leaving them unlabelled leaks at most that work exists. The manifest holds no `http`, no provider host name and no `bot:` reference (S-33).

**Acceptance (live, two agentd hosts against delectra's Synapse).** `keeper-agentd/tests/live_claims.rs`, `#[ignore = "live: Synapse on delectra"]`. It starts two `keeper-agentd run` children, hosts `electra-sim` (`always_on = true`) and `hesperia-sim`, with one agent's two copies (two devices of `nixi-smoke`) over one bare drive. The run command is 90.5's, with `--test live_claims`.
9. **A start race has one winner.** `a_start_race_has_one_winner`: both start within 50 ms, and exactly one logs `claim acquired`. The other yields after its settled re-read (S-05). The ordering of two concurrent claim writes on Synapse is recorded (research §14's last row).
10. **Takeover after a crash.** `the_other_host_takes_over_after_expiry` (FR-783): `kill -9` the holder. The other host acquires with `epoch + 1` no sooner than 180 s after the last renewal's `origin_server_ts`, logs `claim acquired` with `server_ts`, and continues the session from its files (89.5's replay). The restarted old holder writes nothing.
11. **A clean shutdown releases.** `a_clean_shutdown_releases_and_the_other_takes_over_within_two_ticks`: SIGTERM writes `released: true`.
12. **Waiting is shown, named.** `a_session_no_live_host_can_serve_waits_named`: a session needing `screen:mac`, with no host offering it, shows `waiting: hesperia-sim — screen:mac` on its status anchor (FR-782).
13. **The manifest.** Each host's `dev.keeper.agent.host` state carries its tools, its drives with `materialized`, its bots and agents, and is renewed every 60 s. A host stopped with SIGKILL is not live after 180 s.

**Acceptance (hesperia: the desktop host, the owner's scenario):**
14. **The desktop is a host.** `bun run check:rust:macos` is green. On the installed build:
    - Settings › Agents signs in `nixi` on this Mac;
    - with `keeper-agentd@tgorka` running on electra against the same drive (or the delectra fixture for the smoke), stop electra's daemon with `sudo systemctl kill -s KILL keeper-agentd@tgorka`;
    - within 180 s plus one tick, the Mac owns Nixi's main session, and its status reads `nixi@hesperia`;
    - a message to Nixi is answered from the Mac, and its log chunk is `<date>.hesperia.<n>.jsonl`;
    - start electra's daemon again: the Mac hands back at its next idle moment (C10), and the status reads `nixi@electra`.
    - The commands and the row are tested in `agents-section.test.tsx`: the row is absent without a flagged folder; signing in shows `nixi@hesperia`; a wrong password shows Rust's sentence.
15. **Operator action.** The tuwunel steps of 90.4 (acceptance 10) rerun acceptance 9's race on tuwunel, and the ordering is recorded. For the hesperia scenario against production, electra's daemon must be installed (90.5, acceptance 15).

**Acceptance added by the reviews (R28):**
16. **Two takers cannot both win** (S-05; pure, over a fake server, mutation-proved). `a_taker_that_loses_after_the_settle_yields`: hosts A and B each send epoch 5 and each read back their own event; after `settle` the server answers B's event, so A yields and writes no line while B proceeds. Mutation: skipping the second read lets both proceed, and the test fails. `settle_is_the_larger_of_two_rtts_and_a_sync_round`.
17. **Every line names its claim, and a conflict stops service** (S-05). `every_line_carries_the_claims_epoch_and_event`: the lines a holder writes carry the epoch and event id it confirmed. `a_conflicted_session_is_served_by_no_host`: given a session log with two `acquired` lines at epoch 2 and different claim events, the runtime loads no context, answers no message in that room, edits the status anchor once with the sentence, and `status` names both events.
18. **Bot ids, not addresses** (S-33). `placement_matches_bots_by_id`: an agent whose `[model].bot` digest is in a live host's `bots` is placed there; the same model on another base URL is not; `bot_id` of a reference with a trailing `/` on its base equals the one without (the reference is normalised as 89.3's `BotRef` normalises it).
19. **The window travels with the claim** (S-25). `the_claim_carries_the_window_it_runs`: a claim with and without `window` round-trips; a `window` that is not RFC 3339 is refused.
20. **The desktop hosts a drive only under its pin** (S-15). In keeper-agent: `the_desktop_hosts_a_drive_only_under_its_pin` — no pin, no agents hosted; after the pin, hosted; a `_drive.toml` that adds a reader, hosts nothing and names the reader; re-pinning hosts again. In `agents-section.test.tsx`: the first sign-in shows the readers and owner to be pinned; a changed drive shows the difference and *Review readers*; nothing re-pins without the tap.
21. **The desktop exports nothing of an agent host** (S-19, NFR-121). `agent_host_records_are_never_exported` (`keeper-core/src/telemetry/tests.rs`): records under the targets `keeper_agent::…` and `keeper_core::agents::…` never reach the export queue, while an ordinary record beside them does. Mutation: removing the target filter fails it.

**Shell crate:** yes: `agents_host.rs`, the two commands, the Settings › Agents section, and their registration in `lib.rs`.

**binds:** FR-782, FR-783, NFR-120, NFR-121 (the desktop's share), AD-374, AD-378, AD-379, UX-DR128

## Names other epics cite

Settled with Epic 91–93's and Epic 94–95's lanes on 2026-10-02:
- `keeper_agent::{turn, drive, host, task}` (90.1), `host::UNATTENDED_REFUSAL` (90.1, F12), `sessions` (90.2), `headless`, `headless::harden_process` (90.3), `grants::GrantSource`, `writer::SessionWriter`, `matrix_sink::MatrixSink`, `agent`, `agent::SessionContext` and its `on_user_line` hook, `rooms::invite_decision`, `zone` (90.5), `runtime::HostRuntime`, `claims` (90.6);
- `keeper_core::agents::{matrix, events}` with `events::power_levels` (90.4), `{agentd, mount}` with `mount::pin_matches` (90.3), `{claim, host, placement}` with `claim::settle` and `host::bot_id` (90.6);
- `TurnOrigin::Agent { session }` (90.5);
- `agents_copy_sign_in`/`AgentCopyVm` (90.6);
- `FINAL_CUT_BYTES` (90.5);
- the claim content's `window` (90.6's field, written by 92.3).

`approvals` (Epic 93), `agents init`/`agents new` (91.5) and the `session_write`/`skills_*` tool handlers (Epic 94) are not this epic's.

## What stays out

- **`agents init` and the seeded roster:** 91.5 (C12).
- **Labels at every sink:** 92.6.
- **Delegation:** 92.1. **Scheduled cards:** 92.3. **The doorbell:** 92.4.
- **Approvals:** Epic 93. Until then every ask is refused with `UNATTENDED_REFUSAL` (C4).
- **The model as a `check_sink` row** (`Sink::Model`), the per-turn integrity reset of a proxy conversation (S-09) and the `check_sink` rows for status and scope edits (S-16): 92.1 and 92.6, on 90.5's hooks.
- **Rendering agent rooms in keeper:** 91.1. 90.6 adds only the copy sign-in row.
- **The phone as a host:** never (P5).

Deferred, with the ledger entries this epic owns, as committed in `_bmad-output/implementation-artifacts/deferred-work.md` (DW-361…DW-369, DW-431, DW-432; DW-366 is merged into DW-395 and DW-368 is closed, F24 and S-15):

```markdown
### DW-361: Token streaming over MSC4471 event streams is not used.

origin: ARCHITECTURE-AGENTS.md § What stays out (2026-10-02); AD-370
location: `src-tauri/crates/keeper-agent/src/matrix_sink.rs` (anchor, edits, final edit)
reason: MSC4471 is open and needs implementation, and the matrix-rust-sdk PR for it was closed unmerged (digest R7 §2). keeper streams as an anchor plus `m.replace` edits at least 400 ms apart and one final edit, which every homeserver and client already understands. Revisit when MSC4471 is merged in the spec and in matrix-rust-sdk, or when NFR-113's measured edit cadence is the reason an answer feels slow.
status: open

### DW-362: Copies of an agent do not wake each other with addressed to-device events.

origin: ARCHITECTURE-AGENTS.md § What stays out (2026-10-02); ruling R11
location: `src-tauri/crates/keeper-core/src/agents/matrix.rs` (no to-device use); `src-tauri/crates/keeper-agent/src/runtime.rs`
reason: Ruling R11 keeps keeper free of a to-device dependency. Hosts learn about each other from state events (claims, manifests) and room timelines, which every copy already syncs. Revisit if a measured takeover or doorbell path is slower than the state-event round trip allows.
status: open

### DW-363: matrix-sdk stays at 0.18 for the agents program.

origin: ARCHITECTURE-AGENTS.md § What stays out (2026-10-02); ruling R11; research §13 #23
location: `src-tauri/Cargo.toml:61-62`
reason: 0.19.1 is out (research §6.1). An upgrade touches the messenger, verification, backup and the agents' client at once, and is its own decision, not a rider on this program. Revisit as its own epic, re-running 90.4's live tests as the agents' gate.
status: open

### DW-364: Claims, host manifests and presence are unencrypted state events.

origin: ARCHITECTURE-AGENTS.md § What stays out (2026-10-02); *Matrix events*
location: `src-tauri/crates/keeper-core/src/agents/events.rs` (claim and host contents); `src-tauri/crates/keeper-core/src/agents/claim.rs`
reason: Encrypted state is experimental in matrix-sdk (digest R7 §1). So state carries no content (90.6's acceptance 8 pins that): the server sees that a session exists, which host holds it and what a host can do, never a title, path or text. Revisit when matrix-sdk ships encrypted state events as stable.
status: open

### DW-365: A turn cut off by a host's crash is not re-run.

origin: epic 90's plan, 2026-10-02 (story 90.5; C6)
location: `src-tauri/crates/keeper-agent/src/agent.rs` (start-up: a `user` line with no `assistant` after it)
reason: After a crash, the session gets an `error` line and a message asking the person to ask again. A re-run could repeat a tool call that already took effect (a write), which NFR-117's "repeats nothing" forbids, and the log does not yet record which calls had effects that are safe to repeat. Revisit with Epic 93's consume-once record: a turn whose logged calls were all reads (T0) could be re-run automatically.
status: open

### DW-366: keeper-agentd has no Linux CI job.

origin: epic 90's plan, 2026-10-02 (stories 90.1, 90.5; digest D1 §5)
location: `.github/workflows/ci.yml` (Rust runs on `macos-latest`); `lefthook.yml:47`; `.github/workflows/release.yml` (the `agentd` job)
reason: keeper's Rust CI runs on macOS, which compiles agentd but never runs it on its real target. Linux is gated by the pre-push lefthook (clippy) and the release build. The live tests run by hand against delectra. Revisit when a Linux runner is available, or after the first agentd regression that a Linux `cargo nextest -p keeper-agent -p keeper-agentd` would have caught.
status: closed 2026-10-02
resolution: merged into DW-395 (consistency review F24, ruled by the coordinator 2026-10-02). DW-395 is the one "no Linux CI job" entry: it covers keeper-agentd's Linux gates (the pre-push lefthook and the release job) and 96.1's landlock and seccomp tests.

### DW-367: A desktop without an organisation account hosts no agents.

origin: epic 90's plan, 2026-10-02 (story 90.6; AD-374)
location: `src-tauri/crates/keeper/src/agents_host.rs`; `src-tauri/crates/keeper-core/src/org_account/layout.rs:484-491` (the device slug)
reason: A host is named by a slug, and on the desktop that slug is the account's device slug, so two Macs never claim one name. Without an account there is no slug, and the desktop does not host. Settings › Agents says so. The owner signs in with an account on hesperia, so nothing is lost today. Revisit if a person without an account wants a Mac to host agents: derive a slug from `host_label` and record it in the drive's own manifest.
status: open

### DW-368: The mount rule is checked after a drive's first checkout.

origin: epic 90's plan, 2026-10-02 (story 90.3; C8)
location: `src-tauri/crates/keeper-agent/src/headless/` (`enforce_mounts`)
reason: AD-377's rule needs the drive's readers, which live in its own `80-agents/_drive.toml`. agentd learns them only after it has cloned the drive, so a misconfigured `agentd.toml` puts that drive's bytes on disk under the wrong principal's OS user until the check deletes them and exits. No agent runs in between, and the credential that allowed the fetch was the operator's. Revisit if a principal's forge credential can read drives it must not mount: fetch only `80-agents/_drive.toml` from the default branch (a sparse, blob-filtered fetch) before any checkout.
status: closed 2026-10-02
resolution: superseded by security review S-15 (ruling R28). `agentd.toml`'s `[[drives]]` pins each drive's readers and owner, so the mount rule runs on the pins before any checkout and a drive that fails it is never fetched (story 90.3, C8, acceptance 6). A `_drive.toml` that differs from its pin makes the zone host nothing.

### DW-369: A holder hands a session back only when it is idle.

origin: epic 90's plan, 2026-10-02 (story 90.6; C10)
location: `src-tauri/crates/keeper-agent/src/claims.rs` (hand-back)
reason: P6 owns Nixi's main session by the always-on host, and AD-379 does not say what a holder does when placement later prefers another live host. The plan releases at the holder's next idle moment, so a long turn finishes on the Mac even after electra is back. Revisit if the owner wants an immediate hand-back (park the run with `run: blocked` and release at once), or none (the holder keeps the session until it goes away).
status: open

### DW-431: An agent host cannot name who last wrote a drive file, so a person's own file reads as `agent`, never `owner`.

origin: epic 90's plan, amended 2026-10-02 after the consistency review (F2: the session label held in `SessionContext`, story 90.5)
location: `src-tauri/crates/keeper-agent/src/agent.rs` (`SessionContext`, the `ReadFacts` it builds); `src-tauri/crates/keeper-core/src/agents/label.rs` (`label_drive_read`, `Author`)
reason: 89.4's `label_drive_read` gives `owner` integrity to a file whose last author is one of the drive's readers. A host learns a file's last writer only from git, and the last commit's `Keeper-Device` trailer names a device, not a person; no record maps a device to a reader's Matrix id. So 90.5 passes `Author::Unknown`, and every drive read is at most `agent` (fail low, AD-151): only what the person says in the room is `owner`. That blocks nothing today: 92.6's integrity rule acts on `untrusted`, and S-13's promotion counts the person's messages. Revisit when a rule needs `owner` evidence from a file: map a commit's `Keeper-Device` to a reader through the account's device records (`<login>/device.<device>.toml`, D-27), which is what S-31 says `owner` means ("committed by a reader's keeper").
status: open

### DW-432: A conflicted session has no resolve action.

origin: security review S-05, accepted by ruling R28 (2026-10-02); epic 90's plan, story 90.6
location: `src-tauri/crates/keeper-agent/src/claims.rs` (a conflicted session is served by no host); `src-tauri/crates/keeper-core/src/agents/log/reader.rs` (`SessionLog.conflicted`)
reason: When two hosts both acquired one epoch with different claim events (a race the settle makes unlikely, not impossible), the log holds two truths, replay refuses it, and no host serves the session. Its status anchor says so, and `docs/agents.md` tells a person to move the losing host's chunks of that epoch out of `log/` and commit. There is no button, because choosing which host's lines are the truth is the person's call and a wrong choice discards work. Revisit when a conflict happens in practice: a Settings › Agents action that shows both hosts' lines of the epoch side by side and moves the unchosen chunks to `log/conflicted/` in one commit.
status: open
```

## The failure shape this epic must not repeat

**An extraction that changes behaviour** (research §13, "riskiest seams"). A review that finds any of the following is a blocker:
- a `TurnOrigin` decided by entry point instead of `spoken_turn(dir)`;
- a characterisation golden regenerated from the moved code;
- a renamed `#[tauri::command]`;
- `block_in_place` reached on a current-thread runtime.

**Two writers on one session.** A review that finds any of the following is a blocker:
- a line written without a confirmed claim (after 90.6), or without the claim's epoch and event id;
- a takeover decided from the local store rather than the server's read-back, or without the settled second read (S-05);
- an expiry computed on the host's clock;
- a renewal logged as a line;
- a holder that keeps writing past 120 s without a renewal;
- a conflicted session served.

**A daemon that is not a boundary.** A review that finds any of the following is a blocker:
- agentd opening a `sync.db` it did not create;
- a secret in `agentd.toml`, a log line or `keeper.db`, or a `KEEPER_AGENTD_SECRET_*` variable left in agentd's environment once read;
- a drive fetched before the mount rule passed on its pin, or a zone hosted under a `_drive.toml` that differs from its pin;
- a listening socket;
- agentd linking the shell, or an observability crate (`check:agentd-lean`);
- a provider base URL in a host manifest.

**A turn that re-reads its past.** A review that finds a turn opening a file under `log/` while its session is served is a blocker (F2, NFR-116).

**A stream that floods or loses the answer.** A review that finds any of the following is a blocker:
- an edit closer than 400 ms;
- a backlog sent after a 429;
- a final edit not retried;
- a message over the measured cut;
- an incoming event processed twice.

## Sprint-status entry

The epic's entries are in `_bmad-output/implementation-artifacts/sprint-status.yaml` (the `epic-90` block). Its deferred items are in `_bmad-output/implementation-artifacts/deferred-work.md`: DW-361…DW-369 (DW-366 merged into DW-395, DW-368 closed), DW-431 and DW-432.

## Stack rungs

Each rung compiles alone.
1. **`epic90-extract`**:
   - `keeper-agent` with the turn loop, tool host and task runner, and the shell's ports (90.1);
   - the sessions runtime in `keeper_agent::sessions`, with the lock, resume, caller ids, canonical containment and `session_write` (90.2);
   - `check:agent-tauri-free`.
   - The characterisation goldens are committed in this rung, from the capture commit's hesperia run.
   - Awaits CI's macOS job and `check:rust:macos`.
2. **`epic90-agentd`**:
   - `keeper_sync::xdg` and syncd's use of it, the headless platforms with `harden_process`, `agentd.toml` with its pins and the mount rule (90.3);
   - `keeper_core::agents::{matrix, events}` with both power-level shapes (90.4);
   - `keeper-agentd`, `GrantSource`, `SessionWriter`, `SessionContext`, `invite_decision`, `MatrixSink`, `TurnOrigin::Agent`, the signed release job, the iOS exclusion and `check:agentd-lean` (90.5).
   - No shell hunk: CI's macOS job compiles it, and lefthook gates Linux.
3. **`epic90-hosts`**:
   - claims (with the settle), manifests (with bot ids) and placement (pure);
   - `HostRuntime` in agentd;
   - the desktop host with its pins, the telemetry target filter, the two commands and the Settings › Agents row (90.6).
   - Awaits CI's macOS job and `check:rust:macos`.
