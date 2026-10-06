# Epic 94 — Workflows the way BMAD writes them

created: '2026-10-02'
status: planned 2026-10-02; build follows in story order on the agents stack
source: the owner's rounds 1–3 of 2026-10-01/02 (excerpts verbatim below, as `_bmad-output/planning-artifacts/agents-coordinator-decisions-2026-10-02.md` and research §1.1–§1.3 record them), pinned by the coordinator as P11 and P14 and rulings R9 and R21, with the review rulings of 2026-10-02 applied here (R28's S-04, S-09, S-12, S-21, S-27; R29's F2, F6, F22). Other inputs:
- `_bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-AGENTS.md`, which is binding. It decides AD-380 (`ask_human`), AD-396 (the `bmad` module), AD-397, AD-398 and AD-399. This epic adds no decision of its own. Every place it had to settle something the architecture leaves open is listed under *Open questions for the coordinator*.
- `_bmad-output/planning-artifacts/research-agents-2026-10-02.md`: §10 (G4's reading of BMAD), §13 #42–#44, §14. Cited as §n.m.
- The lane digest G4 (`digest-G4BmadWorkflowFormat.md`, kept outside this repository; research §10 carries its findings), cited as G4 §n. The parity tests in 94.1 pin the behaviour G4 documents.
- BMAD's own sources, read on 2026-10-02:
  - this repo's `_bmad/scripts/{config_utils,render_skill,memlog,resolve_config,resolve_customization}.py`. These are byte-equivalent to BMAD-METHOD tag v6.12.0 @05bfbd46d00766ec88eb9b42e76be2c575d64d7b, `src/scripts/` (established by the 89/90 lane).
  - the installed plugin's skills at `~/.claude/plugins/cache/bmad-method/bmad/6.12.0.0/skills/`, written `S/` below as G4 writes it.
  - BMAD-METHOD's `LICENSE`: MIT, "Copyright (c) 2025 BMad Code, LLC", with a trademark notice.
- The drive's own BMAD install, `/workspace/tgdrive/_bmad/`, run in process on 2026-10-02 (cited `tgdrive:_bmad/…`).
- The operator's plugin template, `/workspace/bmad-plugin/plugins/bmad/runtime/_bmad/config.toml`.
- The neighbouring lanes' landing points, agreed by message on 2026-10-02:
  - from the 89/90 lane: 89.1 `keeper_ported::bmad::config`; 89.3 the skills index and `WriteScope::with_agents`; 89.5 `keeper_core::agents::log` and `index`; 90.2 `session_write`; 90.5 `keeper_agent::writer::SessionWriter`;
  - from the 91–93 lane: 91.2 the one-door rule; 92.1 `delegate`/`reply`; 92.2 `card_update` and the card fields; 92.3 due cards; 92.5 the stewards' `[[menu]]` prompts; 93.1 `tier::classify` and the raise; 93.2 `Parked`; 93.4 the unattended raise.

Line numbers are in the `agents-plan` worktree on 2026-10-02.

binds: FR-800…FR-803; NFR-119 (94.1's half); AD-380 (its `ask_human` half), AD-396 (its `bmad` module), AD-397, AD-398 and AD-399. All of these were allocated by the architecture, not here. Deferred items in DW-381…DW-384 (DW-381 placed by the architecture). There is no UX decision.
- **The previous ceilings:** the program's (`_bmad-output/planning-artifacts/agents-program-map-2026-10-02.md`, C1) are epic 88, AD-359, FR-766, NFR-111, UX-DR126, DW-354 and D-30. The architecture allocated AD-360…AD-416, FR-767…FR-822 and NFR-112…NFR-122, and `docs/decisions.md` holds D-31…D-36. FR, NFR, AD and D numbers are the architecture's.
- **No earlier allocation.** On 2026-10-02, a grep found only the architecture's own rows (`ARCHITECTURE-AGENTS.md:977`, `:984`, `:1214`) and its DW-381 (`:974`, `:1246`).
  - **Where:** `_bmad-output`, `docs`, `src`, `src-tauri/crates`, `AGENTS.md`, `README.md` and `CLAUDE.md`.
  - **What for:** `epic-94`, `epic94`, `DW-E94-`, `UX-DR-E94-` and `94-[1-4]-`.

  This epic's deferred items therefore start at DW-382.
see-also:
- D-31 (an agent lives in a drive), D-34 (labels; drafted);
- AD-65 (`browse::resolve` is the one containment rule), AD-158 (a grant is re-checked per call, and the audit row precedes the effect), AD-159 (file content is data, and every bound is disclosed);
- epic 92 (delegation, cards, due cards) and epic 93 (tiers, park and resume, the unattended raise). This epic's workflows run through both, and only through them.

## The owner's ask

Verbatim, rounds 1–3 (2026-10-01/02):

> i want bmad style personalities and different purposes (coding, exploring, designing, marketing, hr, psychologist etc)

> i would prefer to communicate and cooperate and delegate work for different bots instead of sub-agents - to avoid confustion and make one point of true - also want to make suere its fast

> I want support for workflows definitions - i want to support what bmad method have in the workflow ... but also quick free speak model with no workflow that can triger and being proxy between real human and the whole agentic system (the same proxy bot can be between non hyman but other system or service with sync/async matter)

> nixi will alsways be a proxy between tgorka and rest of the system

> I want also the scheduled job on the kaban to work on (tasks on keeper).

> - rewriting to rusr recommended parts (add separate source module)
> Rest plan the bmad and proceed with the implementation (bmad research, architectire, epics etc then review and implement one after another one)

## The verdict, ask by ask

The epic is a plan, so the verdict is what the plan does, not what exists.

| # | The ask (verbatim) | Verdict | How it is met | Mechanism |
| --- | --- | --- | --- | --- |
| 1 | "i want to support what bmad method have in the workflow" | **planned** | A BMAD skill is copied unchanged into a drive's `_workflows/<name>/` with a `workflow.toml` header. Every capability BMAD assumes (G4 §5) has a tool or a sentence that names why not. | AD-397, AD-398; 94.2, 94.3 |
| 2 | "rewriting to rusr recommended parts (add separate source module)" | **planned** | `keeper-ported::bmad` holds BMAD's deterministic helpers in Rust: the configuration merge, overlays, render tokens, memlog, the phase graph and the party roster. The Python scripts are never run. Their own tests are the parity proof. | AD-396; 94.1 |
| 3 | "quick free speak model with no workflow … proxy between real human and the whole agentic system" · "nixi will alsways be a proxy" | **planned** | A workflow's halts and menus reach the person through their proxy (`ask_human`), and the proxy itself runs no workflow. An unattended run answers with the stated default and is held to one tier stricter. | AD-380; 94.2, 94.3 |
| 4 | "delegate work for different bots instead of sub-agents … one point of true" | **planned** | BMAD's "spawn a subagent" becomes a `helper` that owns nothing and lives inside the turn. Anything that must write or outlast the turn is a delegated session. | AD-399; 94.4 |
| 5 | "the scheduled job on the kaban" | **planned, with 92.3** | A workflow card carries `workflow:`, an `assignee` and optionally `schedule:`. Each due run opens a fresh workflow session. | AD-387, AD-398; 94.3 |
| 6 | "bmad style personalities" | **partly here** | The party roster resolves BMAD's agents exactly as BMAD does (94.1). Importing a persona as a soul is 89.1/89.3's job. | AD-396 |

## What the triage found

| Need | Verdict | Evidence |
| --- | --- | --- |
| BMAD's deterministic helpers | **present, in Python only** | `_bmad/scripts/config_utils.py:79-119` (merge, layers), `render_skill.py:30-33` and `:140-267` (tokens), `memlog.py:78-187`, `resolve_config.py:27-78`, `S/bmad-party-mode/scripts/resolve_party.py:90-273`. Beyond those, the skills carry 39 other helper scripts (`find S -path '*/scripts/*.py' -not -path '*/tests/*'`, 2026-10-02), e.g. `lint_spine.py`, `pick_methods.py`, `sprint_plan.py`, `brain.py`. |
| Their upstream tests | **present** | At v6.12.0 @05bfbd46: `src/scripts/tests/test_config_utils.py`, `test_memlog.py`, `test_resolve_config.py`, `test_resolve_customization.py`. Installed beside the party script: `S/bmad-party-mode/scripts/tests/test_resolve_party.py` (17 tests). No upstream test covers `render_skill.py`. |
| A Rust port | **partial after 89.1** | 89.1 lands `keeper_ported::bmad::config` with `load_toml`, `structural_merge` (keyed on `code`/`id`), `merge_layers` and `load_customization` (the 89/90 lane). The central four-layer loader, render, memlog, the phase graph and the party roster are this epic's. |
| The duplicated config keys | **present, in three installs** | See the defect section below. |
| A `.memlog.md` written into a session | **refused today** | The session file verbs refuse any dotted segment (`keeper-core/src/sessions/files.rs:254-269`, `FileVerbError::Hidden`), because keeper's scan never reads a dotted file back (Story 52.5). |
| Asking a person and waiting | **absent for agents** | The ⌘9 approver blocks a tool call in memory (`bots_drive_ipc.rs:162-208`, G1 §3). 93.2 builds `Parked`, and nothing yet asks a *question*. |
| Sub-agents | **absent, by decision** | P7 makes delegation a session owned by the target. "In-turn helpers (review passes) never own work." |
| `skills_list` / `skill_view` | **named, not built** | 89.3's prompt index names `skill_view` as the way a body loads (AD-363). The handlers belong to no story before this one (the 89/90 lane). |
| A workflow format | **absent** | P11's `TaskKind::Workflow` is superseded by ruling R9 (AD-398's note). |

## The BMAD config defect, and what the port does about it

**The defect.** G4 named one defect (G4 §2). The 2026-10-02 in-process run found a third duplicate.
- The duplicates:
  - `planning_artifacts` and `implementation_artifacts` are each defined in both `[modules.bmm]` (`_bmad/config.toml:19-20`) and `[modules.gds]` (`:32-33`);
  - `project_knowledge` is duplicated the same way (`:21`, `:34`), even though both values are `"{project-root}/docs"`.
- How the renderer trips on them:
  - `render_skill.py`'s short token `{{.key}}` must name exactly one leaf anywhere in the merged configuration (`_find_config_values`, `_resolve_short_config`, `render_skill.py:128-150`);
  - it counts leaves by key name and never compares values;
  - `bmad-build`'s step files use `{{.implementation_artifacts}}` (e.g. `S/bmad-build/step-04-review.md:27`, `:76`).
- What happens: the renderer raises `RenderError`, the CLI turns it into `HALT:` (`render_skill.py:393-395`), and `bmad-build`, `bmad-build-auto`, the `bmad-dev-auto` alias and `bmad-loop`'s dev passes stop at load (G4 §2).
- Reproduced in process on 2026-10-02, in this repo and in `/workspace/tgdrive`, with identical sentences:
  - ``ambiguous config value `implementation_artifacts` found at: modules.bmm.implementation_artifacts, modules.gds.implementation_artifacts``
  - ``ambiguous config value `planning_artifacts` found at: modules.bmm.planning_artifacts, modules.gds.planning_artifacts``
  - ``ambiguous config value `project_knowledge` found at: modules.bmm.project_knowledge, modules.gds.project_knowledge``
- Rendering `S/bmad-build` halts on the first: ``HALT: ambiguous config value `implementation_artifacts` found at: …``.
- **Where it comes from.** The plugin's template, `/workspace/bmad-plugin/plugins/bmad/runtime/_bmad/config.toml:30-34`, is written into every install by `/bmad:init` (`tgdrive:_bmad/custom/config.toml:5-9`). No keeper-side or drive-side layer can repair it:
  - `structural_merge` adds or replaces a key and never deletes one (`config_utils.py:79-88`);
  - two equal values are still two leaves.

**The decision: the port errors the same way.** The architecture agrees with the Python: "A render error (such as the duplicate config key) is a refusal with its sentence, never a guess" (AD-397's rule), and research §13 #42 gives "refuse with a sentence naming the ambiguous key, as `render_skill.py` does" as the mitigation.
- `keeper_ported::bmad::render` returns `RenderError::Ambiguous { key, paths }`. Its sentence is byte-identical to the Python's, paths in merged-table order.
- `bmad_render` hands that sentence to the model as a refusal, prefixed `HALT: `, exactly as the CLI prints it.
- The port does **not** resolve module-scoped keys:
  - there is no "prefer the skill's own module";
  - there is no "equal values are one value".
- The explicit path token `{{config.modules.bmm.implementation_artifacts}}` already resolves uniquely in the Python (`render_skill.py:208-215`), and the port keeps that. A skill or overlay that names the module was never ambiguous.
- **Rejected:**
  - Resolving by the skill's module: `render_skill.py` never reads `skill-manifest.csv`'s `module` column. Guessing would make keeper the one BMAD runtime that renders what every other refuses, and would hide the defect from the person who can fix it.
  - Treating equal values as one: `project_knowledge` would render while the other two did not, which is a fix nobody asked for.
  - Editing the drive's `_bmad/config.toml`: it is installer-managed, and the next `/bmad:init` reverts the edit.
- **The fix is the operator's** (OA-94-1 below). Until it is made, format-B skills are refused with the sentence above in any drive whose install carries the duplicates (DW-383).

## The capability map (G4 §5 → keeper)

BMAD assumes these capabilities without naming a tool (G4 §5). 94.2 keeps one table in `keeper_core::agents::workflow` with exactly these 19 rows. Each answers either with a tool from AD-397's closed vocabulary, or with the sentence the model receives when that tool is not offered to it.

| # | Capability BMAD assumes (G4 §5) | keeper's answer | The sentence when the tool is not offered |
| --- | --- | --- | --- |
| 1 | read a whole file or a range | `drive_read` | — |
| 2 | write files; edit YAML frontmatter in place | `session_write` inside the session (90.2); `drive_edit` for an existing file elsewhere in the drives (T2, AD-392) | "Writing outside this session needs `drive_edit`, which this agent is not offered." |
| 3 | list / glob | `drive_list`, `drive_glob` | — |
| 4 | grep the code; read `git log` | `drive_grep` (literal, `spec-61-11-the-drive-as-tools.md:34`); `git log` over a repository in `workspace/` → `run` (96.1) | "`git log` runs only through `run`, which this agent is not offered." |
| 5 | run commands (`uv run`, Python ≥ 3.11) | `resolve_config.py`/`resolve_customization.py` → `bmad_config`; `render_skill.py` → `bmad_render`; `memlog.py` → `bmad_memlog`; `resolve_party.py` → `bmad_party`; any other script → `run` (96.1) | "keeper never runs BMAD's Python helpers; `<script>` has no Rust port (DW-382) and `run` is not offered." |
| 6 | git `rev-parse`, diff to a temp file, commit | the drive's sync engine commits the drive; coding goes to Paseo (96.3) or `run` over `workspace/` | "keeper commits this drive itself; an agent never runs git on a drive." |
| 7 | run tests / linters | `run` (96.1) | "Tests run only through `run`, which this agent is not offered." |
| 8 | ask the user and wait (HALT, menus) | `ask_human` (94.2) | — (an unattended run gets the stated default, AD-380) |
| 9 | invoke a skill by name, forwarding intent | `workflow_start` (94.3), the name resolved through the phase graph (94.1) | "`<name>` is not a workflow in this drive's `_workflows/`." |
| 10 | spawn a sync or parallel context-free subagent | `helper` (94.4). A subagent that must write is BMAD's documented inline fallback (`S/bmad-build/step-01-clarify-and-route.md:62-63`): the agent does it itself, in its own session, or delegates (92.1) | "`helper` is not offered to this agent; do the work inline, as the skill's fallback says." |
| 11 | re-address a live subagent by id | the agent continues its own session (AD-399); a delegated session is re-addressed with `reply` (92.1) | "A helper keeps no identity after it returns; continue the work yourself or `reply` into the delegated session." |
| 12 | agent teams + a capability probe | `delegate` (92.1). The probe (`runtime.canLaunchSubagents?.()`, `S/bmad-testarch-atdd/steps-c/step-04-generate-tests.md:124`) is answered by the session frame: "`helper` is available; a helper cannot write." | — |
| 13 | per-agent model choice | the delegated agent's own bot; a review layer's `bot` (Q7) | — |
| 14 | web search | only an MCP server the person configured (96.2), labelled `untrusted` | "keeper has no web search of its own (DW-381); none of your MCP servers offers one." |
| 15 | MCP / external systems | `mcp__<server>__<tool>` (96.2; the wire name, ruling R24(7)) | "No MCP server is configured for this agent on this host." |
| 16 | headless/TTY detection, environment variables | the session frame states whether a person is in the chain and `checkpoints` (AD-363 slot 5); no environment variable is exposed | "No environment variables are visible to an agent; the session frame says whether a person can answer." |
| 17 | open an editor or an HTML report | `surface_open` for a person's proxy (91.3); any other agent names the artifact's path in its reply | "Only a person's proxy opens a note on their screen; the report is at `<path>`." |
| 18 | token counting, the current date | the session frame: date, time, the turn's remaining token budget (AD-363) | — |
| 19 | session lifecycle hooks, tmux | the session log's lines are the lifecycle; `bmad-loop` and its hooks are not run | "bmad-loop does not run under keeper; a card or a delegation is the loop." |

## What earlier decisions said, and what this epic amends

| The earlier decision | What it said | What this epic needs | The amendment |
| --- | --- | --- | --- |
| **P11** (`_bmad-output/planning-artifacts/agents-coordinator-decisions-2026-10-02.md`) | scheduled workflows are a new `TaskKind::Workflow` | a workflow on a schedule | **Superseded by ruling R9**, as AD-398's note records: a workflow card, never a task kind. |
| **Story 52.5's dotted-name refusal** (`sessions/files.rs:249-269`) | keeper never writes a dotted file or folder into a session, because its scan never reads one back | BMAD's memlog is `.memlog.md`, a fixed name (`memlog.py:78`) | **Scoped (Q3).** One named exception, the basename `.memlog.md` inside `artifacts/`, written only by `bmad_memlog`. Every other dotted name is still refused. |
| **`render_skill.py`'s absolute paths** (`:118-125`, `:255-261`) | a resolved value and a snapshot reference are absolute paths | keeper's tools address drive-relative paths, and no synced file carries an absolute path (FR-145, AD-65) | **Port deviation, stated in `UPSTREAM.md` (Q2).** |
| **The ⌘9 bots' tools** (`bots/tools.rs:180-248`) | seven drive verbs | the BMAD tools | **Held for ⌘9.** The new tools are offered to agents only. |

## Requirements

Copied from the architecture's *Requirements allocated here*. They are not restated and not renumbered.

| id | statement | epic.story | AD |
| --- | --- | --- | --- |
| FR-800 | BMAD's configuration merge, customization overlays, render tokens, memlog, phase graph and party roster run in Rust with the same results as BMAD's own helpers. | 94.1 | AD-396, AD-397 |
| FR-801 | A BMAD skill runs on an agent as written: the agent has every capability BMAD assumes, and "ask and wait" reaches the person through their main agent. | 94.2 | AD-397 |
| FR-802 | A workflow is a BMAD skill plus `workflow.toml`; a workflow card runs it, on a schedule if it has one, and its checkpoints reach the person through their main agent. | 94.3 | AD-380, AD-398 |
| FR-803 | Review passes and helpers run inside the turn and own nothing; work that must be owned is handed to an agent as a session. | 94.4 | AD-399 |
| NFR-119 | **The licence firewall holds.** Every ported module names a permissive licence in its `UPSTREAM.md`; every new crate passes `cargo deny`; AGPL and GPL software (Sygnal, ntfy's GPL option, NanoKVM firmware, Element Call) is run as a separate service or read as a protocol, never linked; every model in `_models/` names its licence. | 89.1, 94.1, 96.5, 97.1, 98.3 | AD-396, AD-410 |

**Held, not restated:**
- NFR-115 (no byte crosses principals). `ask_human` is a send into the proxy's DM and is checked as one (94.2 #6).
- NFR-117 (a crash loses nothing). `bmad_memlog` writes atomically through the session runtime.

## Built on

This epic uses, and does not rebuild:
- `keeper_ported::bmad::config` (89.1);
- the skills index, `keeper-ported::agentskills` and `WriteScope::with_agents` (89.3);
- the session log, its reader and the `.keeper/` index (89.5);
- `session_write` and the session runtime (90.2);
- `SessionWriter` (90.5);
- claims and placement (90.6);
- the person-only-talks-to-the-proxy rule (91.2);
- `delegate`, `reply` and the delegation room (92.1);
- the card fields and `card_update` (92.2);
- due cards (92.3);
- the tier table and its raise (93.1, 93.4);
- `Parked` (93.2).

Before 96.1 and 96.2 are on the stack, rows 4–7, 14 and 15 of the capability map answer with their sentences. The map is keyed by tool, so they answer with the tool as soon as it is offered, with no change here.

## Open questions for the coordinator

Each has the reading this plan builds to, marked as such, so no lane is blocked. None is resolved silently.

- **Q1. BMAD's one project root is two places in a keeper session.**
  - **The gap.** `render_skill.py` and the resolvers take one project root that both holds `_bmad/` and receives the outputs (`{planning_artifacts}` = `{project-root}/_bmad-output/planning-artifacts`). AD-398 puts a workflow's outputs in the session's `artifacts/`. The drive's BMAD install sits at the drive's root (`/workspace/tgdrive/_bmad/`), and the same was reported for neuradrive (`tgdrive:_bmad/custom/README.md:52-54`).
  - **Plan's reading:** the *install root* is the home drive's root. `bmad_config`, `bmad_render` and `bmad_party` read `<drive>/_bmad/` there (the four central layers, `_bmad/custom/<skill>.toml`, `_bmad/_config/`). The *output root*, which is what `{project-root}` means in a workflow session, is the session's `artifacts/`. So a BMAD output lands inside the session, versioned and promotable. Every tool result and the session frame state both roots.
  - **Rejected:**
    - the drive root for both: every output would be a T2 write outside the session, and two runs would share one `epics.md`;
    - the session folder as output root: outputs outside `artifacts/` are neither promotable nor the session's contract.
- **Q2. Absolute paths in a render.**
  - **The gap.** The Python substitutes the absolute root and refuses a value that does not then resolve to an absolute path (`render_skill.py:118-125`). It renders `[[bmad-snapshot:f.md]]` as an absolute path into `_bmad/render/<skill>/<slug>-<hash12>/<gen20>/` (`:255-261`, `:358-365`).
  - **Plan's reading:**
    - the port substitutes the session's drive-relative output root, and replaces "must resolve to an absolute path" with "must resolve inside this session" (`browse::resolve`);
    - a generation is published under `<session>/workspace/bmad-render/<skill>/<gen20>/`, which is unsynced and rendered again on takeover (94.3 #8);
    - the generation identity hashes the drive-relative root and `keeper-ported::bmad`'s version where the Python hashes the absolute root and `render_skill.py`'s own SHA-256 (`:347-357`), so one run renders the same path on every host.
  - Every other byte of a rendered file equals the Python's output for the same root string (94.1 #4).
- **Q3. `.memlog.md` is a dotted name.**
  - **Plan's reading:** `bmad_memlog` writes through 90.2's session runtime with one exception to the Hidden rule: the basename exactly `.memlog.md`, inside `artifacts/`.
  - The board still does not list it. The model reads it with `drive_read` on resume, which is the only time BMAD reads it (`memlog.py:23-26`).
- **Q4. `ask_human`'s default.** AD-380 gives `ask_human(question, choices?)`, and says an unattended run "returns the question's stated default".
  - **Plan's reading:** an optional `default` argument is the stated default. It must be one of `choices` when they are given.
  - With no person in the chain and no default, the call is refused with "No person can answer this run and the question names no default". BMAD's own HALT then ends the turn with `run: blocked`.
- **Q5. A workflow card's runs.**
  - **The gap.** AD-387 has a card's run "append a turn to the card's own session". AD-398 has a due workflow card "open a session of kind `workflow`".
  - **Plan's reading:** each run of a workflow card opens a new `workflow` session.
    - Its caller-supplied id derives from the card's id and the due window, so a retried start is one session (AD-368).
    - It is owned by the card's `assignee`, with `parent` = the card's session.
    - The card's session gets the `delegate` line that links the run.
    - The card's `run:` and `last_run` follow the latest run.
  - One run gets one fresh context (bmad-help's "fresh context window" per skill, G4 §4) and one set of progress files.
- **Q6. `workflow_start` is a hand-off to oneself.**
  - **Plan's reading:** `workflow_start(name, inputs)` uses 92.1's delegation machinery (room, placement, idempotent create, reply) with `to` = the calling agent and `card.workflow = name` (the delegate event already carries `workflow?`), kind `workflow`.
  - `hop` is the caller's plus one, so AD-385's bound of three also stops a workflow that starts itself.
  - AD-392's raise for `hop ≥ 1` then applies once, as for any hand-off.
- **Q7. "A bot named by the workflow's review layer"** (AD-399). BMAD's review-layer tables carry `id`, `name`, `instruction` and `when` (`render_skill.py:72-95`).
  - **Plan's reading:** a review layer may carry `bot = "bot:{kind}:{base}#{target}"`. This key lives in the merged customization, and BMAD's renderer ignores it.
  - `helper(…, lens = "<layer id>")` uses that bot when this host resolves it and `check_sink(Model { local })` (92.1, R28 S-04) allows it for the session's label: a model provider is a processor the person chose, so only the label's `local_only` can refuse it.
  - Absent, the helper uses the agent's own bot, which is what `bmad-build` asks for: "All review subagents must run at the same model capability as the current session" (`S/bmad-build/step-04-review.md:6`).
- **Q8. Party modes that need living handles.** `subagent` and `agent-team` (`S/bmad-party-mode/SKILL.md:41-50`) keep one handle per persona across rounds, and AD-399 forbids a helper that lasts.
  - **Plan's reading:** `bmad_party` returns the roster exactly as the Python does, `party_mode` included.
  - The capability map (rows 11–12) answers those two modes with "keeper runs a party in one mind (session mode); a persona that must think on its own is a delegation". The orchestrator proceeds in `session` mode.
- **Q9. A workflow folder's name decides its overlays.** `load_customization` keys `_bmad/custom/<skill>.toml` by the skill directory's name (`config_utils.py:110-117`).
  - **Plan's reading:** kept. A workflow folder carries the BMAD skill's own name (`_workflows/bmad-create-epics-and-stories/`) for the drive's overlays to apply, and `workflow.toml`'s `name` equals the folder (the architecture's grammar).
  - **Rejected:** a `bmad_skill` key in `workflow.toml`. The grammar is closed, and two names for one thing invite the overlay to miss.

## Stories

Every story names its rung in the stack (*Stack rungs*, below).
- **No story here touches `src-tauri/crates/keeper/**`** (the epic map). Everything is `keeper-ported`, `keeper-core` and `keeper-agent`, proved on this Linux host. The desktop host runs the same `keeper-agent` code in process.
- **Every new pure behaviour test is mutation-proved:** mutate, run, restore, and read the diff.
- **Parity fixtures are generated once by the Python and committed.** The command that produced each one is recorded in `keeper-ported/src/bmad/UPSTREAM.md`, and no test runs Python.
- **Names are suggestions the lanes agree on.** Function and file names below are the plan's; the behaviour may not change.

### 94.1 — keeper-ported::bmad: config merge, overlays, render tokens, memlog, phase graph, party roster

**Intent:** "rewriting to rusr recommended parts (add separate source module)"; "i want to support what bmad method have in the workflow". **Rung:** **epic94-bmad**. AD-396 (the `bmad` module grows here; its first consumer was 89.3), AD-397 (BMAD's helpers are never executed, and a render error is a refusal with its sentence); FR-800; NFR-119.

**Files:**
- `src-tauri/crates/keeper-ported/src/bmad/config.rs` (89.1's) gains:
  - `load_central_config(layers)`, the four central layers in the installed order (`config_utils.py:98-107`: `config.toml` → `config.user.toml` → `custom/config.toml` → `custom/config.user.toml`);
  - `extract_key` (`resolve_config.py:27-34`);
  - every `ConfigError` sentence verbatim (`config_utils.py:17-34`, `:45-52`).
- `bmad/render.rs` (new):
  - the four token grammars (`render_skill.py:30-33`);
  - `resolve_short` (`:128-150`) and `resolve_config_value` (`:118-125`, with Q2's containment mode);
  - customization values (`:153-189`): strings; string lists as `- ` bullets with continuation lines indented two spaces; `_None._` for an empty list; review layers as ``#### <name> (`<id>`)`` sections with `Run only when:`; and "No active review layers. HALT with blocking condition `no active review layers`.";
  - `resolve_replacements` (`:192-229`);
  - `render_sources` (`:232-267`: one pass, longest token first, `{skill-root}` bound to the generation);
  - the generation identity and `manifest.json` (`:340-378`: canonical JSON, sorted keys, compact separators, non-ASCII kept);
  - `verify_existing` (`:270-292`).

  Everything is pure: it takes bytes and a `ProjectRoot` and returns outputs and a manifest. Publishing is 94.2's.
- `bmad/memlog.rs` (new): `split`, `render`, `touch` (the clock is a parameter), `init`, `append`, `set`, `entry_count`, `ack` (`memlog.py:81-187`).
- `bmad/help.rs` (new): the `bmad-help.csv` grammar.
  - 13 columns (`_bmad/_config/bmad-help.csv:1`), RFC 4180 quoting, `_meta` rows.
  - The phase graph: rows keyed `(skill, action)`; `preceded-by`/`followed-by` tokens `skill` or `skill:action`; `required`; phases kept as written (`anytime`, `plan`, `2-planning`, `ship`).
  - Lookups by menu code, skill and `skill:action`.
  - A dangling token is listed, never refused (`S/bmad-help/SKILL.md:43-50`).
  - The reader is written here; no CSV crate is added.
- `bmad/party.rs` (new): `alias`, `build_collective`, `resolve_members`, `group_menu`, `group_detail`, and the three projections (`resolve_party.py:90-273`).
- `bmad/UPSTREAM.md` (89.1's) gains:
  - the files this story read at v6.12.0 @05bfbd46 (`src/scripts/render_skill.py`, `memlog.py`, `resolve_config.py`, `resolve_customization.py` and their tests);
  - the party script and its test at the same tag;
  - `bmad-help/SKILL.md`'s CSV rules;
  - licence MIT (BMAD-METHOD `LICENSE`, © 2025 BMad Code, LLC), with the trademark notice: the name is used only to describe compatibility;
  - the deviations Q2 and Q3;
  - the revisit note that main@4f61d4e7 later moved the scripts to `skills/bmad/scripts/` and dropped `config.user.toml`, which this port does not follow.
- `src-tauri/crates/keeper-ported/tests/bmad_upstream_*.rs`, `bmad_parity_*.rs`, and `tests/fixtures/bmad/`.
  - Inputs copied from this repo's `_bmad/`, from `tgdrive:_bmad/custom/config.toml` and from the installed skills `S/bmad-build`, `S/bmad-agent-architect`, `S/bmad-architecture` and `S/bmad-party-mode`. They are MIT, carried with their notice.
  - `expected/*.json`, generated by `tools/bmad-port-fixtures.py`. That script imports the scripts in process, with the same root strings.

**Acceptance:**
1. **BMAD's own tests pass in Rust.**
   - **What is ported:** every test function of `test_config_utils.py`, `test_resolve_config.py`, `test_resolve_customization.py` and `test_memlog.py` at v6.12.0 @05bfbd46, and the 17 tests of `S/bmad-party-mode/scripts/tests/test_resolve_party.py`.
   - **How:** each becomes a Rust test of the same name, every assertion kept. That includes `test_filesystem_layer_precedence` (four layers; the custom user layer wins). A case that tests argv parsing or stdout encoding becomes a test of the function the CLI calls, and `UPSTREAM.md` names each such mapping.
   - **Tests:** `bmad_upstream_config_utils`, `bmad_upstream_resolve_config`, `bmad_upstream_resolve_customization`, `bmad_upstream_memlog`, `bmad_upstream_resolve_party`.
   - **Risk:** an upstream case dropped silently. Review checks `UPSTREAM.md`'s list against the upstream files.
2. **The four-layer merge matches the resolver on real configuration** (G4 §2). Fixture: this repo's `_bmad/config.toml` and `config.user.toml`, plus `tgdrive:_bmad/custom/config.toml` as the custom layer.
   - **Expected:** `load_central_config` equals `resolve_config.py`'s full dump, and its `--key agents`.
   - **Merge rules:** scalars override; tables deep-merge; arrays of tables keyed by `code`, else by `id`, replace in place or append; other arrays append.
   - **Errors:** a keyed array whose identifier is not a string, or is empty, refuses with the Python's sentence.
   - **Literal root:** values keep a literal `{project-root}`. `modules.bmm.planning_artifacts` is `"{project-root}/_bmad-output/planning-artifacts"`, as G4 observed.
   - **Tests:** `central_config_matches_resolve_config` and `keyed_array_identifier_errors`.
   - **Risk:** TOML value fidelity (integers, booleans, arrays of strings like `primary_platform`, `config.toml:35`). The test compares as JSON values.
3. **Overlays match the resolver.**
   - **Agent:** `load_customization` over `S/bmad-agent-architect/customize.toml` with `--key agent` equals `resolve_customization.py`'s output. The menu keeps `CA → bmad-architecture` and `IR → bmad-sprint-planning` (G4 §1).
   - **Workflow, real overlay:** over `S/bmad-architecture` with `tgdrive:_bmad/custom/bmad-architecture.toml` as the custom layer, `--key workflow` equals the resolver's output.
   - **Tests:** `agent_customization_matches_the_resolver` and `workflow_customization_with_a_real_overlay`.
4. **Render tokens match `render_skill.py`, token by token.**
   - **Run:** for two skills — `S/bmad-build` (format B), against a fixture central config with the gds duplicates removed, and a synthetic fixture skill exercising every token kind — `resolve_replacements` and `render_sources` with `ProjectRoot::Absolute("/fixture-root")`.
   - **Expected:** the Python's `_resolve_replacements` and `_render_sources` outputs, byte for byte, including `manifest.json`'s `inputs.resolved_values` and `outputs` hashes.
   - **Refusals, each with the Python's sentence:**
     - a missing `{{config.a.b}}`;
     - a missing `{{.key}}`;
     - an undeclared `[[bmad-snapshot:x.md]]`;
     - an unsupported default type;
     - a review layer without `instruction`;
     - a duplicate review-layer `id`.
   - **Keeper mode:** with `ProjectRoot::Session("60-sessions/active/s/artifacts")`, a value resolving outside the session is refused with "`config.<path>` must resolve inside this session: <value>" (Q2).
   - **Tests:** `render_tokens_match_render_skill`, `render_refusals_match_render_skill`, `render_in_a_session_stays_inside_it`.
   - **Risk:** the real `bmad-build` sources — eleven step and reference files with every token kind G4 lists.
5. **The defect, reported as BMAD reports it.**
   - **Fixtures:** this repo's real `_bmad/config.toml`, read at test time from `$CARGO_MANIFEST_DIR/../../../_bmad/`, and a fixture copy of tgdrive's central layers.
   - **Short keys:** `resolve_short` returns `RenderError::Ambiguous` for `implementation_artifacts`, `planning_artifacts` and `project_knowledge`. The sentences are the three quoted in *The BMAD config defect*, byte for byte.
   - **A full render:** rendering `S/bmad-build` against the same configuration fails before producing any output, with `implementation_artifacts`'s sentence.
   - **Explicit paths still work:** `{{config.modules.bmm.implementation_artifacts}}` still resolves.
   - **Tests:** `duplicate_module_keys_are_refused_as_render_skill_refuses` and `module_scoped_paths_still_resolve`.
   - **Risk:** the installer-written file, not a fixture. If the operator fixes the template (OA-94-1), this test switches to the fixture copy, which keeps the defect and records why.
6. **The memlog line grammar.** After acceptance 1:
   - **Round trip:** `render(split(x))` over the real `_bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/.memlog.md` (G4 §3) is byte-identical to `memlog.py`'s `render(*split(x))`.
   - **Append:** collapses whitespace runs and newlines into one line; tags `(type)`, `(type by who)` and `(by who)`.
   - **Set:** keeps `updated` last.
   - **Init:** refuses an existing file with "error: <path> already exists; use append/set to update it".
   - **Frontmatter:** closes at the first line that is exactly `---`.
   - **Ack:** `{"ok": true, "memlog": <path>, "entries": <n>}`.
   - **Tests:** `memlog_round_trips_the_architecture_memlog`, `memlog_append_and_set`.
7. **The phase graph reads this repo's catalogue** (`S/bmad-help/SKILL.md:31-57`). There is no Python helper to match, so the documented rules are the oracle.
   - **Coverage:** every row of `_bmad/_config/bmad-help.csv` parses, including quoted descriptions with commas (`:4`).
   - **`bmad-build`** (`:27`): phase `ship`, `preceded-by` `bmad-sprint-planning`, `followed-by` `bmad-code-review`, `required` true, output location `implementation_artifacts`.
   - **`bmad-sprint-planning`'s two rows** are told apart by action (`status` at `:20`, empty at `:26`).
   - **`bmad-agent-builder:quality-analysis`** resolves to its row (`:5`).
   - **Menu code** `CA` → `bmad-architecture`.
   - **`_meta` rows** carry a module's documentation URL and are not skills.
   - **Unknown tokens:** a token naming no row is reported as dangling.
   - **Test:** `phase_graph_reads_this_repos_catalogue`.
   - **Risk:** the real 84-row file.
8. **The party roster matches `resolve_party.py` on this install.**
   - **Default projection:** for this repo's `[agents.*]` with `S/bmad-party-mode/customize.toml`, it equals the script's output: `party_mode: "session"`, `active: "installed"`, the installed members in order, two groups (G4 §4).
   - **`--list-groups` and `--party <id>`:** equal the script's, including `unknown_group`.
   - **Custom members:** a custom member matched by code, alias or name overrides in place and keeps the installed fields it omits.
   - **Test:** `party_roster_matches_resolve_party`.
9. **Pure and licensed.**
   - `check:ported-pure` stays green.
   - 89.1's `UPSTREAM.md` test passes, reading `licence: MIT` for `bmad/`.
   - `cargo deny check` passes.
   - The crate gains no dependency outside the workspace's locked `toml`, `serde_json` and `sha2`. The CSV reader is written in `help.rs`.
   - Proof: `cargo tree -p keeper-ported` pasted in the PR.

**Shell crate:** does not touch it.

**binds:** FR-800, NFR-119, AD-396, AD-397

### 94.2 — The workflow tool surface

**Intent:** "i want to support what bmad method have in the workflow"; "nixi will alsways be a proxy between tgorka and rest of the system". **Rung:** **epic94-bmad**. AD-397 (the closed vocabulary and the map of BMAD's assumptions), AD-380 (`ask_human`); FR-801.

**Files:**
- `keeper-core/src/agents/workflow.rs` (new, pure):
  - `CAPABILITIES`, the 19 rows above;
  - `capability_answer(cap, offered) -> Tool | Refusal(sentence)`;
  - the tool specs and argument validation for `bmad_config`, `bmad_render`, `bmad_memlog`, `bmad_party`, `skills_list`, `skill_view` and `ask_human`;
  - the session-frame lines for a workflow session: both roots (Q1), `checkpoints`, whether a person is in the chain, and the helper probe.
- `keeper-core/src/agents/ask.rs` (new, pure):
  - the ask record `{id, question, choices, default, requested_of: <person>, via: <proxy>}`;
  - who answers: the first person found by walking `requested_by` up the `parent` chain, else nobody;
  - the encrypted content `dev.keeper.agent.ask: {id, question}` (*Matrix events*);
  - matching an answer to `choices` by number or folded text.
- `keeper-agent/src/bmad.rs` (new):
  - the four tool handlers over `keeper-ported::bmad`;
  - each reads `<drive>/_bmad/` and the running workflow's folder through `browse::resolve`;
  - publishes a render generation into `workspace/bmad-render/` (Q2), refusing an existing generation whose manifest differs (`verify_existing`);
  - writes `.memlog.md` through 90.2's runtime (Q3).
- `keeper-agent/src/skills.rs` (new):
  - `skills_list()`, the index of the skills offered to this agent (89.3), each refused skill with its reason;
  - `skill_view(name, path?)`, `SKILL.md` or one file inside the skill, read through `browse::resolve` under AD-159's caps (Hermes' progressive loading, research §9.1).
- `keeper-agent/src/ask.rs` (new):
  - `ask_human`: the check against the proxy's audience (92.6), the invite, parking through 93.2's `Parked`, and resuming on the answer;
  - the proxy side: an incoming ask becomes a `peer` line in the proxy's DM carrying the asking session's label — so that turn of the DM runs at the label's integrity, which resets at the person's next line (92.6, R28 S-09) — and the proxy answers it with 92.1's `reply`, addressed by the ask's id. A proxy copy invited only for the ask leaves the room after its `reply` (R28 S-27), so it stops receiving the room; one already a member (the requesting agent) stays.
- `docs/agents.md` § *Workflows* (the capability map, the BMAD tools and `ask_human`).

**Acceptance:**
1. **Every capability has a tool or a sentence.**
   - `CAPABILITIES` has exactly G4 §5's 19 rows, in order.
   - Every tool it names is in AD-397's vocabulary.
   - For each row, `capability_answer` returns the tool when it is offered, and otherwise the row's sentence naming the tool.
   - **Test:** `every_bmad_capability_has_a_tool_or_a_sentence` (pure; the vocabulary list is the one 89.3 validates `[tools].allow` against).
   - **Risk:** a capability that silently has neither. The test fails when a row is added without an answer.
2. **`bmad_config` answers as the resolvers do.**
   - `bmad_config({scope: "central"})` returns `{config: <resolve_config.py's dump>, roots: {install, output}}`.
   - `{scope: "central", keys: [...]}` returns only the keys found (`resolve_config.py:70-76`).
   - `{scope: "customization", key: "agent"}` resolves the running workflow folder's layers.
   - With no `<drive>/_bmad/config.toml`, the call is refused with "required TOML file not found: <drive-relative path>".
   - **Test:** `bmad_config_is_the_resolvers` (keeper-agent, a temp drive holding the 94.1 fixtures).
3. **`bmad_render` publishes once and halts as BMAD halts.**
   - On a fixture drive whose config has no duplicates, `bmad_render` publishes `workspace/bmad-render/bmad-build/<gen20>/` with `manifest.json` and returns `read and follow <path>/workflow.md`.
   - A second call returns the same path and writes nothing.
   - A planted generation whose files were edited is refused with "generation output hash mismatch: …".
   - On the defective config (94.1 #5), the result is a refusal beginning ``HALT: ambiguous config value `implementation_artifacts` ``, and nothing is written.
   - **Tests:** `bmad_render_publishes_one_generation`, `bmad_render_halts_on_the_duplicate_keys`.
   - **Risk:** a real file tree and a real hash check.
4. **`bmad_memlog` is the memlog CLI, inside the session.**
   - `init`, `append` and `set` on `artifacts/<run>/.memlog.md` write through 90.2's runtime.
   - Each call is one atomic replace (temp file, fsync, rename, as `memlog.py:122-129` writes).
   - It returns `memlog.py`'s ack.
   - A path anywhere but `artifacts/` (or any dotted name other than `.memlog.md`) is refused with the Hidden sentence (Q3).
   - **Tests:** `bmad_memlog_writes_only_the_memlog`, `bmad_memlog_survives_a_crash_mid_write` (the process is killed between the temp write and the rename, and the old file reads whole).
5. **`bmad_party` and `skills_list` / `skill_view`.**
   - **`bmad_party`:** returns 94.1's projections for the drive's install.
   - **`skills_list`:** lists exactly the skills 89.3 offers to this agent (`[tools].skills`), with refused skills and their reasons — among them a skill an agent proposed that no person has adopted yet ("proposed by <agent> on <date>; not offered until a person adopts it", 95.2, R28 S-12).
   - **`skill_view`:**
     - `skill_view("x")` returns the body;
     - `skill_view("x", "references/a.md")` returns that file;
     - `skill_view("x", "../y/SKILL.md")` is refused by `browse::resolve`;
     - a skill under `_skills/.archive/` is not found (95.3), and a skill still carrying `metadata.keeper_proposal` is refused as not adopted.
   - **Logging:** every `skill_view` is a `tool_call` line, so the log records which skill bodies an agent read.
   - **Tests:** `skills_list_is_the_offered_index`, `skill_view_stays_inside_the_skill`.
6. **`ask_human` reaches the person through their proxy, and only through it** (AD-380).
   - **The ask:** in a steward's session requested by `@tgorka` through Nixi, the call:
     - posts `m.room.message` with `dev.keeper.agent.ask: {id, question}` into the session room, inviting `@nixi` if absent;
     - writes an `ask` record in the log;
     - parks the run (`run: blocked`, detail "waiting for tgorka, through Nixi"), holding no thread (93.2).
   - **The relay:**
     - Nixi's host logs the question as a `peer` line in Nixi's DM session, carrying the steward session's label;
     - Nixi asks in its own voice;
     - the person's answer is relayed back with `reply` addressed by the ask's id: the relayed bytes are the person's own sealed message in Nixi's DM after the question was shown, verbatim, composed by the host — the model never authors them; `reply` at most picks which of the person's messages answers (else the host relays their first), and the choice is the host's match of those bytes against the ask's choices (R199, amending R100);
     - it lands in the steward's session as a `peer` line `{sender: @nixi, text, ask: {id}}`, and the run resumes on whichever host holds the claim;
     - Nixi's copy, invited for the ask, leaves the steward's room after its `reply` (S-27).
   - **The sink:** the ask is a send into Nixi's DM, whose audience is checked against the session label (AD-391). A session labelled `{tgorka, marta}` asking through Nixi (audience `{tgorka}`) passes. A session whose readers exclude tgorka is blocked with the reason.
   - **Tests:**
     - `ask_human_parks_and_resumes_through_the_proxy` (keeper-agent, two host runtimes and an in-process homeserver fake), which also asserts that Nixi is not a member of the steward's room after the relay;
     - `ask_human_is_a_send_into_the_proxys_dm` (pure `check_sink` case);
     - `a_relayed_ask_taints_one_turn_of_the_dm` (keeper-agent): an ask from an `untrusted` session makes the DM turn that relays it `untrusted`, and the person's next line returns the DM to `owner` (S-09);
     - the same flow against `keeper-test-synapse` (OA-94-3), `ask_human_round_trip_on_synapse`, `#[ignore]` by default and named in the PR.
   - **Risk:** a real Matrix server, real encryption, a real invite.
7. **Choices, defaults and nobody to ask** (Q4).
   - With `choices: ["Continue", "Stop"]`, an answer of `1`, `continue` or `Continue` matches `Continue`. An answer matching no choice returns the text with `choice: null`.
   - In a session with no person in the chain, or with `checkpoints = "unattended"` (94.3), the call returns `default` at once. It writes the session's `unattended` fact, which 93.4's raise reads, and the raise is applied once.
   - With no default, the call is refused with Q4's sentence.
   - **Test:** `ask_human_defaults_and_choices` (pure plus keeper-agent).
8. **Only agents get these tools.** `offer_tools` (`bots/tools.rs:613`) offers none of the BMAD tools, `ask_human`, `skills_list` or `skill_view` to any ⌘9 bot.
   - **Test:** `bots_never_offer_workflow_tools` (keeper-core).

**Operator-verified:**
- [ ] OA-94-3 done. Against `keeper-test-synapse`, `ask_human_round_trip_on_synapse` passes, and the PR pastes its output.

**Shell crate:** does not touch it.

**binds:** FR-801, NFR-115 (the ask's send), AD-397, AD-380, AD-391

**As built (rung `agents-94-config`, 2026-10-06).** Acceptance 1, 2, 5 and 8; 3, 4, 6 and 7 are rungs `agents-94-render` and `agents-94-ask`. 94.1's config and party ports, their upstream tests and `UPSTREAM.md` are the prep rung's (`agents-94-ported-prep`, 7349f274).
- **Symbols.** `keeper_core::agents::workflow` — `CAPABILITIES` (19 rows: `assumes`, `tools`, `sentence`, `names` — the tools the sentence stands in for), `capability_answer`, `frame_lines` (R96 roots and the map, for a turn offered any `bmad_*` tool, rendered by `SessionFrame.bmad` at the end of slot 5), `specs`, `parse_config`/`parse_party`/`parse_list`/`parse_view`. `keeper_agent::bmad::BmadTools` (`bmad_config`, `bmad_party`, dispatching `skills_list`/`skill_view` to `keeper_agent::skills`), routed in `AllowedTools::run_named` with a T0 row and one audit row each (`AgentTool::{BmadConfig, BmadParty, SkillsList, SkillView}`, `approval::summary_of` templates); the files a call read join the label in the reporter (`file_read_label`). `AgentDeps.drive_root`; keeper-agent depends on keeper-ported and toml; `keeper-agentd status` counts the four as implemented.
- **Grammar as built.** `bmad_config({scope: "central"|"customization", skill?, keys?})`: `keys` (a list) serves both scopes — the `key: "agent"` of acceptance 2 is `keys: ["agent"]`; `skill` names an offered skill under `_skills/`, else the session's workflow folder. The central result is `{config, roots: {drive, install, output, paths}, overlays?}`. `bmad_party({list_groups?, party?})` reads `list_groups` over `party` as the script does; its customization is the offered skill `bmad-party-mode`, or the running workflow of that name. `skill_view` serves offered skills only.
- **Corrections (codemap §3).** Row 11's sentence names `delegate` (a delegated session's next round, R49), not `reply`; row 12's sentence includes `auto` (row 29); the sentences of the "—" rows were written so every row answers (acceptance 1's risk). Row 17: the `metadata.keeper_proposal` clause of acceptance 5 moves to 95.2 (DW-531). Row 24: acceptance 8 is R38's `the_bots_vocabulary_is_the_drives_seven_verbs`; no second test. Row 25: row 18 states `Now:`; the turn's token bound joins it in rung `agents-94-helpers`.
- **Tests.** 1: `workflow::tests::every_bmad_capability_has_a_tool_or_a_sentence`, `the_frame_states_the_roots_and_the_map_for_its_offer`. 2: `bmad::tests::bmad_config_is_the_resolvers` (the 94.1 goldens under `config`, roots, customization by skill and by workflow, the drive-relative refusal), `overlays_are_read_only_from_the_drive` (R97). 5: `bmad::tests::bmad_party_is_resolve_party`, `skills::tests::skills_list_is_the_offered_index`, `skill_view_stays_inside_the_skill`, and `agent_turns::skill_view_is_a_logged_read_that_joins_the_label` (offer, frame, `tool_call` line, label). Tiers: `tier::tests::every_tool_has_a_tier_row`. 8: R38's test, unchanged and green.

**As built (review fixes R94C-01…08, rung `agents-94-config`, 2026-10-06, R195).** Supersedes the symbols above where they differ: `Capability` is now `assumes`, `tools`, `said: [(When, sentence)]` (`When::{Always, Without(tools), With(tool)}`), `CapabilityAnswer` is `tools`, `sentences`, `missing`; `file_read_label` is gone.
- **01 provenance.** `bmad::FileRead` (`requested`, `landed` — canonical drive-relative through every link —, the bytes' `okf` and `card_untrusted`, taken at the read by `FileRead::of` in `BmadTools::layer` and `skills::view`); the reporter labels each with `FileRead::label` (requested ⋈ landing, `untrusted` when the landing is unknown) and names `FileRead::path`; nothing is reopened. Test: `bmad::tests::a_read_is_labelled_by_its_landing_and_its_bytes` (an installer layer linked into `00-inbox/`; a marked file replaced or removed after the read; a clean file replaced by a marked one; unknown landing).
- **02 grants.** `agent::agent_grants` (the turn's `AgentGrants`, shared by `arm_agent` and the host) and `grant_read`: `arm_agent` offers `workflow::specs` only where a `drive_read` of the home drive would be allowed, and `AllowedTools::run_named` refuses a BMAD/skill call with the grant layer's sentence before its read. Test: `agent_turns::bmad_tools_read_the_home_drive_only_under_its_grant` (home drive out of scope: not offered, no frame, `skill_view` and `drive_read` of the same file both refused, nothing reaches the model or the label).
- **03 frame.** `workflow::FRAMED` (the four `bmad_*`, `skills_list`, `skill_view`, `workflow_start`); the roots line names `bmad_config` only when it is offered. Tests: `workflow::tests::the_frame_states_the_roots_and_the_map_for_its_offer`, and `agent_turns::skill_view_is_a_logged_read_that_joins_the_label` now runs an agent allowed `skill_view` alone.
- **04 answers.** Row 9 answers inline invocation (`With(skill_view)` / `Without(skill_view)`) and the handoff (`Without(workflow_start)`) apart; rows 11 and 12 say `delegate` only `With(delegate)`, row 12 states its absence. Test: `workflow::tests::every_bmad_capability_is_answered_for_every_offer` — for the empty offer, each vocabulary tool alone, an unrelated MCP tool and everything: never neither, and every tool a sentence names is offered or in `missing`; row 9's partial offers by name.
- **05 web search.** Row 14 has no tool and always says no search is established; row 15 alone answers with MCP tools. Same test: an unrelated `mcp__github__get_me` changes nothing of row 14.
- **06 party overlays.** `bmad_party` prints the projection under `party` and `overlays` beside it as `bmad_config` does; DW-530 closed. Test: `bmad::tests::bmad_party_says_which_overlays_it_did_not_read`; goldens of `bmad_party_is_resolve_party` compared byte for byte under `party`.
- **07 UTF-8.** `skills::view` inspects the UTF-8 error once: an incomplete final character at the cap is trimmed, any other error refuses as not text. Test: `skills::tests::skill_view_cuts_at_a_character_and_refuses_bad_bytes` (a split two-byte character; a bad byte first, mid-file past the cap, and below it).
- **08 pins.** Removed: the heading split and full sentence of `skills_list_is_the_offered_index`, the frame test's prefixes, positions and formatted lines, keeper-authored refusal sentences in `skills`, `bmad` and `workflow` tests. Kept: the upstream goldens and the drive-relative missing-layer refusal.

**As built (rung `agents-94-render`, 2026-10-06).** Acceptance 3 and 4. 94.1's `render.rs` and `memlog.rs` (with their upstream tests and goldens, acceptance 1, 4, 5, 6 of 94.1) are the prep rung's (`agents-94-ported-prep`, 7349f274); this rung is their consumer.
- **Symbols.** `keeper_core::agents::workflow` — `BMAD_RENDER`, `BMAD_MEMLOG`, `RenderCall`, `MemlogCall`/`MemlogTarget`/`MemlogCommand`, `parse_render`, `parse_memlog`, their specs. `keeper_core::sessions::files` — `check_memlog` and `compile_memlog` (the memlog door: basename exactly `.memlog.md` under `artifacts/`, any depth; a new file, or a write guarded on the bytes read, R120), `check_agent_file`/`compile_agent_file` and `AGENT_ARTIFACT_EXTENSIONS` (R112), `FileVerbError::{AgentExtension, NotMemlog}`. `keeper_agent::sessions::write` — `memlog_write`, `publish_generation` (staging folder + `MoveDir` in one journaled plan; an existing folder handed to `verify_existing`), `session_write`'s artifact branch on `compile_agent_file`. `keeper_agent::sessions::exec::atomic_write` syncs the temp file and then the folder (codemap §0.8, §3 row 15). `keeper_agent::bmad::BmadTools` — `render` (`ProjectRoot::Session(<session>/artifacts)`, renderer identity `render::renderer_sha256()`, every refusal `HALT: <sentence>`, the answer `read and follow <drive-relative>/workflow.md`), `sources` (`_load_sources`, a source linking out of the skill refused), `memlog` (ack as `memlog.py` prints it; a session-relative or drive-relative path), `SessionFolder`; routed in `AllowedTools::run_named` as writes (home drive's readers at the sink, `lifting`, one audit row, the claim asked right before the effect). `AgentTool::{BmadRender, BmadMemlog}` at T1 with `approval::summary_of` templates (R105). `keeper-agentd status` counts both as implemented (`bmad::serves`).
- **Grammar as built.** `bmad_render({skill?})`: the offered skill under `_skills/`, else the session's workflow. `bmad_memlog({command: init|append|set, workspace | path, fields? | text, type?, by? | key, value})`, argparse's rules: exactly one target, only the command's own flags.
- **Corrections (codemap §3).** Row 15: fsync added in `atomic_write`, so every session-runtime write (journal included) is durable. Row 16: `bmad_memlog`'s refusals are `Hidden`; `session_write`'s dotted refusal ("not a plain path inside the session") is unchanged. Acceptance 3's "refused with 'generation output hash mismatch: …'" reads `HALT: generation output hash mismatch: …`, as the CLI prints every refusal. Acceptance 4's "the process is killed": no kill harness exists; the journal the write leaves is planted and resumed (codemap §4).
- **Tests.** 3: `bmad::tests::bmad_render_publishes_one_generation` (generation path, manifest, `{project-root}` and snapshot binding, reads joined, second call byte- and mtime-identical with no journal, planted edit refused), `bmad_render_halts_on_the_duplicate_keys` (nothing under `workspace/`; no claim, nothing written), `a_render_source_never_leads_out_of_its_skill`. 4: `bmad::tests::bmad_memlog_writes_only_the_memlog`, `bmad_memlog_survives_a_crash_mid_write`; `files::tests::only_the_memlog_under_artifacts_is_dotted`. R112: `files::tests::an_agents_artifact_takes_bmads_output_kinds`, `cards::tests::session_write_takes_bmads_outputs_under_artifacts`. Grammar: `workflow::tests::the_tools_arguments_read_as_their_scripts_flags`. Tiers: `tier::tests::every_tool_has_a_tier_row`.

**As built (review fixes R94R-01…08, rung `agents-94-render`, 2026-10-06, R196).** Supersedes the "staging folder + `MoveDir`" of the section above.
- **01 staging.** `keeper_core::sessions::plan::PlanStep::{MkDirNew, PublishDir}`; `sessions::write::publish_generation` stages in `.staging-<generation>-<ulid>` made by `MkDirNew` (a new, real folder; a resume takes only a real one, never a link), every staged subfolder too, and publishes with `PublishDir`, which moves only once the staged tree is exactly the plan's files by SHA-256 (no link, no other entry, nothing extra) and on resume recognises only its own publication. Tests: `exec::tests::a_generation_is_published_only_as_exactly_what_was_staged`, `a_new_folder_is_never_a_link`; `bmad::tests::a_render_stages_only_in_a_folder_of_its_own` (a link into another session and a stale folder with an extra file at the staging names). Leftover staging from a refused plan: DW-552.
- **02 inspection.** `sessions::exec::regular_files` errors on a link, another kind of entry, a non-UTF-8 name or a failed read; `publish_generation` hands that sentence to `verify` ("corrupt existing generation …: <file> is a link"). Test: `bmad::tests::a_link_in_a_generation_is_never_verified` (an empty output replaced by a link to non-empty bytes).
- **03 durability.** `exec::make_dirs` (each new folder, ancestors included, synced into its parent; `MkDir`, `atomic_write`), `MkDirNew`'s and `MoveDir`'s/`PublishDir`'s parent syncs (`sync_moved`) before the cursor advances; `run_from` syncs the journal's folder after removing it. Test: `exec::tests::a_folder_the_journal_counts_is_synced_where_it_was_made` (an unsyncable parent fails the step: a new ancestor of `artifacts/run`, a generation's move). The removal's sync has no plantable fault: DW-554.
- **04 memlog.** `sessions::write::memlog_write` takes only `NotFound` as absent; `files::compile_memlog` creates a new memlog with `PlanStep::CreateFile` (refused when a file appeared, its own bytes accepted on resume). Tests: `bmad::tests::an_unreadable_memlog_is_never_started_over` (invalid UTF-8, write-only), `exec::tests::a_created_file_never_replaces_one_that_appeared`, `files::tests::only_the_memlog_under_artifacts_is_dotted`.
- **05 addressing.** One rule, `sessions::write::in_session`: `session_write` and `card_update` (`CardTools::run`, `CardTools::session_dir`), their audit target and landing (`AllowedTools::card_at`) and `bmad_memlog` take a session's file session-relative or drive-relative through the session's folder; render keeps R96's drive-relative write locations. Tests: `cards::tests::session_write_takes_the_write_locations_bmad_names`; turns-level in `agent_turns::bmad_render_and_memlog_are_t1_writes_through_the_turn` (the render's stated location, Markdown and YAML, exact destination, no `60-sessions/` under the session).
- **06 sources.** `bmad::markdown_under` returns the sentence naming the folder whose listing, entry or type read failed; the render halts. Test: `bmad::tests::an_unreadable_source_folder_halts_the_render`.
- **07 declassification.** `BmadTools::prepare` renders before admission (`bmad::Write { at, effect }`): the audit target and the park's pin are the generation folder, the effect bytes `{drive, path, manifest_sha256}`; `BmadTools::write` publishes exactly that prepared generation; `AllowedTools::run_named` passes `prepared.effect` to the sink verdict. Tests: `agent_turns::parks::a_parked_render_publishes_only_the_generation_it_bound` (a source changed while the approval waits → `CHANGED`, nothing published; unchanged → that generation), `bmad::tests::a_renders_effect_is_its_generation`.
- **On R195.** Both writes ask `agent::grant_read` of the home drive before anything is read or prepared (`AllowedTools::run_named`), are in `workflow::FRAMED`, and keep each render source as a `bmad::FileRead` (`BmadTools::sources`), so its label is the read's own.
- **08 turns.** `agent_turns::bmad_render_and_memlog_are_t1_writes_through_the_turn` (offer, T1 `tool_call` lines, one classified audit row each naming where it wrote, the render's source joining the label before the next round) and `bmad_writes_are_refused_unoffered_or_beyond_the_label` (not offered and refused without the allow; beyond a narrowed label refused with Marta named, nothing written, one `Deny` row each). A claim lost between admission and effect is handler-level only: DW-553.

**As built (rung `agents-94-ask`, 2026-10-06).** Acceptance 6 and 7, by R99–R103, R105 and R113.
- **Symbols.** `keeper_core::agents::ask` — `AskContent`/`AnswerContent` (`events::ASK`, `events::ANSWER`), `ask_text`, `ask_content`/`read_ask`/`enveloped_ask`, `answer_content`/`read_answer`, `offered` (R102 by kind), `answerer` (R102 over the dispatch chain), `proxy_dm` (the sink), `choice_of`, `waiting_detail`, `NO_ONE_TO_ASK`. `keeper_core::agents::workflow` — `ASK_HUMAN`, `ask_spec`, `parse_ask`/`AskCall`. Log: `LineKind::Ask`/`AskBody`/`AskState` (`asked`, `sent`, `answered`, `defaulted`, `refused`), `PeerAsk {id, question, room, label}`, `PeerBody.answers: PeerAnswer {id, choice}`; replay reads an answer as one. `AgentClient::leave`; `AgentTool::AskHuman` (T1). `keeper_agent::ask` — `AskTools` (the call), `relay` (`reply(text, ask)`), `OpenAsk`, `Relay`, `ask_txn`. `keeper_agent::rooms` — `Arrival::{Ask, Answer}`, `Disposition::{Ask, Answer}`, invite arm (d), `admit_ask`. `keeper_agent::delegate` — `ReplyOffer`, `DelegationPort::{invite, leave}`, `TurnView::relay`. `keeper_agent::agent` — `SessionContext.{asks, relays}`, `TurnEnding::Asked` (the round gate's stop), `ServedSession::{answered, take_ask, send_asks}`. `keeper_agent::runtime` — `intercept_ask` (into the proxy's `main`, or the session that delegated into the room), `arrival_of`'s two rows, `ClientRooms::{invite, leave}`.
- **Corrections (codemap §3).** Row 18: no two-runtime in-process homeserver; `agent_turns` carries the events between two served sessions over port doubles, and `live_ask.rs` is the real path. Row 19: the answer is `peer {sender, text, answers: {id, choice}}` (`choice` added so the model reads the picked choice; `null` when none). Row 20/Q4: no `Parked`; the ask ends the turn (R99), whose run reads `blocked`. Row 21/Q5: the proxy answers with `reply(…, ask)` (R100). Row 22/Q6: arm (d) (R101). Row 23/Q8: no `unattended` fact is written here (DW-537). Acceptance 6's "posts … inviting `@nixi` if absent": the question goes in only once the proxy has joined (`ask sent`, at once or on the worker's clock), as a brief waits for its target (R29 F5), so its device holds the room's key.
- **Tests.** 6: `agent_turns::ask_human_parks_and_resumes_through_the_proxy` (invite, `ask asked`, `run: blocked` detail, the gate's one request, no send before the join, `ask sent` once, Nixi's `peer` line with the ask and Tola's label, the relay's `answer` into Tola's room and Nixi's leave, Tola's `peer {answers}` with `Continue`, `ask answered`, `run: running`, the model told the choice), `ask::tests::ask_human_is_a_send_into_the_proxys_dm`, `agent_turns::a_relayed_ask_taints_one_turn_of_the_dm`, `rooms::tests::invite_decision_table` (arm d), `an_ask_is_admitted_only_for_this_proxys_person`, `an_ask_is_a_turn_only_in_a_proxys_own_rooms`, and live `live_ask::ask_human_round_trip_on_synapse`. 7: `ask::tests::ask_human_defaults_and_choices`, `workflow::tests::ask_humans_default_is_one_of_its_choices`, `agent_turns::ask_human_defaults_and_choices`. R102: `ask::tests::the_head_of_the_chain_answers_through_their_proxy`, `every_session_but_a_proxys_own_is_offered_ask_human`, `keeper_agent::ask::tests::only_a_known_proxy_or_a_pinned_ones_relays`. Log: `log::tests::every_kind_round_trips_in_the_documented_key_order`, `agents_log::attachments_and_a_peer_question_replay_as_they_were_sent`. Tiers: `tier::tests::every_tool_has_a_tier_row`.

**As built (review fixes R94A-01…13, rung `agents-94-ask`, 2026-10-06, R199).** Supersedes the section above where they differ: the call publishes nothing — no invite, no send — and `DelegationPort::leave` is gone.
- **01 the relay is the person's.** `SessionContext.relays` hold the person's `user` messages since the question (`Relay.said`, kept from `user` lines of the session's requester after the ask's `peer` line); `keeper_agent::ask::relay` sends one of them — the one `reply({ask, text?})`'s `text` quotes, else the first (`ask::relayed`) — and refuses before the person spoke (`NOT_ANSWERED_YET`) or when `text` is none of theirs (`NOT_THEIR_WORDS`); the proxy's `ask answered` line carries the message relayed; the asking side's choice is `choice_of` over those bytes. `reply`'s relay spec makes `text` optional and says the model never writes the answer. Tests: `ask::tests::a_relay_is_only_the_persons_own_message`; `agent_turns::a_relay_carries_only_the_persons_own_message` (a reply before tgorka spoke, one naming `Stop` when he said `1`, one carrying a private sentence of the DM: refused, nothing sent; the relay sends his `1` alone).
- **02 pinned proxies.** `ask::with_pinned_proxies`/`proxy_audience`: a pinned `[[trust]]` entry's proxy no mounted drive names — and pinned for one person only — is checked as that person's proxy, audience the person: in the invite (`DelegationPort::invite`'s audience; `keeper_core::agents::matrix::invitee_sink`, the client's own check) and in every room check (`ServedSession::gate`). Tests: `ask::tests::a_pinned_proxy_is_its_persons_audience_and_no_one_elses`, `agent_turns::a_pinned_proxy_is_asked_from_outside_the_room` (the fake's invite applies `invitee_sink` as the client does).
- **03 routing.** `DelegationPort::watch(child, parent, kind)`; `runtime::Children` holds the parent's kind; `runtime::ask_target` routes to the parent only when it is the proxy's `main` or `conversation`, else to its DM (`route_ask`). Test: `runtime::tests::an_ask_goes_to_a_parent_only_when_it_takes_asks`.
- **04/13 intent first, one send boundary.** `AskTools::run` checks the DM sink and the room as it is at the call, writes `ask asked` (now with `card`) and `run: blocked`, and publishes nothing; `ServedSession::send_asks` — after each served arrival, at serve start and on the clock — syncs the log first, invites the proxy unless present, waits for its join, checks the label and the room at the send through `gate()` and sends under `ask_txn(id)`. Tests: `agent_turns::an_ask_is_on_disk_before_it_is_sent_and_sent_after_a_restart` (nothing published by the call with Nixi in the room; sent once after a restart under `ask-<id>`; never again once `sent` is logged), `a_question_the_room_no_longer_lets_in_is_never_asked_and_the_run_hears_it` (an outsider joins between the call and the send: nothing sent).
- **05 durable intake.** `runtime::recover_asks` (from `recover_briefs`, on the host's clock) over `AskRooms` (each joined, unserved session room once per start, again after a failed read, route or leave, or a relay), `asks_waiting` (each admitted ask no answer of this proxy's names), `route_ask`; a failed room read or DM lookup at the live interception marks the room to be read again; `SessionContext.asked_of_me` takes an ask once, relayed or not. Tests: `runtime::tests::a_read_back_finds_the_asks_still_waiting_for_a_relay`, `an_ask_room_is_read_again_until_it_settles`, and the duplicate/restart half of `agent_turns::a_relay_carries_only_the_persons_own_message`. Residue: DW-546.
- **06/07 scheduled cards.** `AskBody.card`/`OpenAsk.card` from `TurnTools.scheduled`; `ServedSession::holds_windows` (an approval waiting, or an ask of a card) feeds `Activity.parked` and ignores a later `Scheduled::Run` (`ASK_WAITS`); `ServedSession::go_on` sets that card `running`, runs the answer's (or refusal's) turn under it and `finish_scheduled`s it. Tests: `agent_turns::a_scheduled_runs_answer_finishes_its_card` (the next window ignored and the card untouched; after a restart the answer ends the card `review`); `ask_human_parks_and_resumes_through_the_proxy` now ends `running, review`.
- **08 answers wait for their round.** `Disposition::Answer` and `Disposition::Unasked` are served after the approval guard: held while a call waits. Test: `agent_turns::parks::an_answer_waits_for_its_rounds_parked_call` (ask + parked `card_update` in one round, the answer before and after the decision: three requests, every tool call resolved in each, the card ends `review`). A continuation still makes one more model request: DW-547.
- **09/10 leaving.** `DelegationPort::depart` marks the room for read-back; `recover_asks` leaves it only when `runtime::leaves` (it answered there, nothing waits, no session of its delegated into it), and a failed leave is read and tried again. Tests: `runtime::tests::a_proxy_leaves_only_after_its_last_relay`, `a_read_back_finds_the_asks_still_waiting_for_a_relay` (two asks, one relayed: stays). Backoff: DW-548.
- **11/12 refusals.** `Arrival::Unasked`/`Disposition::Unasked` and `agent::unasked_arrival`: a refusal at the send (the room, the invite's `Label`/`Forbidden`, a `TooLarge`/`Forbidden` send) is the run's next turn — a `peer` line in the agent's own name, then `ask refused` — never sent meanwhile (`ServedSession.refusing`); a `RateLimited` send waits its `retry_after_ms` (`Retry.asks_after`). The ask's text is bounded (`workflow::ASK_TEXT_BYTES`) and its content (`ask::ASK_EVENT_BYTES`) at the call. Tests: `agent_turns::an_ask_waits_out_a_rate_limit_and_stops_at_a_permanent_refusal`, `a_question_the_room_no_longer_lets_in_is_never_asked_and_the_run_hears_it`, `workflow::tests::an_ask_is_bounded_before_it_is_asked`. DW-535 narrowed to DW-546; DW-536 and DW-538 closed.
- **Rulings recorded.** R197 (the question goes in only after the proxy joined; `ask sent`), R198 (`dev.keeper.agent.answer {v, id}`, `peer.answers {id, choice}`, older `peer` ask lines reshaped under R69) and R199.

### 94.3 — `workflow.toml` and workflow cards; checkpoints through the proxy

**Intent:** "I want support for workflows definitions - i want to support what bmad method have in the workflow"; "I want also the scheduled job on the kaban to work on (tasks on keeper)". **Rung:** **epic94-workflows**. AD-398, AD-380, AD-387 (with Q5), AD-392's unattended raise; FR-802.

**Files:**
- `keeper-core/src/agents/workflow.rs` (94.2's) gains:
  - the `workflow.toml` grammar exactly as *Data formats* gives it;
  - `[[inputs]]` validation (`text | path | drive | session`);
  - `[[outputs]]` path expansion (`{{date}}`, `{{slug}}` only);
  - the start check (`tools` ⊆ the agent's `allow`; `drives` within scope);
  - the run id derivation (Q5).
- `keeper-agent/src/workflow.rs` (new):
  - `workflow_start` (Q6), refused in a proxy's `main` session (AD-380);
  - the workflow card runner over 92.3's due evaluation (Q5);
  - checkpoints through `ask_human` (94.2);
  - the closing check of declared outputs;
  - re-rendering a format-B generation on takeover (Q2).
- `keeper-agentd`'s `agents init` (91.5's verb): seeds `80-agents/_workflows/triage/` and `_workflows/dispatch/` (a BMAD-format `SKILL.md`, `steps/` and `workflow.toml`, each written by this story), never overwriting a file. Newly seeded steward `agent.toml` files get `[[menu]]` entries naming those workflows. Existing `prompt` menu entries (92.5) are left as the person has them, by agreement with the 91–93 lane.
- `src-tauri/crates/keeper-agent/tests/fixtures/workflows/`: copies of `S/bmad-create-epics-and-stories` (format C) and `S/bmad-build` (format B), each with a `workflow.toml`.
- `docs/agents.md` § *Workflows* (the header, cards, checkpoints, resume).

**Acceptance:**
1. **The grammar is closed** (*Data formats*).
   - **Refused, each with a sentence:**
     - an unknown key, by name;
     - `version = 2`, listed as unreadable;
     - `name` ≠ folder;
     - `description` over 280 characters;
     - an `entry` outside the folder or missing;
     - an input `type` outside the four;
     - an output path with a token other than `{{date}}`/`{{slug}}`;
     - a `tools` name outside AD-397's vocabulary;
     - a `trigger.schedule` at all (R200: "a schedule is a workflow card's `schedule:`, never the workflow's");
     - `checkpoints` other than `proxy`/`unattended`.
   - **Defaults:** `entry = "SKILL.md"`, `drives = ["home"]`, `trigger = {manual = true}`, `checkpoints = "proxy"`.
   - **Test:** `workflow_toml_grammar` (pure, one row per rule).
2. **A workflow the agent cannot run is refused for that agent, by name.**
   - A folder without `workflow.toml` is listed as "not a workflow: no workflow.toml".
   - A workflow whose `tools` include `run` started by an agent without `run` is refused: "`bmad-build` needs `run`, which `amelia` is not allowed".
   - **Test:** `a_workflow_is_refused_for_an_agent_lacking_its_tools`.
3. **`workflow_start` opens one session per call.**
   - **Inputs:** validated. A required input missing is refused. A `path` resolves through `browse::resolve` inside the drives in scope. A `drive` input must be in scope. A `session` input must name an existing session folder.
   - **The session:** kind `workflow`; session `agent.toml` carries `workflow = "<name>"` and `parent`; the brief carries the inputs.
   - **Idempotent:** the same call id makes one session (AD-368).
   - **Tests:** `workflow_start_opens_one_session_per_call_id` (keeper-agent, a real sessions zone in a temp drive, 90.2's runtime) and `workflow_start_validates_its_inputs`.
4. **Never inside the proxy's DM** (AD-380, P1). `workflow_start` called in a `main` session is refused ("a workflow is started by delegation or a card, never inside the DM"), even if a person added it to the proxy's `allow`.
   - **Test:** `workflow_start_is_refused_in_a_proxy_dm`.
5. **A workflow card runs once per window, in a fresh session** (Q5).
   - **The card:** `workflow: bmad-create-epics-and-stories`, an `assignee` and `schedule: "@daily"`.
   - **When due (92.3):** it opens one workflow session whose id derives from the card's id and the window; the card's `run:` goes `running`, and `last_run` is set.
   - **Two hosts:** due in the same window, they open one session between them (92.3's claim).
   - **`trigger.card = false`:** a card naming such a workflow ends `run: failed` with "`<name>` may not be started by a card".
   - **A schedule an agent wrote** (92.2, 92.3; R28 S-21): the same card whose `workflow:` or `schedule:` an agent set carries `scheduled_by` and opens no session in any window until a person's *Allow*; then it runs once in the next window.
   - **Tests:** `workflow_card_runs_once_per_window_in_a_fresh_session`, `a_card_naming_a_manual_only_workflow_fails_with_a_sentence` and `an_agent_written_workflow_card_waits_for_a_persons_tick`.
6. **Checkpoints reach the person through the proxy** (`checkpoints = "proxy"`). The format-C fixture's menu after step 2 (`[A] Advanced Elicitation [P] Party Mode [C] Continue`, G4 §3) is asked through `ask_human`, scripted by the fake model. The person's `C`, relayed by the proxy, resumes at step 3.
   - **Test:** `a_classic_checkpoint_reaches_the_requesters_proxy` (keeper-agent, the 94.2 fakes).
7. **An unattended workflow answers with defaults, held one tier stricter.** With `checkpoints = "unattended"`:
   - the same menu returns its default `C`;
   - the session's `unattended` fact is set once, at open;
   - a write that needs a person is held one tier stricter, once (93.4's raise): a run is a hop deep, so a T2 write is T3 attended or not, and the audit row's `raised_by` names `delegated,unattended` (R171: a T0 or T1 call is never raised); the `tool_call` line records the tier.
   - **Test:** `an_unattended_workflow_takes_defaults_and_is_raised_once`.
8. **Progress survives a change of host** (AD-398, §10.3).
   - **Format C, the move:**
     - host A runs the fixture to `stepsCompleted: [1, 2]` in `artifacts/_bmad-output/planning-artifacts/epics.md`;
     - its claim expires;
     - host B takes the session over (90.6) on its own checkout of the same remote;
     - B replays the log and continues at step 3.
   - **Format C, the evidence:**
     - the file's frontmatter becomes `[1, 2, 3]`;
     - the lines carry epoch + 1;
     - nothing of A's is rewritten.
   - **Format B:** B renders the generation again before its first turn and reads the same paths. If the drive's `_bmad/config.toml` changed in between, the generation hash differs, and the resume is refused with "this workflow's sources or BMAD configuration changed since this run rendered them; start it again".
   - **Tests:** `a_classic_workflow_resumes_on_the_other_host_from_its_own_files` and `a_rendered_workflow_is_rendered_again_after_takeover` (keeper-agent, two checkouts of one local bare remote through keeper-sync's engine).
   - **Risk:** real git sync between two checkouts, not a shared directory.
9. **Declared outputs are checked at close.**
   - Each `[[outputs]]` path, tokens expanded, must exist under `artifacts/` when the run closes.
   - A missing one is named in the run's reply and on its `run: review` line with the sentence "declared output `<path>` was not written" (R107: the run closes at its `reply`; no `close` line), and the card's `run:` is `review`.
   - **Test:** `declared_outputs_are_checked_at_close`.
10. **The stewards' workflows are seeded, never overwritten.**
    - `keeper-agentd agents init` on an empty zone writes `_workflows/triage/` and `_workflows/dispatch/`, each with a valid `workflow.toml` (#1's grammar).
    - Run again, it writes nothing.
    - A person's edited `SKILL.md` is untouched.
    - **Test:** `agents_init_seeds_the_steward_workflows_without_overwriting` (keeper-agentd, a temp drive).

**Operator-verified:**
- [ ] OA-94-2 done. In tgdrive, `_workflows/bmad-create-epics-and-stories/` carries a `workflow.toml`.
- [ ] A card for it, assigned to a specialist and started manually from the board, reaches its first checkpoint as a question from Nixi on the phone.
- [ ] Answering `C` there resumes the run on electra.

**Shell crate:** does not touch it.

**binds:** FR-802, AD-398, AD-380, AD-387, AD-392

**As built (rung `agents-94-workflows`, 2026-10-06).** Acceptance 1–10, by R103 (its `checkpoints` half), R104, R105 (`workflow_start`), R106–R108, R83 as extended, R200 and R201.
- **Symbols.** `keeper_core::agents::workflow` — `Workflow`/`WorkflowInput`/`WorkflowOutput`/`Trigger`/`InputKind`, `parse_workflow_toml` (closed, R108, R200), `start_check`, `check_inputs`, `brief`, `expand_output`, `missing_output`, `run_id`/`start_id`, `start_spec`/`parse_start`, `WORKFLOW_START`, `NOT_A_WORKFLOW`, `IN_THE_DM`, `CONTINUATIONS_PER_RUN`, `CONTINUE`. `session::Checkpoints` and the `checkpoints` root key; `delegation::workflow_session`; `tier::Context::of_session` reading `checkpoints`; `AgentTool::WorkflowStart` (T1). `keeper_ported::bmad::help` (the catalogue, R201's consumer). `keeper_agent::workflow` — `WorkflowTools` (the call), `open`/`Opening`/`Opened`, `for_card`, `read_workflow`, `resolve_name`, `declared_outputs`, `checkpoints_of`, `not_by_card`/`not_by_hand`/`not_found`, `SOURCES_CHANGED`. `keeper_agent::agent` — `Arrival::Workflow`/`Disposition::Workflow`, `WorkflowStep`, `workflow_arrival`, `ServedSession::workflow_arrivals`/`workflow_step`/`continuation`/`rerender`/`run_workflow_card`, `SessionContext::continuations`/`rendered`; `DelegationPort::create(kind, …)`; `DelegateTools::with_outputs` (R107 in `reply`); `ask::can_ask`, `AskTools.unattended`. Seeds: `seed::STEWARD_WORKFLOWS` (`_workflows/triage/`, `_workflows/dispatch/`), `WT`/`WD` in `steward-menu.toml`; `dev/mock-shell.ts`'s seeded list.
- **Corrections.** Codemap §3 row 30 / Q13: no schedule in the header (R200). Row 33: B's turn after a takeover starts from the relayed answer; an interrupted run is resumed once (R106). Row 34: the takeover tests run both hosts over one folder at two epochs (DW-539). Row 35: no `close` line; the outputs check is in `reply` (R107). Row 36: `WT`/`WD` sit beside `TR`/`DS`/`HV`. Acceptance 5's "two hosts": 92.3's claim, plus the run's derived id (DW-540). Acceptance 7: a run is a hop deep, so the unattended raise adds a reason, not a tier (R171). A card's run is checked against what the run is offered — its `allow` and `ask_human` (R102) — not `allow` alone.
- **Tests.** 1: `agents::workflow::tests::workflow_toml_grammar` (pure, twelve refusal rows and the defaults). 2: `agents::workflow::tests::a_workflow_is_refused_for_an_agent_lacking_its_tools` (pure) and, end to end, `agent_turns::workflows::workflow_start_validates_its_inputs` (`builder` needs `run`; `_workflows/bare: not a workflow: no workflow.toml`). 3: `agent_turns::workflows::workflow_start_opens_one_session_per_call_id`, `agent_turns::workflows::workflow_start_validates_its_inputs`, `agents::workflow::tests::inputs_are_checked_and_runs_are_named_alike_everywhere`, R201's `workflow::tests::a_workflow_is_named_by_its_folder_or_one_catalogue_row`. 4: `agent_turns::workflows::workflow_start_is_refused_in_a_proxy_dm`. 5: `agent_turns::workflows::workflow_card_runs_once_per_window_in_a_fresh_session`, `agent_turns::workflows::a_card_naming_a_manual_only_workflow_fails_with_a_sentence`, `agent_turns::workflows::an_agent_written_workflow_card_waits_for_a_persons_tick` (the claim across two hosts: `hosts::tests::a_due_card_runs_once_across_two_hosts`). 6: `agent_turns::workflows::a_classic_checkpoint_reaches_the_requesters_proxy`. 7: `agent_turns::workflows::an_unattended_workflow_takes_defaults_and_is_raised_once`. 8: `agent_turns::workflows::a_classic_workflow_resumes_on_the_other_host_from_its_own_files`, `agent_turns::workflows::a_rendered_workflow_is_rendered_again_after_takeover` (one folder, two epochs; DW-539). 9: `agent_turns::workflows::declared_outputs_are_checked_at_close`. 10: `keeper-agentd` `seed_cli::agents_init_seeds_the_steward_workflows_without_overwriting`. R106: `agent_turns::workflows::a_workflow_run_continues_itself_at_most_three_times`.
- **Owed.** The operator-verified items above (OA-94-2; the phone checkpoint on hesperia/kalypso; the resume on electra) are device runs, not claimed here.

**As built (rung `agents-94-workflows`, 2026-10-06, review fixes R94W-01…15).** R202. Tests are `agent_turns::workflows::…` unless named otherwise; each fix's test kills a mutant of it.
- **01 the opening passes the label.** `workflow::Opening::effect` — the brief and inputs at the run's real folder (`sessions::verbs::new_session_path`, or the folder the id has) — is checked against the home drive's readers: `WorkflowTools::run` through `Sinks::verdict` and `CallAudit::blocked` (parks on a declassification with a lift, `AllowedTools` `.lifting`), `for_card` through `Sinks::check`. Tests: `a_narrowed_session_opens_no_run_in_a_broader_drive` (tool and card: refused, no room, no session); `a_run_is_admitted_by_what_its_turns_are_offered` (the audit row names the run's folder).
- **02 the claim fences the opening.** `workflow::Parent` (`may_write`, `record`, `delegation`; `OwnLog` for a card's window, `Mutex<TurnLog>` in a turn); `open` asks it before the room and after; `verbs::create_claimed_session` asks it under the zone's lock as the folder is made. Test: `an_opening_cut_short_goes_on_from_what_its_parent_logged`.
- **03 admission is the run's offer.** `agent::agent_offer` is what arming offers and what `workflow_offer` (a session of kind `workflow`, its own grants, the model's tools) returns; `WorkflowTools.run_offer`, `CardRun.run_offer`; `keeper_core::agents::workflow::tools_check`/`run_drives`. Test: `a_run_is_admitted_by_what_its_turns_are_offered` (`run` allowed but unoffered: refused; `reply` not in `allow`: started; since rung `agents-94-helpers` sits on this one, `helper` allowed and offered: started).
- **04 opening is recoverable.** `delegate opened` naming the room is recorded and synced before the folder; a logged room is the run's; an existing run (`Opened::Existed`) has the parent's missing `opened`/`sent` written from its `agent.toml` and its room watched. Test: `an_opening_cut_short_goes_on_from_what_its_parent_logged`.
- **05 a step begun elsewhere is not begun again.** `SessionContext::started` records workflow steps (`steps_begun`) from the room's anchors; `workflow_arrivals` writes `run: waiting` (`BEGUN_ELSEWHERE`). Tests: `a_first_turn_begun_on_another_host_is_not_begun_again`; across checkouts, `a_classic_workflow_is_taken_over_on_another_checkout`.
- **06 host steps survive the queue.** `RunBody.step` written by `continue_run` and by a resume before the step is queued; `SessionContext.pending_step` and `cut_off` from replay; `workflow_arrivals` returns the pending step. Test: `a_runs_next_step_survives_a_lost_queue`.
- **07 resumes count.** `WorkflowStep::Resume(n)`, `key`/`of_key`, `is_continuation` by key; generation stops past `CONTINUATIONS_PER_RUN`, `workflow_step` refuses out of turn (`NOT_NOW`). Test: `a_workflow_run_continues_itself_at_most_three_times` (the fourth refused).
- **08 no resume past a checkpoint.** `ServedSession::may_go_on` (accepted, not ended, no open ask, not blocked, no park) gates generation and consumption; the round gate stops on a replayed open ask. Tests: `a_cut_turn_waiting_for_an_answer_is_not_resumed`; `a_run_waiting_for_its_answer_takes_no_round_for_a_reply` (a child's reply into a run whose ask was read back after a restart: its receipt logged, no model round).
- **09 outputs are the run's.** `SessionAgent.outputs` stamped by `open` (`declared_outputs`); the reply checks them. Test: `a_runs_outputs_are_those_it_opened_with` (the header broken after opening).
- **10 reply is terminal.** `reply` writes `run: review` for a workflow whatever `set_card_run` does; `TurnView::ended`, asked in `AllowedTools::run_named` ahead of every call — a drive verb's too — refuses later calls (`RUN_ENDED`); the round gate stops (`TurnLog.stopped`); a late child reply is a receipt (`Outcome::Ignored(RUN_MOVED_ON)`, `peer_the_reply` none). Tests: `a_run_that_replied_ends_there` (its card unreadable, a `session_write` and a `drive_read` after the reply refused); `a_card_is_handed_on_once_however_many_runs_ask` (the late reply).
- **11 every generation.** `SessionContext.rendered` is a list; `rerender` restores and checks each. Test: `a_rendered_workflow_is_rendered_again_after_takeover` (workflow and `_skills/bmad-build`).
- **12 the window's own run.** `DelegateBody.window` on `opened`, `Delegation.window`, `ServedSession::names_window`. Test: `workflow_card_runs_once_per_window_in_a_fresh_session` (October 6's late reply leaves October 7's card running; October 7's reviews it).
- **13 nobody to ask.** `tier::Context::nobody_to_ask`, `AllowedTools.nobody_to_ask`. Test: `agent_turns::parks::a_session_nobody_can_be_asked_in_is_raised_once`; `parks::an_action_needing_a_person_in_a_delegated_session_parks_with_a_source` now knows tgorka's proxy. DW-537 closed.
- **14 a card handed on once.** `source = "<session>:<card>"` (`delegate::handed_from`), `workflow::handoff_id`, `delegate::handed_on`; the seeded `dispatch` step names it. Test: `a_card_is_handed_on_once_however_many_runs_ask`. Its scan's cost: DW-542.
- **15 two checkouts, two hosts.** AC8: `a_classic_workflow_is_taken_over_on_another_checkout` with `sync_one_checkout` (agentd's engine in a process per sync). DW-539 closed for format C; format B on two checkouts is DW-541. AC5: `hosts::tests::a_due_workflow_card_opens_one_run_across_two_hosts` (two `HostRuntime`s, the real claim, `cards::begin` and `workflow::for_card` on the holder: one room, one run folder); `ServedSession::run_workflow_card` in that race stays DW-540 (narrowed).

### 94.4 — Helper sessions and review layers

**Intent:** "i would prefer to communicate and cooperate and delegate work for different bots instead of sub-agents - to avoid confustion and make one point of true - also want to make suere its fast". **Rung:** **epic94-workflows**. AD-399, with Q7; FR-803.

**Files:**
- `keeper-core/src/agents/helper.rs` (new, pure):
  - the helper's tool set (read-only: `drive_list`, `drive_read`, `drive_glob`, `drive_grep`, `drive_stat`, `drive_search` once 95.4 lands, `skill_view`);
  - its system message: the session frame without soul or core memory, plus the lens's instruction;
  - the lens lookup: a review layer's id in the run's merged `[[workflow.review_layers]]` (and `oneshot_review_layers`), and its optional `bot` (Q7);
  - the model check: a helper's bot — the lens's or the agent's — passes `check_sink(Model { local })` (92.1; R28 S-04) against the session's label before the call, so a `local_only` label never reaches a provider that is not local (AD-377).
- `keeper-agent/src/helper.rs` (new):
  - `helper(brief, lens?, inputs)`, run as a context-free model call inside the turn, on the agent's bot or the lens's;
  - calls of one round run concurrently and are all awaited before the next model step;
  - tokens are counted against the turn's budget;
  - the helper's own tool calls are logged as child lines of its `tool_call`.
- `keeper-core/src/agents/log.rs` (89.5's reader): replay skips lines whose `parent` is a `helper` call, and 90.5's in-memory `SessionContext` (R29 F2) never appends them either, so the session's messages are what its model saw, whether a turn reads the warm context or a takeover replays the log.
- `docs/agents.md` § *Helpers and review layers*.

**Acceptance:**
1. **A helper owns nothing.**
   - A helper whose fake model calls `drive_write`, `drive_edit`, `session_write`, `delegate`, `reply`, `ask_human`, `card_update`, `helper`, `journal_append`, `memory_propose` or `run` gets "a helper cannot write, send, delegate or start another helper" for each.
   - The temp drive's bytes and the session folder are unchanged, compared file by file.
   - **Test:** `a_helper_cannot_write_send_or_delegate` (keeper-agent).
2. **A helper starts with no context.**
   - The captured request holds the frame, the lens instruction, the brief and the inputs.
   - It holds no turn history, no `SOUL.md` text and no core memory.
   - **Test:** `a_helper_request_is_context_free` (the fake provider records the body).
3. **Review layers run in parallel and are all awaited.** Three `helper` calls in one round, against a fake provider that answers each after 300 ms:
   - finish in under 600 ms;
   - all three results are in the next request.
   - **Test:** `review_layers_run_in_parallel_and_are_all_awaited`.
4. **Helper tokens are the turn's tokens.**
   - With `tokens_per_turn = 2000` and helpers that report 1500 tokens each, the second helper is stopped with "this turn's token budget is spent", and the turn sees that result.
   - **Test:** `helper_tokens_count_against_the_turn`.
5. **In the log, out of the replay.**
   - A helper call is a `tool_call`/`tool_result` pair at T0.
   - Its internal calls are lines whose `parent` is that `tool_call`.
   - Replaying the session yields the parent model's messages without them, and the served session's in-memory `SessionContext` after the turn holds the same messages as that replay.
   - **Test:** `helper_steps_are_in_the_log_and_out_of_the_replay` (keeper-core reader over a fixture log, and keeper-agent comparing the warm context with a cold replay).
6. **What a helper reads joins the label.**
   - A helper that reads a file of a narrower drive narrows the session label with a `label` line.
   - Its result carries the session label.
   - **Test:** `a_helpers_reads_join_the_session_label`.
7. **A review layer's bot, and `local_only`** (Q7; NFR-115's review-layer row).
   - `lens = "blind-hunter"` with `bot = "bot:openai:…#gpt-x"` in the layer runs on that bot.
   - Without `bot`, it runs on the agent's bot.
   - In a session whose label is `local_only` (the agent's home or a drive it read is marked so, 89.4), a layer bot that is not local is refused through `check_sink(Model { local: false })` with "this work stays on a local model", and no request reaches the fake provider; the same layer in a session whose label is not `local_only` runs.
   - **Test:** `review_layer_bot_honours_local_only`.
8. **`bmad-build`'s review step runs as written.**
   - **The run:**
     - from the format-B fixture rendered against the duplicate-free fixture config, `{workflow.review_layers}` renders three sections (94.1);
     - the fake model, following step 4 (`S/bmad-build/step-04-review.md:21-25`), launches `blind-hunter`, `edge-case-hunter` and `verification-gap` as three `helper` calls in one round;
     - their findings come back, and the step's triage proceeds in the same session.
   - **The patch path:** "Re-engage the step-03 implementation subagent" (`:65`) is answered by row 11 of the map: the agent continues its own session.
   - **Test:** `bmad_build_review_layers_run_as_helpers`.

**Shell crate:** does not touch it.

**binds:** FR-803, NFR-115 (the review-layer model call), AD-399, AD-377, AD-391

**As built (rung `agents-94-helpers`, 2026-10-06).** Acceptance 1–8, by R105, R110 and R111.
- **Symbols.** `keeper_core::agents::helper` — `HELPER`, `TOOLS` (the five drive reads and `skill_view`), `REFUSAL`, `TURN_SPENT`, `spent`, `spec`/`parse`/`HelperCall` (`{brief, lens?, skill?, inputs?}`), `Lens`/`lens_of` (by id in `workflow.review_layers`, then `workflow.oneshot_review_layers`; blank instruction not active; `bot` read by `BotRef::parse`), `system_message`, `brief_message`, `result_text` (the answer as data). `keeper_core::agents::prompt::frame_text` (slot 5 without home files). `keeper_core::agents::log::replay::HelperSteps` (a step is a line whose parent is a `helper` `tool_call` other than its own result, or a step), used by `replay` and `SessionContext::push` (a step only counts its tokens). `keeper_core::bots::tools::ToolHost::prepare_round` (default no-op, called once per round with the calls it will run). `AgentTool::Helper` (T0) and its `approval::summary_of` row; capability row 10 says `helper` while offered, row 18 the turn's `tokens_per_turn` (`workflow::frame_lines`'s new argument). `keeper_agent::helper` — `Helpers` (per turn: frame, offer, runs by call id, `running`), `Launch`, `run` (a nested `run_tool_loop_gated` over `Reads`, the session's own host restricted to reads and `skill_view`, every other call `REFUSAL`, a park refused), `Step`/`HelperRun` (written under the helper's `tool_call` by the reporter). `keeper_agent::agent` — `AllowedTools::{prepare_round, run_helpers, launch}`, `impl helper::Parent`, `AgentDeps.rows` (every provider row; a lens `bot` runs on the row of its kind and base URL), `TurnView::turn_spend`/`TurnLog::turn_spend`, the round gate's budget stop, `TurnEnding::Spent` (`turn_tokens`), `SessionContext::frame`, `BmadTools::customization_of`; `keeper-agentd status` counts `helper` as implemented.
- **Corrections (codemap §3).** Row 38: the replay rule lives in `core/agents/log/replay.rs` and the warm context's `SessionContext::push`, one `HelperSteps` for both. Row 39/40: `tokens_per_turn` is built (R111) and acceptance 4 runs over two rounds — the second round's helper is refused at launch once the first helper's 1500 and the second round's own 500 reach 2000; the turn then ends at its next round's gate with the same sentence (the model does not read that result; the log and the room do). A helper whose own round reaches the budget stops at its next round. Row 41: the refusal is `LOCAL_ONLY_SINK`'s sentence. Row 42: `bmad-build`'s step 4 stages a diff only through `run` (96.1); the test names the diff as an input. Acceptance 1's "the session folder unchanged" is every file of the drive but the session's `log/`. Acceptance 6's "its result carries the session label" is the session's label joined with the helper's reads. Acceptance 8 runs over the offered skill `bmad-build` (`skill`), as rung 4 is not below this one (DW-556).
- **Tests.** 1: `agent_turns::helpers::a_helper_cannot_write_send_or_delegate`. 2: `a_helper_request_is_context_free`. 3: `review_layers_run_in_parallel_and_are_all_awaited` (arrival times at the stub: the three launched within 300 ms, the next request within 600 ms of the first). 4: `helper_tokens_count_against_the_turn`. 5: core `agents_log::helper_steps_are_in_the_log_and_out_of_the_replay` and `agent_turns::helpers::helper_steps_are_in_the_log_and_out_of_the_replay` (warm context equals a cold replay over two turns). 6: `a_helpers_reads_join_the_session_label`. 7: `review_layer_bot_honours_local_only` (two stubs, two provider rows) and core `helper::tests::a_lens_is_a_review_layer_of_the_run`. 8: `bmad_build_review_layers_run_as_helpers`. Grammar: `helper::tests::a_helpers_arguments_read_as_given`. Frame bound: `workflow::tests::the_frame_states_the_roots_and_the_map_for_its_offer`. Tier: `tier::tests::every_tool_has_a_tier_row`. ⌘9: `characterisation.rs` unchanged and green.
- **Deferred.** DW-555 (`drive_search` with 95.4), DW-556 (a workflow-session run of acceptance 8 after 94.3), DW-557 (a helper's frame states the label at arming), DW-558 (a failed round's usage), DW-559 (a helper's audit rows do not name its call). New rulings pending numbers: a helper's calls run through the session's own host and never park (R-NEW-1); the turn budget's ending (`Spent`, `turn_tokens`, run `blocked`, round's own completion counted at helper launch) (R-NEW-2); a helper's result label is the session's joined with its reads (R-NEW-3).

**As built (rung `agents-94-helpers`, 2026-10-06, review fixes R94H-01…07).** By R203.
- **01, no helper past a park.** `ToolHost::prepare_round` now only tells the host the round (`Helpers::prepare`); `AllowedTools::run_named` launches a helper at its turn with the helpers right after it (`Helpers::batch`, up to the round's next other call), so a call that parks leaves every later helper unlaunched, a call of the rest re-run once on resume (or answered `NOT_RUN` on denial); a completed helper's run is taken exactly once by its `tool_call` line. Test: `agent_turns::parks::a_helper_runs_once_on_its_side_of_a_park` (approval, denial, approval after a restart; a helper before the park not run again).
- **02, refused steps audited.** `helper::Parent::refused` → `AllowedTools` classifies the step where it would land (its line takes that tier) and `Sinks::helper_refused` writes its one `Deny` row, closed refused, `message_id` the outer helper call id (no tier for a name with no table row). Test: `helpers::a_helper_cannot_write_send_or_delegate` (one row per refused step, the read's one row, nothing else).
- **03, the spend survives a park.** `SessionContext::turn_tokens_at` is `tokens_spent` at the last `user`/`peer` line that opened a turn — a decision's note is now written under the last result (`last_result`) and opens none — and `run_agent_turn` counts from it for arrivals and resumes alike, so a restart reconstructs it from the log. Test: `parks::a_resumed_turn_keeps_the_spend_it_parked_with` (in process and after a restart).
- **04, shared spend.** `Helpers::spent` (reset per launch) takes every round of every helper launched together, once, as it ends; a helper's round 0 is gated on the launch spend, every later round on that plus `Helpers::spent`. Test: `helpers::parallel_helpers_share_the_turns_spend_after_their_launch` (two stubs; 1900 then 200 → no second edge request).
- **05, failed rounds.** `helper::run` writes the loop's last round — `failed` or `cancelled` once any of it arrived — with its usage, counted. Test: `helpers::a_failed_helper_round_keeps_its_line_and_its_tokens`.
- **06, Stop is terminal.** `chat::stream_chat` checks cancellation before each attempt and races the send and the retry back-off (`cancelled_before`); `tools::run_tool_loop_gated` sends no round once cancelled, runs no call of a stream Stop cut, and answers a round's calls after Stop with `tools::STOPPED`; `helper::run` races credential resolution and answers a stopped helper `helper::STOPPED`; a turn stopped between rounds does not repeat the logged round's prose. Test: `helpers::stop_ends_the_helpers_and_the_turn` (mid-stream, before headers, during `Retry-After`; the stub gained `hold_ms` and `status`).
- **07, AC8 reads a real diff.** `helpers::bmad_build_review_layers_run_as_helpers` stages `artifacts/review.diff`, grants `drive_read`, and each lens's helper reads it before answering; asserted on each helper's isolated next request and its logged read. DW-556 (the workflow-session run) stays open; rung 4 is below this rung since the restack below.
- **Deferred.** DW-558 narrowed to the turn's own failed round; DW-559 narrowed to a helper's reads.
- **Restacked onto rung `agents-94-workflows` (8671d08e, R202), 2026-10-06.** Both rungs' behaviour kept: the tier table holds `Helper` (T0) and `WorkflowStart` (T1), agentd's `status` counts `helper` and `workflow_start`, `TurnView` has `ended` and `turn_spend`, and `agent::agent_offer` (the one offer arming and a run's admission read) offers `helper` as `allow` says — so `workflows::a_run_is_admitted_by_what_its_turns_are_offered` now names `run` as the allowed-but-unoffered tool and admits a workflow naming `helper`. Where they meet: a helper in a workflow's run is answered `RUN_ENDED` once the run replied (`AllowedTools::run_named` asks `ended` before the helper branch); the round gate stops a run that replied or waits on an ask before `bound` and `tokens_per_turn` (`TurnEnding::Spent` after `stopped` and `Bounded`); a run's continuation peer line opens a new turn, so `turn_tokens_at` restarts there; and a helper obeys the run's own token budget as its next round would — `TurnView::session_budget` (spent, the round under way included, and the limit of a delegated session or a run) is taken at launch beside the turn's spend, and the helper's gate refuses with `BoundReached::Tokens`' sentence once that spend plus its helpers' rounds since reaches it (R214). Grants and label are the run's by construction (`agent_grants`, the live label). Test: `agent_turns::workflows::a_helper_in_a_run_stops_at_its_budget_and_its_reply`. DW-556 stays open (no lens read from a run's workflow folder yet).
- **Merge review fixes R94HM-01…04 (R215, 2026-10-06).** 01: `ServedSession::after_delegated_turn` takes the turn's ending; `Spent` writes `run: blocked` (`turn_tokens`) to the log and the card, no reply, and `continue_run` takes no step. Test: `workflows::a_run_its_turn_budget_stopped_waits_blocked` (also after reload). 02: `after_delegated_turn` returns at once for a run that ended (`run_ended`), so a run that replied keeps `review` and its one reply. Test: `workflows::a_run_that_replied_keeps_review_past_its_budget` (`[helper, reply]` reaching the budget). 03: `helper::run`'s gate checks the session's bound before `tokens_per_turn`. Test: `workflows::a_helper_says_the_runs_bound_when_both_budgets_are_spent` (at launch and before a second round). 04: `workflows::helpers_in_a_run_stop_mid_way_at_its_budget` (one helper stopped after its own 1900, its usage in the log after reload; two siblings, 1900 and 200, the later one stopped at 2200). Each fix kills a mutant in `mut-94help.py` (M28–M31).


## Operator actions

These are owed outside this repository. Each is named where a story depends on it, and none is assumed done.
- **OA-94-1 — remove the duplicated module keys from the BMAD plugin's template** (the owner decides whether to; keeper works either way). Until it is done, DW-383 stands.
  1. In `/workspace/bmad-plugin/plugins/bmad/runtime/_bmad/config.toml`, delete `planning_artifacts`, `implementation_artifacts` and `project_knowledge` from `[modules.gds]` (lines 32–34), or drop the `gds` module from the plugin's install set.
  2. Publish the plugin.
  3. Run `/bmad:init` in `/workspace/tgdrive`, in neuradrive and in this repository.
  4. Verify in each with `uv run _bmad/scripts/render_skill.py --project-root . --skill ~/.claude/plugins/cache/bmad-method/bmad/<version>/skills/bmad-build`. It must print `read and follow …`, not `HALT: …`. The CLI writes `_bmad/render/`, which this repository ignores (`_bmad/render/.gitignore`).
- **OA-94-2 — put a workflow into a drive.** Only a person writes `_workflows/**` (AD-362's fence).
  1. Run `cp -R ~/.claude/plugins/cache/bmad-method/bmad/6.12.0.0/skills/bmad-create-epics-and-stories /workspace/tgdrive/80-agents/_workflows/`.
  2. Add a `workflow.toml` (`version = 1`, `name = "bmad-create-epics-and-stories"`, `description`, `tools = ["drive_read", "drive_glob", "session_write", "bmad_config", "bmad_memlog"]`, `[trigger] card = true`).
  3. Let keeper's sync commit it.
- **OA-94-3 — test users on the Synapse test homeserver** (shared with 90.4's smoke).
  1. Start delectra from makistack with `task vm:start -- delectra`.
  2. Register the users: `ssh delectra docker exec keeper-test-synapse-1 register_new_matrix_user -c /data/homeserver.yaml -u <user> -p <password> --no-admin http://localhost:8008`, once for each of a test person, `nixi-test` and `tola-grey-test`.
  3. Put the passwords in the test host's secrets, never in a file in this repository.

## What stays out

- **Running BMAD's Python** in any form, on any host. That is refused (AD-397). Other scripts reach `run` (96.1) only where a host has Python.
- **Sub-agents with an identity**: a helper that is re-addressed, lives beyond the turn or writes. Refused (AD-399). The work is a delegation.
- **A workflow inside a person's DM with their proxy.** Refused (AD-380, P1).
- **A `TaskKind` for workflows.** Refused (ruling R9).
- **`bmad-loop`'s orchestration** (tmux sessions, hook events, `result.json`, `/bmad-loop-resolve`). Not run by keeper; cards and delegation are the loop (map row 19).

Deferred, with the ledger entries opened here so a later planner finds them; `_bmad-output/implementation-artifacts/deferred-work.md` carries DW-381…DW-384 in full.
- DW-381 — web search as a built-in tool (placed by the architecture).
- DW-382 — BMAD's other 39 helper scripts have no Rust port.
- DW-383 — format-B skills halt in any drive whose BMAD install carries the duplicated module keys.
- DW-384 — a render generation lives in unsynced scratch, so a run whose BMAD inputs changed cannot resume.

## The failure shape this epic must not repeat

**A workflow that improvises.** BMAD assumes capabilities it never names (G4 §5), and a model that lacks one will make something up. A review that finds any of the following is a blocker:
- a BMAD capability answered with neither a tool nor a sentence;
- a render that guesses a value BMAD would refuse, including the duplicate keys;
- a Python process started by any BMAD tool;
- an `ask_human` that blocks a thread, or that reaches a person anywhere but through their proxy.

**A sub-agent by another name.** A review that finds any of the following is a blocker:
- a helper that writes, sends, delegates or is addressed after it returned;
- a helper call missing from the session log;
- a helper whose steps appear in the parent's replayed context.

## Sprint-status entry

Applied under `development_status:` in `_bmad-output/implementation-artifacts/sprint-status.yaml`, above the epic-93 block: `epic-94` and its four story keys.

## Stack rungs

Rungs by layer, bottom → top, on top of epic 93's last rung (`epic93-card`). Each compiles alone.
1. **`epic94-bmad`** — 94.1 and 94.2.
   - `keeper-ported/src/bmad/{config,render,memlog,help,party}.rs`, `UPSTREAM.md`, and the parity tests and fixtures with their generator;
   - `keeper-core/src/agents/{workflow,ask}.rs`;
   - `keeper-agent/src/{bmad,skills,ask}.rs`;
   - `docs/agents.md` § *Workflows*.
   The PR names the Synapse round trip as `#[ignore]` and pastes its run (OA-94-3).
2. **`epic94-workflows`** — 94.3 and 94.4.
   - The `workflow.toml` grammar and the card runner;
   - `keeper-agent/src/{workflow,helper}.rs`;
   - `keeper-core/src/agents/helper.rs` and the reader's helper-line rule;
   - `agents init`'s two seeded workflows;
   - the workflow fixtures;
   - `docs/agents.md`.
