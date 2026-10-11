# Epic 95 — Memory that improves, knowledge that lands

created: '2026-10-02'
status: planned 2026-10-02; build follows in story order on the agents stack
source: the owner's rounds 1–3 of 2026-10-01/02 (excerpts verbatim below, as `_bmad-output/planning-artifacts/agents-coordinator-decisions-2026-10-02.md` and research §1.1–§1.3 record them), pinned by the coordinator as P2, P12 and P14 and rulings R1, R21 and R26, with the review rulings of 2026-10-02 applied here (R28's S-04, S-05, S-12, S-13, S-31, S-32, S-35; R29's F4, F7, F14, F20, F22, F24). Other inputs:
- `_bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-AGENTS.md`, which is binding. It decides AD-364 and AD-400…AD-404, the *Data formats* for core memory, `journal/` and `proposals/`, and the placeholders DW-385…DW-387 and UX-DR137. This epic adds no decision of its own. Every place it had to settle something the architecture leaves open is under *Open questions for the coordinator*.
- `_bmad-output/planning-artifacts/research-agents-2026-10-02.md`: §2.5, §2.7, §3.8 (OKF on the drives), §9.1–§9.4 (Hermes, OpenClaw, the design), §13 #3, #4, #45, §14. Cited as §n.m.
- Upstream, read on 2026-10-02:
  - Hermes Agent (NousResearch/hermes-agent, MIT) at main@bfc7152687277dd877734e8e33ad0dd6bbbfa07d: `tools/memory_tool_store.py`, `tools/threat_patterns.py`, `tests/tools/test_memory_tool.py`, and `agent/curator.py`'s constants;
  - OpenClaw's documentation at main@07c176c3ce431216edab63b0dba3239183aadad5: `docs/concepts/dreaming.md` and `docs/cli/memory.md`. Its `LICENSE` reads MIT ("Copyright (c) 2026 OpenClaw Foundation"); GitHub's API reported "Other" (digest R1).
- The drive's own OKF tools, `/workspace/tgdrive/.okf/` (`config.yaml`, `bin/okf`, `bin/okf_lib.py`, `bin/okf_index.py`, `bin/okf_links.py`, `OKF-0.2-digest.md`, `registry/types.md`), read and run in process on 2026-10-02 and cited `tgdrive:.okf/…`.
- The phase-7 sessions PRD, `prds/prd-keeper-2026-07-03/phase-7-sessions.md`, for what FR-229…FR-244 actually say, and `docs/sessions.md`.
- The neighbouring lanes' landing points, agreed by message on 2026-10-02:
  - from the 89/90 lane: 89.3's read-side `keeper_core::agents::memory` and `WriteScope::with_agents`; 89.5's `ChunkWriter::append_line` and the `.keeper/agents.db` index; 90.2's `session_write`; 90.6's claims;
  - from the 91–93 lane: 92.2's card fields; 92.5's harvest hook, which runs in the steward's own harvest session and writes no knowledge files; 93.1/93.3's approval record and card.

Line numbers are in the `agents-plan` worktree on 2026-10-02.

binds: FR-804…FR-808; NFR-118 (95.1 and 95.2's halves); AD-364 (its write half), AD-396 (its `hermes`, `openclaw` and `okf` modules), AD-400, AD-401, AD-402, AD-403 and AD-404. All of these were allocated by the architecture, not here. The phase-7 FR-243 and FR-244 are realised in 95.5 (Q1). Deferred items in DW-385…DW-392 (DW-385…DW-387 placed by the architecture) and DW-440; UX decisions UX-DR137 (the architecture's, refining the phase-7 UX-DR90) and UX-DR138.
- **The previous ceilings:** the program's (`_bmad-output/planning-artifacts/agents-program-map-2026-10-02.md`, C1) are epic 88, AD-359, FR-766, NFR-111, UX-DR126, DW-354 and D-30. The architecture allocated AD-360…AD-416, FR-767…FR-822 and NFR-112…NFR-122, and `docs/decisions.md` holds D-31…D-36. FR, NFR, AD and D numbers are the architecture's; DW-440 comes from this lane's amendment range (DW-440…DW-449).
- **No earlier allocation.** On 2026-10-02, a grep found the following, and nothing else:
  - **Where:** `_bmad-output`, `docs`, `src`, `src-tauri/crates`, `AGENTS.md`, `README.md` and `CLAUDE.md`.
  - **What for:** `epic-95`, `epic95`, `DW-E95-`, `UX-DR-E95-` and `95-[1-5]-`.
  - **What it found:**
    - the architecture's DW-385…DW-387 (`ARCHITECTURE-AGENTS.md:1001`, `:1247-1249`) and UX-DR137 (`:1018`);
    - D-34's draft naming DW-385 (`agents-decisions-draft.md:205`);
    - epic 96's sentence placing its ledger above this epic's (`epic-96-…:327`).

  This epic's deferred items therefore start at DW-388, and its UX decisions at UX-DR138.
see-also:
- D-21 (a derived index is disposable), D-31 (an agent lives in a drive), D-34 (labels; drafted);
- AD-65, AD-158, AD-159; AD-378 (claims, reused here as leases); AD-388 (the doorbell); AD-390/AD-391 (labels and sinks);
- epic 94 (the `helper` the nudges run on), epic 93 (the approvals a shared drive's memory waits on), epic 92 (92.5's harvest trigger).

## The owner's ask

Verbatim, rounds 1–3 (2026-10-01/02):

> i like hermes self improvement mehanism and continues memory

> The data from sessions can be used after to update the knowledge in the main drive (or drives)

> Use sessions in the drives as a working place of the agent with all the data he needs, scripts he needs to use etc logs but also the message history and actions taken - so the session can be used after. …

> i own the server infrastructure and tgrive is only for tgorka - make sure the sensitive part goes only to the private bots/drives (nixi needs to be told to use what drive context - but can multiple)

> - memory, skills, soul, etc bot data find a right place in the drives fro this files (tgdrive i neuradrive)
> - rewriting to rusr recommended parts (add separate source module)

## The verdict, ask by ask

The epic is a plan, so the verdict is what the plan does, not what exists.

| # | The ask (verbatim) | Verdict | How it is met | Mechanism |
| --- | --- | --- | --- | --- |
| 1 | "hermes self improvement mehanism" | **planned, with its rails** | The pieces are Hermes' own: caps and `§` entries, staged proposals, nudges after 10 turns or 15 tool rounds, and a weekly curator that never deletes. The agent writes a journal and proposals; core memory changes only by consolidation or a person, and a skill an agent proposes is offered only after a person adopts it. | AD-364, AD-400, AD-402; 95.1, 95.2, 95.3 |
| 2 | "continues memory" | **planned** | A nightly consolidation promotes proposals through OpenClaw's documented gates under a lease; on a private drive only what a person said or wrote stands behind a promotion, else the person is asked. Each run is one commit that names its sources. | AD-401; 95.2 |
| 3 | "update the knowledge in the main drive (or drives)" | **planned** | A steward's harvest writes OKF notes that nobody has reviewed yet. The promote panel lets the person review them and copy them into the drive. | AD-404; 95.5 |
| 4 | "nixi needs to be told to use what drive context - but can multiple" · "sensitive part goes only to the private bots/drives" | **planned** | `drive_search` searches only the drives in the session's scope, through their OKF bundles, never what a bundle excludes. Every result is labelled with its drive's readers. A memory or knowledge write wider than its label is refused. | AD-403, AD-391; 95.4, 95.1, 95.5 |
| 5 | "a right place in the drives fro this files (tgdrive i neuradrive)" | **planned** | `80-agents/<agent>/USER.md`, `MEMORY.md`, `journal/` and `proposals/`, and `80-agents/_skills/`, in each drive (89.2's zone). | AD-361 |
| 6 | "rewriting to rusr recommended parts (add separate source module)" | **planned** | `keeper-ported::hermes` and `keeper-ported::openclaw` (the latter written from documentation, per ruling R21), and `keeper-ported::okf`, ported from the drive's own `.okf/bin` tools. | AD-396; 95.1, 95.2, 95.4 |

## What the triage found

| Need | Verdict | Evidence |
| --- | --- | --- |
| Core memory files, read side | **present after 89.3** | Per the 89/90 lane, 89.3 lands `keeper_core::agents::memory`: the cap constants 1375/2200, `§` split, scalar count, duplicate and invisible-character refusal, a frozen snapshot at open, and no writer. |
| Hermes' memory semantics | **upstream, MIT** | Hermes' `MemoryStore`:<br>• `ENTRY_DELIMITER = "\n§\n"`;<br>• budgets are `len(ENTRY_DELIMITER.join(entries))`, so the delimiters count;<br>• `add`, `replace` and `remove` match a whole entry exactly first, else a unique substring;<br>• distinct matches are an error;<br>• a staged write is pinned to `matched_entry`;<br>• `apply_batch` is all-or-nothing against the final budget and refuses to empty a store;<br>• external drift is refused;<br>• load-time threat hits become `[BLOCKED: …]` in the snapshot only.<br>(`tools/memory_tool_store.py` @bfc71526) |
| An injection scan | **upstream, MIT** | `tools/threat_patterns.py` @bfc71526:<br>• 36 patterns in three cumulative scopes, with memory at `strict`;<br>• 17 invisible code points;<br>• NFKC folding;<br>• a 64 KiB scan cap;<br>• the user-facing sentences.<br>Its `hardcoded_secret` pattern uses a lookahead that the workspace's `regex` 1.12.4 cannot express. |
| OpenClaw's gates | **documented** | "`minScore`, `minRecallCount`, `minUniqueQueries` must all pass". `untrusted` and `system` candidates are removed structurally, and cron, heartbeat and subagent sessions never ingest (`docs/concepts/dreaming.md`). The defaults are 0.75 / 3 / 3, a 14-day recency half-life, a 30-day maximum age and `0 3 * * *` (`docs/cli/memory.md`). The six documented ranking weights are 0.30 relevance, 0.24 frequency, 0.15 query diversity, 0.15 recency, 0.10 consolidation and 0.06 conceptual richness. |
| A commit with chosen trailers | **absent** | keeper-sync stamps a fixed trailer block on every commit (`Keeper-Profile`, `-Device`, `-Origin`, `-Source`, `-Agent`, `-Tag`; `keeper-sync/src/provenance.rs:54-59`, `:107-126`). Values are sanitized against forged lines (`:209-222`). Commits come from the watcher's settle, so no caller can add a trailer. |
| A lease on Matrix | **present after 90.6** | The claim arithmetic (AD-378) on a session room's `state_key ""`. A lease is the same thing under another `state_key` in the control room (AD-401). |
| Drive search for agents | **absent** | DW-212 refused retrieval for ⌘9 bots (`epic-61-…:327-328`); AD-403 takes it for agents. The notes vault's hybrid index exists, read-only capable, in keeper-core (`notes/search_index.rs:171`, `:292`, `:362`, `:559`). It is built only by the shell's notes vault, and `.keeper/` never syncs. |
| The drive's OKF contract | **present, in Python** | `tgdrive:.okf/config.yaml` declares 9 bundles, 16 exclusions, guides, `no_index` and one listed staging zone. `okf_lib.py` parses it with PyYAML or its own fallback, which agree on this file (checked 2026-10-02). `_match` (`okf_lib.py:819-830`), `is_excluded` with guides winning (`:833-836`) and `bundle_for` with the innermost bundle (`:770-778`). The listing grammar is in `okf_index.py:101-174`. |
| The promote panel | **specified, not built** | `docs/sessions.md:1070-1073`; the archive dialog defers to it (`src/components/sessions/session-actions.tsx:61-62`). The table's parser and splice are built: `keeper-core/src/sessions/promote.rs:22-61`, `:127-136` (FR-243, FR-244, AD-108). |
| OKF writes by keeper | **refused for ⌘9 bots** | "keeper reads OKF and never writes it" (`spec-61-11-the-drive-as-tools.md:40`, research §2.7). Here a harvest writes OKF frontmatter for agents (AD-404). |

## The one sentence

**An agent can read its memory but not change it. It cannot learn from what it did, cannot find anything in a large drive without walking it, and what it works out stays inside its session.** The fix has five parts, each fenced:
- **Write a journal and stage proposals** (95.1). Hermes' caps, scan and pinning are enforced by a port of Hermes' own code.
- **Promote proposals once a night** (95.2). Only the always-on host does it, under a lease, through OpenClaw's gates, as commits that name their sources. A shared drive waits for its owner.
- **Retire skills nobody uses** (95.3). Marked stale at 14 days, archived at 30, never deleted, and only the skills the agents made.
- **Search the drives in scope** (95.4). Through their OKF bundles, never what a bundle excludes, every result labelled.
- **Harvest into notes nobody has reviewed** (95.5). A person promotes them from a panel.

## What earlier decisions said, and what this epic amends

| The earlier decision | What it said | What this epic needs | The amendment |
| --- | --- | --- | --- |
| **spec-61-11's Never list** (`:40`) | keeper reads OKF and never writes it | harvested notes carry OKF frontmatter (AD-404) | **Scoped.** ⌘9 bots still never write OKF. An agent's harvest writes it only into its own session's `artifacts/knowledge/`, with `generated.by` stamped by the host. |
| **DW-212** (`epic-61-…:327-328`) | no index over the drive for bots | `drive_search` | **Taken for agents** by AD-403 (its note). ⌘9 bots are unchanged. |
| **AD-404's wording** | a harvest writes into "the session's `artifacts/knowledge/`", meaning the closed session | one owner and one writer per session (AD-365, AD-378); the closed session's claim is released at `close` | **Ruled (R26): the steward's own harvest session** (Q2); the architecture is corrected to match. |
| **AD-404's `generated: {by: "<agent>@<host>"}`** | an agent's signature | keeper's own reader classifies `tola-grey@electra` as `Unknown` (`notes/okf.rs:231-260`) | **Ruled (R26): `agent:<agent>@<host>`** (Q3), the vault's `agent:` shape (`okf.rs:223-224`). |
| **AD-404's citation FR-229/FR-235/FR-236/FR-241** | "the promote panel" | the panel is FR-243 and FR-244 (`phase-7-sessions.md:244-254`) | **Ruled (R26): realises FR-243 and FR-244** (Q1); FR-235/FR-236/FR-241 stay unbuilt (DW-391); AD-404 is corrected. |

## Requirements

Copied from the architecture's *Requirements allocated here*. They are not restated and not renumbered.

| id | statement | epic.story | AD |
| --- | --- | --- | --- |
| FR-804 | An agent keeps a journal and stages proposals for its memory and skills; its core memory changes only through consolidation or a person. | 95.1 | AD-364, AD-400 |
| FR-805 | Every night the always-on host promotes staged proposals through fixed gates — nothing from outside-tainted, scheduled, handed-on or gate work, no change that loses more than a quarter of what was there — as commits that name their source; on a shared drive the drive's owner approves first, a USER.md change by the source session's requester; on a private drive a change is promoted only when a contributing session had owner or peer input, else it waits for the owner. | 95.2 | AD-401 |
| FR-806 | Every week skills unused for 14 days are marked stale and those unused for 30 days are archived; nothing is deleted, and skills a person wrote are left alone. | 95.3 | AD-402 |
| FR-807 | An agent searches the drives in its scope through their OKF bundles and keeper's indexes, never reading what the bundles exclude, with every result labelled and bounded. | 95.4 | AD-403 |
| FR-808 | What a session learned lands as OKF notes of at most 64 KiB marked as not reviewed by a person, and the promote panel lets the person promote them, and any other artifact of the session, into the drive's notes vault. | 95.5 | AD-404 |
| NFR-118 | **Memory stays bounded.** Core memory never exceeds 1375 and 2200 characters; a consolidation that would lose more than 25 % is rejected; a session's memory snapshot never changes while it runs. | 89.3, 95.1, 95.2 | AD-364, AD-401 |

From the phase-7 sessions PRD, realised here and not renumbered (Q1):

| id | statement (`phase-7-sessions.md`) | story |
| --- | --- | --- |
| FR-243 | **promote.** From the workspace listing or the promote panel: pick a `workspace/` file, name (or reuse) its `artifacts/` target, keeper copies under the sync engine's stability gate and writes/updates the `## Promote` table row in the same action. Re-promotion overwrites the target. Table edits by hand are equally valid — the panel renders the table, it does not own it. (`:244-249`) | 95.5 |
| FR-244 | **promote panel truth.** The panel shows each table row with staleness (source newer than target), missing-source (shown quietly), and missing-target (shown loudly). Plus unlisted workspace files, one action away from promotion. (`:251-254`) | 95.5 |

**Held, not restated:**
- NFR-115 (no byte crosses principals). It is allocated to 89.4/90.3/92.6; this epic owns the tests of three of its sinks: *propose* (95.1 #7), *promote* (95.5 #6) and the embeddings model call (95.4 #12).
- D-21: `drive_search`'s indexes are derived and disposable, and 95.4 writes nothing to a drive.

## Built on

This epic uses, and does not rebuild:
- `keeper-ported`'s crate and `UPSTREAM.md` test, and `agentskills` (89.1);
- the zone and `_drive.toml` (89.2);
- 89.3's read-side memory, the frozen snapshot, the skills index and `WriteScope::with_agents`;
- labels and `check_sink` (89.4, 92.6);
- `ChunkWriter`, the log reader and the `.keeper/agents.db` index (89.5);
- `session_write` (90.2);
- claims and placement (90.6);
- the card fields (92.2);
- 92.5's harvest hook and session;
- the approval record, `Parked` and the card on every device (93.1–93.3);
- `helper` (94.4).

## Open questions for the coordinator

Each has the reading this plan builds to, marked as such, so no lane is blocked. None is resolved silently.

- **Q1. FR-229/235/236/241 are not the promote panel.** AD-404, research §9.4 and the program map cite "FR-229/235/236/241" for the promote panel. Those ids come from `docs/sessions.md:1070-1073`, where one parenthetical covers a list of five unbuilt items: the panel, unread marks, per-file history, capture into the session log, and the sticky current session. The PRD says:
  - FR-229 is the sessions query grammar (`phase-7-sessions.md:166-169`);
  - FR-235 is unread marks and "changes since you last looked" (`:201-204`);
  - FR-236 is per-file history and blame (`:206-207`);
  - FR-241 is capture parity (`:234-237`);
  - the panel itself is FR-243 (promote) and FR-244 (the panel's truth: staleness, missing source, missing target, unlisted workspace files), at `:244-254`.

  The architecture also cites `docs/sessions.md:1084-1089`, which has since moved to `:1070-1073`.
  - **Plan's reading:** 95.5 builds the panel as FR-243/FR-244 specify, plus AD-404's promotion of a note out of the session. FR-235, FR-236 and FR-241 stay unbuilt and are named (DW-391). The query grammar (FR-229) already reaches sessions through spaces (`docs/sessions.md:1076-1081`).
  - **Ruled (R26):** the panel is FR-243/FR-244; AD-404 and research §9.4 are corrected in the architecture, and DW-391 is reworded to match (R29 F24).
- **Q2. Where a harvest writes.** AD-404 has the steward's harvest hook write into the closed session's `artifacts/knowledge/`. That session is owned by another agent; its claim is released at `close`; its log has no per-line agent field; and replay assumes every line is the owner's context (AD-365, AD-366, AD-378).
  - **Ruled (R26):** the steward's own harvest session, as agreed with the 91–93 lane:
    - 92.5's hook appends a turn to the steward's own harvest session (`60-sessions/active/<date>-harvest`, kind `scheduled`, owned by the steward, created idempotently by 92.5), naming the closed session's path;
    - 95.5 lets that turn write candidate notes into the harvest session's `artifacts/knowledge/<source-slug>/`;
    - `sources` point back at the closed session.
- **Q3. Signing an agent's note.** AD-404 writes `generated: {by: "<agent>@<host>"}`. Two readers disagree on how to read it:
  - keeper's own actor classifier reads `tola-grey@electra` as `Unknown` (`notes/okf.rs:231-260`), and the vault knows `agent:<id>` (`:223-224`);
  - keeper's `OkfFacts.human_reviewed` is derived from `verified[]` holding a person (`bots/tools.rs:401-414`), while AD-390's label wrapper reads a `human_reviewed:` key.
  - **Ruled (R26):**
    - `generated: {by: "agent:<agent>@<host>", at: …}`;
    - the note carries `human_reviewed: false`;
    - the person's tick sets `human_reviewed: true` *and* appends `verified: [{by: "human:<localpart>", at: …}]`, so both readers agree;
    - no agent can write `verified` (95.5 #1);
    - what a tick proves (R28 S-31): that the change was committed by a reader's keeper, not that the person typed it; `docs/agents.md` says so, beside AD-390's reading of `owner` integrity.
- **Q4. What "recall" means for a proposal.** OpenClaw's recall count and query diversity come from a retrieval store that keeper does not have. Relevance and conceptual richness are measured from retrieval and concept tags.
  - **Plan's reading:**
    - **The candidate.** A candidate is a pending core-memory proposal keyed by `(target, op, folded text)`, where folding collapses whitespace and case-folds.
    - **The signals:**
      - *recall count* is the number of pending proposals with that key (the agent independently proposed it again);
      - *unique queries* is the number of distinct source sessions among them;
      - *consolidation* is the number of distinct UTC days;
      - *recency* is `0.5^(age_days / 14)` from the newest.
    - **The score** is OpenClaw's weighted sum over the four measured signals, renormalised by their weights' total (0.64). Frequency is `min(1, recall/3)`, diversity `min(1, sessions/3)`, consolidation `min(1, days/3)`.
    - **The thresholds** are OpenClaw's defaults, 0.75 / 3 / 3.
    - **What this costs:** a fact proposed in one session is promoted only after it has come up in three sessions on three days.
    - **What a person must stand behind** (R28 S-13, ruled): on a private drive a candidate that passes the gates is applied only if at least one of its proposals was written while its session's integrity was `owner` or `peer` — the person said it, or a person-authored file did. Otherwise the change takes the review path a shared drive takes (a review session, a card, a T2 approval for the drive's `owner`), so an agent cannot promote its own words by repeating them across sessions.
    - **The quicker route:** a person who wants it at once writes it into `MEMORY.md` themselves (people may, AD-364), or approves the review card with one tap. Their write passes Hermes' scan at the next snapshot.
  - Unmeasured signals are DW-392. Whether `owner`-integrity proposals deserve lower thresholds is the owner's call.
- **Q5. Skill proposals and the gates.** Score and recall describe facts, not procedures, so no skill proposal could ever reach three recalls.
  - **Plan's reading:** a skill proposal takes the structural exclusions and the threat scan.
    - On a private drive, it is then applied: a `create`, or a `patch` or `archive` of a skill that still carries `metadata.keeper_proposal` (an agent's skill no person has adopted), is validated by `agentskills` and applied, and a created or patched skill is stamped `metadata.keeper_proposal: "<ulid>"`. A `patch` or `archive` of a skill without the key — a person's, or one a person adopted — is never applied by the night: it takes the review path (a T2 approval for the drive's `owner`), and the approved change is written without the key. `archive` moves to `_skills/.archive/`.
    - On a shared drive, its owner approves first, and that approval is the adoption: the approved skill is written without the key.
    - **Not offered until a person adopts it** (R28 S-12, ruled): 89.3's skills index (`keeper-core/src/agents/skills.rs`) does not offer a skill carrying `metadata.keeper_proposal`; `skills_list` names it as waiting for a person (94.2). A person adopts it by deleting the key, which 95.3 already defines as adoption; from then on it is the person's skill and the curator leaves it alone (DW-440: the adoption is a hand edit). So an agent's skill never becomes another session's instructions without a person.
  - Hermes writes a skill directly unless `skills.write_approval` stages it (research §9.1; the option's default is not established, because the digest's row is cut, §14); keeper stages every agent skill behind a person. **Rejected:** a separate `keeper_adopted` key that keeps an adopted skill under the curator (a second person action for one decision).
- **Q6. Whose approval a `USER.md` entry needs on a shared drive.** AD-401 says "for a `USER.md` entry about a person, for that person", but no proposal field names the person (the proposal grammar is closed).
  - **Ruled (R26):** the approver is the person who requested the source session. Every session whose origin survives the structural exclusions is a foreground or review session of a person's proxy, so that person is the one the entry concerns.
  - `MEMORY.md` changes need the drive's `owner`.
- **Q7. "03:00 in the principal's time zone".** `agentd.toml` has no time-zone key, and its grammar is closed.
  - **Ruled (R26):** the host's local offset, read at evaluation time through `SyncPlatform::utc_offset_minutes`. That is how every keeper schedule fires (`keeper-sync/src/tasks.rs:800-812`).
  - The weekly curator runs at `0 4 * * 0`, Sunday 04:00 local, after that night's consolidation.
- **Q8. Which skills "a workflow names"** (AD-402).
  - **Plan's reading:** a skill is named when an agent's `[tools].skills` lists it by name (not `"*"`), or when its name appears as a whole word in any file under `_workflows/`. Either keeps it active.
  - A false match only spares a skill, never touches one.
- **Q9. The drive's session-workspace exclusion is inert.** `tgdrive:.okf/config.yaml:75` excludes `60-sessions/**/workspace/**`. But `_match` reads any pattern ending `/**` as a literal prefix (`okf_lib.py:820-821`), so the drive's own tools never exclude a real session's workspace.
  - Checked in process on 2026-10-02:
    - `is_excluded("60-sessions/active/2026-10-02-x/workspace/a.md")` is `False`;
    - only the literal `60-sessions/**/workspace/a.md` is `True`.
  - The config's own comment states the literal-prefix rule (`:84-85`).
  - **Plan's reading:** `keeper-ported::okf` reproduces `_match` exactly, and its parity test pins the inert case. `drive_search` keeps session workspaces out by keeper's own rule, that a workspace is never searched by default (`phase-7-sessions.md:39-41`, FR-237).
  - The drive's fix is the owner's (OA-95-3, DW-389).
- **Q10. A gate's proposals** (R29 F4, ruled: 95.2's gate skips them, the curator expires them at 30 days, and the nudge pass proposes no memory in a gate session). **Plan's reading:** a pending proposal whose `origin` is `gate` is skipped by every night's structural gate — no verdict, no score, it stays under `proposals/`; 95.3's weekly sweep gives it `<ulid>.verdict.toml` with `verdict = "expired"` and moves it to `proposals/done/` at the first sweep on or after its 30th day, in the sweep's commit; `expired` is the only verdict a gate proposal ever gets. 95.1's nudge review pass in a `kind = gate` session is offered neither `memory_propose` nor `skill_propose` (AD-400 as corrected), so it writes nothing; a gate-origin proposal from any other path takes the skip-and-expire path above. AD-416's "expire unread after 30 days" is this.
- **Q11. The clobber check** (R28 S-35, ruled: compare git blob ids, not bytes read through a possibly stale checkout). **Plan's reading:** the night pulls the drive first (`Engine::pull_now`, 92.4) and plans over the blobs of `USER.md`/`MEMORY.md` at the fetched head; before writing, a file whose blob id at the head, or whose working-tree content hashed the same way, differs from the plan's is skipped for the night. An edit pushed later still meets the sync engine's conflict rule (the remote wins the path; AD-43), so a person's words are never lost.

## Stories

Every story names its rung in the stack (*Stack rungs*, below).
- **Only 95.5 touches `src-tauri/crates/keeper/**`** (the epic map): its promote-panel commands. Those await CI's macOS job and `check:rust:macos`. Everything else is `keeper-ported`, `keeper-core`, `keeper-sync` and `keeper-agent`, proved on this Linux host.
- **Every new pure behaviour test is mutation-proved:** mutate, run, restore, and read the diff.
- **Parity fixtures are generated once by the upstream code and committed.** The generating command is recorded in each module's `UPSTREAM.md`, and no test runs Python.
- **Names are suggestions the lanes agree on.** Function and file names below are the plan's; the behaviour may not change.

### 95.1 — Memory files and the journal/proposal tools (keeper-ported::hermes)

**Intent:** "i like hermes self improvement mehanism and continues memory"; "memory, skills, soul, etc bot data find a right place in the drives". **Rung:** **epic95-memory**. AD-364 (the write half), AD-400, AD-396 (`hermes` lands here, with its first writer), AD-391's memory sink, AD-362's fence; FR-804; NFR-118.

**Files:**
- `src-tauri/crates/keeper-ported/src/hermes/` (new):
  - `memory.rs`:
    - `ENTRY_DELIMITER = "\n§\n"`;
    - `parse_entries`: split on the full delimiter, strip, drop the empty, strip a BOM;
    - `dedupe`: order-preserving, first wins;
    - `char_count`: Unicode scalars of the joined entries;
    - `add`, `replace` and `remove`, with `find_unique_match` (exact whole entry first, else a unique substring; distinct matches are an error);
    - pinned `matched_entry` with Hermes' stale sentence;
    - `apply_batch` and its dry run `resolve_batch`: all-or-nothing, checked against the final budget, refusing to empty a non-empty store;
    - `detect_drift`: the round-trip check, with no `.bak` file;
    - `sanitize_for_snapshot`: the `[BLOCKED: …]` placeholder.
  - `threats.rs`:
    - the pattern table with its three cumulative scopes, `INVISIBLE_CHARS`, NFKC folding (the locked `unicode-normalization` 0.1.25), `MAX_SCAN_CHARS`, and `first_threat_message`;
    - `hardcoded_secret`'s negative lookahead becomes a post-check on the match, because `regex` has no lookaround.
  - `UPSTREAM.md`:
    - repository, commit bfc71526, licence MIT, the files read;
    - what changed and why:
      - no file lock, because one writer holds the file by construction (95.2's lease);
      - no `.bak` snapshot, because git keeps history;
      - the post-check above;
      - budgets in Unicode scalars, which is what Python's `len` counts on `str`;
      - the `[BLOCKED: …]` placeholder's last clause names keeper's way to remove an entry (`memory_propose` with `op: remove`, or a person's edit) in place of Hermes' `memory(action=remove)`, which keeper does not offer;
    - the generator for `tests/fixtures/hermes/`.
- `keeper-core/src/agents/memory.rs` (89.3's):
  - its read-side primitives move behind `keeper-ported::hermes`, and 89.3's tests stay as the regression;
  - the snapshot replaces a threat-matching entry with Hermes' placeholder, and the file is left as it is.
- `keeper-core/src/agents/proposal.rs` (new, pure):
  - the proposal frontmatter grammar from *Data formats*, with render and parse;
  - `origin` from the session (`main` → `foreground`; a nudge's helper → `review`; `scheduled` and a workflow started by a card → `scheduled`; `delegated` and a workflow started by `workflow_start` → `delegated`; `gate` → `gate`);
  - `match` resolved to the exact current entry when proposing.
- `keeper-agent/src/memory/` (new):
  - `journal_append`, `memory_propose` and `skill_propose`;
  - `JournalWriter` on 89.5's `ChunkWriter::append_line` primitive: `O_APPEND`, one `write` per entry, `fsync` at turn end, the torn tail truncated when the host reopens its own file;
  - proposals created with create-new, never overwritten;
  - the nudge counters and the review pass on 94.4's `helper`;
  - `review_prompts.rs`: Hermes' `_MEMORY_REVIEW_PROMPT` and `_SKILL_REVIEW_PROMPT`, borrowed with an MIT attribution header (research §9.4: "borrow with attribution").
- `docs/agents.md` § *Memory*.

**Acceptance:**
1. **Hermes' own memory tests pass in Rust.**
   - **What is ported:** every `MemoryStore` case of `tests/tools/test_memory_tool.py` @bfc71526, as `hermes_upstream_*` tests with every assertion kept. That covers add, duplicate add, the cap error with `current_entries`, replace and remove by exact and substring match, ambiguity, the pinned stale write, `apply_batch`'s all-or-nothing behaviour and its refusal to empty a store, drift, a BOM, and the security scan.
   - **What is mapped:** cases about file locks, `.bak` files or the tool dispatcher's JSON become tests of the pure function they reach, named in `UPSTREAM.md`.
   - **Risk:** Hermes' sentences are what a model reads and acts on, so they are kept verbatim and diffed.
2. **The caps count what Hermes counts.**
   - **The count:** a body is capped by the Unicode scalars of its entries joined with `\n§\n`. Frontmatter is not counted (*Data formats*).
   - **At the cap:** a `USER.md` of exactly 1375 such scalars, with `ą` and an emoji at the boundary, is accepted.
   - **One over:** 1376 is refused, and the error lists the current entries.
   - **`MEMORY.md`:** the same at 2200.
   - **Test:** `caps_count_the_joined_entries` (pure).
   - **If 89.3 disagrees:** if 89.3's boundary fixture counted without delimiters, this rung corrects that one test and says so in the PR. Hermes' count is the one the architecture cites (digest R6 A1).
3. **A duplicate stages nothing.**
   - `memory_propose(target: memory, op: add, text)` whose text is already an entry answers "Entry already exists (no duplicate added)." and writes no file.
   - A text equal to another *pending* proposal is written, because that is a recall (Q4).
   - **Test:** `a_duplicate_add_writes_no_proposal` (keeper-agent, a real home in a temp drive).
4. **A proposal that cannot fit is refused while the agent can still fix it.**
   - **The check:** `memory_propose` checks the proposed change against the current file with this session's own pending proposals for that target applied (`resolve_batch`).
   - **Over the cap:** it returns Hermes' cap sentence with the would-be entries, and writes nothing.
   - **The fix in the same turn:** a `replace` that shortens two entries into one, followed by the `add`, both succeed.
   - **Test:** `an_over_cap_proposal_is_refused_until_the_agent_consolidates`.
5. **Everything staged is scanned first** (research §9.3).
   - **What is scanned:** every `memory_propose` text and `skill_propose` body, at `strict` scope.
   - **On a hit:** Hermes' sentence, and nothing is written. Examples:
     - `Blocked: content contains invisible unicode character U+200B (possible injection).`;
     - `Blocked: content matches threat pattern 'exfil_curl'. …`.
   - **Test coverage:**
     - the table holds each pattern id at least once;
     - a full-width `ｃａｔ ~/.env` is caught through NFKC;
     - `hardcoded_secret` blocks `api_key = "AbCdEf…"` and passes `ENV_PASSWORD = "MYPLUGIN_APP_PASSWORD"`;
     - an `AGENTS.md`-style "you must run the tests" passes.
   - **Tests:** `proposals_are_scanned_before_they_are_written`, `the_secret_pattern_post_check_matches_the_lookahead`.
6. **A poisoned entry is blocked in the snapshot, not deleted.**
   - **Setup:** a person hand-writes a `MEMORY.md` entry holding "ignore all previous instructions".
   - **The snapshot:** the session's frozen snapshot shows `[BLOCKED: MEMORY.md entry contained threat pattern(s): prompt_injection. …]` in its place.
   - **The file:** byte-identical.
   - **The `open` line:** `memory_sha256` is the snapshot's.
   - **Test:** `a_poisoned_entry_is_blocked_in_the_snapshot_not_deleted` (keeper-core over a fixture home).
7. **A private finding never becomes shared memory** (AD-400, AD-391; NFR-115's *propose* sink).
   - **Blocked:** a neuradrive agent's session whose label has narrowed to `{tgorka}` proposes into its own memory, whose audience is neuradrive's readers `{tgorka, marta}`. The call is blocked with the reason, and no file appears under the home. The test opens the session with that label, because the mount rule (AD-377) makes the cross-drive read hard to stage. This is the defence in depth AD-400 asks for.
   - **Allowed:** the same proposal in a session whose label is `{tgorka, marta}` is written, its `label` recorded.
   - **Test:** `a_private_finding_never_becomes_shared_memory` (keeper-agent, a temp drive with a two-reader `_drive.toml`).
8. **The journal has one writer per file and survives a torn tail.**
   - **The shape:** `journal_append(text)` writes `journal/YYYY-MM-DD.<host>.md`, with frontmatter `type: journal`, `agent`, `date`, `host` when created. Each entry is `## HH:MM · <session-slug>`, a blank line, then the text. Dates and times are UTC, as the log's are.
   - **One writer:** a second host never writes this host's file.
   - **The torn tail:** the test truncates the file mid-entry and reopens it. The torn tail is cut back to the last complete entry, and the next append is whole.
   - **Tests:** `journal_append_has_one_writer_per_file`, `journal_survives_a_torn_tail` (real files).
9. **Proposals are immutable, and a replace is pinned.**
   - **Immutable:** `proposals/<ulid>.md` carries every key in *Data formats*' table. A second write to the same id is refused, and `drive_write`/`drive_edit` on it is refused by 89.3's fence.
   - **Pinned:** a `replace`'s `match` is the full current entry its `match` text selected, resolved by `find_unique_match` when proposing. So the consolidator applies to exactly what was reviewed (Hermes' `matched_entry`).
   - **Tests:** `proposal_frontmatter_round_trips`, `a_replace_proposal_pins_the_exact_entry`.
10. **The nudges fire on Hermes' counts, and the review pass can only propose** (P12).
    - **When:** after 10 person turns (`user` lines) or 15 tool iterations, where `skill_propose` resets the second counter. The counters live in the session.
    - **The pass:** the host runs one `helper` (94.4) with Hermes' review prompt. That helper is offered `memory_propose` and `skill_propose` and nothing else that writes — except in a `kind = gate` session, where it is offered neither (R29 F4, AD-400, Q10).
    - **Off:** `[memory].nudge_user_turns = 0` turns that nudge off.
    - **Tests:** `nudge_counters` (pure), `the_review_pass_can_only_propose` (a fake model that calls `drive_write` is refused), `a_gate_sessions_review_pass_proposes_nothing` (in a `kind = gate` session the pass's offered tools hold neither `memory_propose` nor `skill_propose`, and a fake model that calls either is refused and writes no file).
11. **A session never sees its own proposals as memory.** A proposal written in turn 3 leaves turn 4's system message and the `open` line's `memory_sha256` unchanged (89.3's snapshot, held).
    - **Test:** `a_session_never_sees_its_own_proposals_in_memory`.
12. **Pure and licensed.**
    - `check:ported-pure` and 89.1's `UPSTREAM.md` test pass (`licence: MIT`);
    - `cargo deny check` passes;
    - the crate gains no crate outside the lockfile (`regex` 1.12.4 and `unicode-normalization` 0.1.25 are already locked).

**Shell crate:** does not touch it.

**binds:** FR-804, NFR-118, NFR-115 (the propose sink), AD-364, AD-400, AD-396, AD-391, AD-362

**As built (rung `agents-95-memory`, 2026-10-06).** Rulings R123–R127 and R132; R126 makes the review pass its own model run, not 94.4's `helper`.
- **Ported:** `keeper-ported::hermes::{memory, threats, review}` with `UPSTREAM.md` (MIT, Nous Research; every upstream case named — 37 of 57 ported, 1 mapped, 19 not ported with the reason) and the parity fixture `tests/fixtures/hermes/scan.jsonl`, generated once by `generate.py` from upstream's own scan (136 rows; it found that `regex`'s `\s` lacks U+001C–U+001F, now matched as Python matches).
- **keeper-core:** `agents::memory::{MemoryTarget, MemoryFile}` (the adapter, R124) and the per-entry `[BLOCKED: …]` in `snapshot`; `agents::proposal` (`Proposal::render`/`parse`, `origin_of`, R127); `agents::nudge::Nudges` (the counters, from the log); `agents::skills` lists `metadata.keeper_proposal` skills as `waiting` and skips dotted folders; `log::MemoryOp::Review` and `log::ReviewLines` (a review pass's lines, skipped by `replay`); `log::writer::open_own_file`; `tier` rows `JournalAppend`, `MemoryPropose`, `SkillPropose` at T1 with summaries.
- **keeper-agent:** `memory::{journal, MemoryTools}` served through `run_named` — label against the home's readers (`Sink::MemoryWrite`), claim, scan, Hermes' check, create-new write, a `memory` line under the `tool_call`; `ReviewPass` and `ServedSession::review` after a completed main/conversation turn; `skills_list` names waiting skills. **agentd:** `report::tools` counts the three as implemented.
- **Acceptance → proof:** 1 `hermes::memory::tests::hermes_upstream_*`, `hermes::threats::tests::hermes_upstream_*` (incl. `hermes_upstream_scan_matches_the_fixture`); 2 `memory::tests::caps_count_the_joined_entries` (the 89.3 boundary fixture already counted delimiters, §0.1: nothing corrected); 3 `agent_turns::a_duplicate_add_writes_no_proposal`; 4 `agent_turns::an_over_cap_proposal_is_refused_until_the_agent_consolidates`; 5 `threats::tests::proposals_are_scanned_before_they_are_written`, `the_secret_pattern_post_check_matches_the_lookahead`, `agent_turns::a_proposal_with_a_threat_is_refused_and_writes_nothing`; 6 `memory::tests::a_poisoned_entry_is_blocked_in_the_snapshot_not_deleted` and the session half in 11; 7 `agent_turns::a_private_finding_never_becomes_shared_memory` (also the `MemoryWrite` producer of `every_sink_refuses_a_wider_audience`); 8 `memory::journal::tests::journal_append_has_one_writer_per_file`, `journal_survives_a_tear_at_every_byte`, `journal_appends_take_turns`; 9 `proposal::tests::proposal_frontmatter_round_trips`, `a_proposal_outside_the_grammar_is_refused`, `memory::tests::a_proposal_file_is_never_written_over`, `agent_turns::a_replace_proposal_pins_the_exact_entry`, the fence half `files_write::an_agents_fence_refuses_every_home_file_and_leaves_the_rest_alone` (existing); 10 `nudge::tests::nudge_counters`, `agent_turns::the_review_pass_can_only_propose`, `a_gate_sessions_review_pass_proposes_nothing`; 11 `agent_turns::a_session_never_sees_its_own_proposals_in_memory` (and the existing `the_memory_snapshot_does_not_move_during_a_session`); 12 `check:ported-pure`, `tests/upstream.rs`, `cargo deny check`, no new crate in the lock.
- **Corrections to this text:** the journal is not 89.5's `append_line` (Q3); the prompts are adapted, not verbatim, and live in `keeper-ported::hermes::review` (Q4); `[BLOCKED]`'s last clause names `memory_propose`; the review pass parks nothing — what needs a person is refused in it. Deferred: DW-560 (skill support files), DW-561 (journal headings in an entry's text; done by R95M-09), DW-562 (a lost claim mid-review, turn level).
- **As built (review fixes R95M-01…16, R204, 2026-10-06).** 01 `hermes::memory::sanitize_for_snapshot` passes only an exact placeholder (`is_placeholder`; `UPSTREAM.md` records the deviation) — `only_an_exact_placeholder_passes_unscanned`; 02 `MemoryTools::skill` refuses a body over `MAX_SCAN_CHARS` — `memory::tests::a_skill_longer_than_the_scan_is_refused`; 03 `prompt::memory_slot` opens with `FILE_CONTENT_IS_DATA`, `failure_text` puts matches and entries after it — `prompt::tests::memory_entries_are_data_in_their_slot` (split attack), `memory::tests::an_ambiguous_match_hands_back_entries_as_data`; 04 `skills::adoption` (`Adoption::Unreadable`, `UNREADABLE_METADATA`) — `skills::tests::unreadable_adoption_metadata_is_not_offered`; 05 the prospective file read by `MemoryFile::read` before staging — `a_proposal_keeper_could_not_read_back_is_refused`; 06 `MemoryTools::viewed`, recorded from a whole `skill_view` of SKILL.md — `a_skill_patch_is_pinned_to_what_was_read`, `agent_turns::a_replace_proposal_pins_the_exact_entry` (a patch before the view refused); 07 `may_write` carried into `write_new` and `journal::append` — `a_proposal_is_published_whole_under_the_claim`, `journal::tests::a_lost_claim_cuts_and_writes_nothing`; 08 `FileExt::lock_exclusive` over the journal transaction — `journal_appends_take_turns`; 09 `<!-- n -->` lengths and `journal::whole` — `journal_survives_a_tear_at_every_byte`, `a_journal_keeper_did_not_write_is_left_alone`; 10 `journal::append_with` syncs home and folder on creation — `a_new_journal_file_is_published_durably`; 11 `MemoryTools::next_id` — `dependent_proposals_replay_in_the_order_they_were_made`; 12 the duplicate check against the pending-adjusted store — `an_entry_a_pending_change_took_out_can_be_added_back`; 13 `ReviewPass::ceiling` from `spent_before` + `tokens_per_turn`, checked before the pass and in the round gate — `agent_turns::the_review_pass_is_charged_to_the_turns_budget`; 14 `MemoryTools::failures` with `Store::set_consolidation_failures` — `the_failure_cap_is_one_budget_for_both_files`; 15 `write_new`: part file, sync, claim, `hard_link`, folder sync — `a_proposal_is_published_whole_under_the_claim`, `a_proposal_file_is_never_written_over`; 16 the review prompt and refusal sentences no longer pinned in `the_review_pass_can_only_propose`. Every fix's mutants killed, and the build's re-run (`mut95mem3.py`: 48 mutants, 48 killed; `mut-95mem-summary3-part1.log`, `mut-95mem-summary3.log`); 11's test now puts the pending id in a later millisecond with the largest random part (a same-millisecond id let `r11-no-floor-from-home` survive), which also fixed `next_id` past that part, where it took a fresh, earlier id. Deferred: DW-575 (part files a crash leaves), DW-576 (unoffered skills cannot be patched), DW-577 (a proposal taken back after a failed folder sync), DW-578 (a journal append refused after its entry landed).
- **As built (restack onto 83a2d3a1, `agents-94-helpers`, 2026-10-07).** The memory tools sit in `agent_offer` (so `workflow_offer` checks a run's start against them) and in `run_named` after its ended check: inside a workflow's run they obey the run's gate — refused with `RUN_ENDED` after the run's `reply`, its round/turn/session budgets, its label and grants (R202) — `workflows::a_memory_tool_after_the_runs_reply_has_no_effect`. The review pass stays its own run (R126), offered no `helper`; it spends the turn's one `tokens_per_turn` account (R111, R226): it starts from `SessionContext::turn_tokens_at` like the answer and its helpers, `ServedSession::review` skips it once `helper::spent`, and a round that crosses it ends `TurnEnding::Spent`, closed by `helper::TURN_SPENT`/`turn_tokens` (the separate `ceiling` is gone) — `the_review_pass_and_the_helpers_share_the_turns_budget`, `the_review_pass_is_charged_to_the_turns_budget`. A helper is never offered a memory tool, and one it calls anyway gets `helper::REFUSAL` — `a_helper_is_never_offered_the_memory_tools`. Tier table: `Helper` T0, `WorkflowStart` and the three memory tools T1; replay skips helper steps (R203) and a review pass's lines; approval summaries from both rungs. Merge review R95MM-01…03 (R227): a helper's `skill_view` runs as `helper::Parent::view_skill` and never moves the session's `MemoryTools::viewed` pin — `a_helpers_skill_view_never_moves_the_sessions_pin`; a review pass's last, prose completion that reaches the budget ends `Spent` too, its assistant line (usage once) then the `turn_tokens` error — `the_review_pass_and_the_helpers_share_the_turns_budget`; a workflow needing the memory tools is admitted and offered them, or refused before any room — `workflows::a_run_needing_the_memory_tools_is_admitted_and_offered_them`.

### 95.2 — The nightly consolidation (keeper-ported::openclaw gates, lease, git trailers)

**Intent:** "i like hermes self improvement mehanism and continues memory". **Rung:** **epic95-memory**. AD-401; AD-396 (`openclaw` lands here, written from documentation, ruling R21); AD-378 (the lease); AD-388 (the doorbell); AD-393/AD-395 (the approvals a shared drive, and a private drive's unbacked change, wait on); AD-402 (skills not offered until adopted); FR-805; NFR-118; UX-DR138.

**Files:**
- `src-tauri/crates/keeper-ported/src/openclaw/` (new):
  - `gates.rs`:
    - the structural exclusions;
    - the thresholds (`min_score` 0.75, `min_recall_count` 3, `min_unique_queries` 3, `recency_half_life_days` 14, `max_age_days` 30);
    - the documented weights and Q4's score;
    - the prior-entry loss check at AD-401's 25 %.
  - `UPSTREAM.md`: "written from OpenClaw's documentation, `docs/concepts/dreaming.md` and `docs/cli/memory.md` at main@07c176c3, read 2026-10-02; no source read as code (ruling R21); `LICENSE` reads MIT while GitHub's API reported 'Other'".
- `keeper-core/src/agents/consolidate.rs` (new, pure):
  - candidates from proposals (Q4);
  - each agent's night as a plan: which proposals promote, stay pending, are rejected or expire — and the `gate`-origin proposals, which it skips: no verdict, left pending for 95.3's sweep (Q10);
  - the resulting `USER.md`/`MEMORY.md`, through `hermes::apply_batch`;
  - the loss, cap and drift checks;
  - the evidence rule on a private drive (Q4, R28 S-13): a candidate is applied only if one of its proposals carries `owner` or `peer` integrity in its `label`, else it goes to the review path;
  - the shared-drive routing (Q6);
  - skill proposals (Q5);
  - the commit subject and trailers.
- `keeper-core/src/agents/skills.rs` (89.3's `index`, `:77`): a skill carrying `metadata.keeper_proposal` is listed as not adopted and not offered (Q5, R28 S-12).
- `keeper-sync`:
  - `Engine::commit_paths(profile_id, CommitRequest { paths, subject, trailers })`. It stages exactly those paths and commits with keeper's provenance block followed by trailers from a closed key set (`Memory-Origin`, `Source-Session`), each value through `provenance::sanitize` (`provenance.rs:209-222`). It then queues the push.
  - It holds the profile's commit lane, so no watcher pass commits those paths in between.
- `keeper-agent/src/consolidate.rs` (new):
  - **The run:** the nightly job on the host's tick, which keeper-sync's pure schedule parser evaluates at `0 3 * * *` on the host's offset (Q7). It runs only where `always_on` holds.
  - **The lease:** `state_key = "consolidate:<drive>"` in the principal's control room, using 90.6's claim arithmetic with the state key as a parameter — the read-back and the re-read after the settle (R28 S-05) included.
  - **The read:** `Engine::pull_now` (92.4) first, then the plan over the blobs of each home's files at the fetched head (Q11).
  - **The writes:**
    - files written with a guard: a file whose git blob id at the head, or whose working-tree content hashed the same way, differs from the plan's is skipped for the night (Q11, R28 S-35);
    - proposals moved to `proposals/done/` beside `<ulid>.verdict.toml`;
    - one `commit_paths` per agent;
    - the doorbell `{reason: "memory"}`.
  - **The review path** — on a shared drive, and on a private drive for a change no `owner`/`peer` proposal stands behind (S-13), a person's skill an agent wants to patch or archive (Q5), or a batch over the cap:
    - a review session `60-sessions/active/<date>-memory-review-<agent>`: kind `scheduled`, owned by that agent, its caller-supplied id derived from drive, agent and date;
    - a card in it;
    - a T2 approval record (93.1) whose `preview` is the before/after artifact (UX-DR138), decided by the drive's `owner` (by the source session's requesting person for a shared drive's `USER.md`, Q6);
    - applying the change once the approval is consumed (93.2).
- `docs/agents.md` § *Memory › Consolidation*.

**Acceptance:**
1. **Nothing tainted, scheduled, handed on or from a gate is ever scored** (AD-401; OpenClaw's structural gate).
   - **Rejected unscored:** a proposal whose `label.integrity` is `untrusted`, or whose `origin` is `scheduled` or `delegated`, gets `verdict = "rejected"` with a reason naming the gate.
   - **Skipped, never scored** (R29 F4): a proposal whose `origin` is `gate` gets no verdict and stays under `proposals/`, night after night; 95.3's sweep expires it (Q10).
   - **Test:** `excluded_origins_never_reach_scoring` (pure, one row per origin and integrity; the `gate` row asserts that no verdict file is written).
2. **The gates are OpenClaw's, measured as Q4 says.**
   - **Promotes:** a candidate proposed in three sessions on three days, the newest today, one of them at `peer` integrity (S-13).
   - **Stays pending:** the same candidate from two sessions.
   - **Expires:** a candidate whose newest proposal is 31 days old (`verdict = "expired"`).
   - **The boundary:** a candidate at exactly the thresholds promotes.
   - **Test:** `gates_and_score_table` (pure, golden table recorded in `openclaw/UPSTREAM.md`).
3. **Promotion is one batch against the final budget.**
   - **The batch:** an agent's promoted changes to one file are one `apply_batch`.
   - **A stale pin:** a `replace`/`remove` whose pinned `match` is gone is `rejected` with Hermes' stale sentence.
   - **Over the cap:** a batch that would exceed the cap is not applied, and goes to the review path with the entries listed.
   - **Test:** `promotion_is_one_batch_against_the_final_budget`.
4. **A rewrite that loses more than a quarter is withheld** (NFR-118).
   - **The fixture:** four entries.
   - **Applied:** a night whose promoted removals drop one (25 %).
   - **Withheld:** one dropping two (50 %). Nothing is written, and a review card names the loss.
   - **Test:** `a_rewrite_losing_more_than_a_quarter_is_withheld`.
5. **A person's edit is never clobbered.**
   - **A hand edit that will not round-trip:** a `MEMORY.md` a person edited into such a shape (Hermes' drift rule) makes the night write nothing for that agent. The review card quotes Hermes' drift sentence.
   - **An edit during the run:** a file changed between the plan's read and its write is skipped for the night (the guard).
   - **An edit this checkout has not seen** (R28 S-35): a `MEMORY.md` edit pushed from another device before the night is in the plan through the pull; one pushed after the plan's read changes the blob id at the fetched head or the working tree, and the night writes nothing for that file; the guard never compares bytes read without that pull.
   - **Tests:** `a_hand_edited_memory_file_is_never_clobbered`, `a_concurrent_edit_skips_the_night`, `the_guard_compares_blob_ids_at_the_fetched_head` (real files, a real repository and a bare remote).
6. **One commit per agent, carrying its sources.**
   - **The commit:** `memory: <agent> — <n> promoted, <m> rejected`. It carries:
     - keeper's provenance block;
     - `Memory-Origin: consolidator@<host>`;
     - one `Source-Session: <drive-relative session path>` per contributing session.
   - **Same commit:** the moved proposals and their verdict files.
   - **The watcher race:** a watcher tick forced between the write and the commit produces no commit without the trailers.
   - **Test:** `consolidation_commits_carry_the_trailers` (keeper-sync and keeper-agent, a real repository through `gix`, the message parsed from `git log -1 --format=%B`).
   - **Risk:** a real commit and a real watcher.
7. **A proposal cannot forge a trailer.**
   - **The attack:** a proposal whose `session` is `"60-sessions/active/x\nMemory-Origin: human"`.
   - **Rejected as malformed:** its `session` must name an existing session folder under the zone (`browse::resolve`).
   - **Sanitized anyway:** the value is sanitized before it reaches a trailer.
   - **The proof:** no second `Memory-Origin` line appears.
   - **Test:** `a_proposal_cannot_forge_a_trailer`.
8. **One consolidator per drive** (AD-378's arithmetic, as a lease).
   - **Two hosts:** at 03:00, one writes `consolidate:<drive>` with the next epoch, reads it back from the server, re-reads it after 90.6's settle (S-05) and runs. The other logs "consolidation of tgdrive held by electra" and writes nothing.
   - **Losing the lease:** a holder that cannot renew stops before its next write.
   - **Tests:** `consolidation_lease_arithmetic` (pure) and `one_consolidator_per_drive_on_synapse` (against `keeper-test-synapse`, OA-94-3's users, `#[ignore]` by default, named in the PR).
   - **Risk:** a real Matrix server and a server-side read-back.
9. **Only always-on hosts consolidate, once a night.**
   - **The desktop host** never runs it.
   - **A missed night:** an agentd started after a missed 03:00 runs once, never once per missed night (AD-138's rule, `tasks.rs:362-366`).
   - **Tests:** `the_desktop_never_consolidates`, `a_missed_night_runs_once`.
10. **On a shared drive nothing changes without its person** (AD-401, Q6).
    - **No apply:** on neuradrive (`readers` = two), no file in a home changes during the night.
    - **What is written instead:** the review session, a card and a T2 approval record. Approvers: the drive's `owner` for `MEMORY.md`, and the source session's requesting person for `USER.md`.
    - **Approved:** the consolidator applies the exact previewed change and commits.
    - **Denied:** the proposals are `rejected` with `decided_by` set to the person.
    - **Test:** `on_a_shared_drive_nothing_changes_without_its_person` (keeper-agent with epic 93's approval path beneath).
11. **Skills follow Q5.**
    - **Private drive, a valid skill:** a `create` from a foreground session lands as `_skills/<name>/SKILL.md`, carrying `metadata.keeper_proposal: "<ulid>"`, validated by `agentskills`, and is not offered: `skills_list` names it as waiting for a person (S-12). After a person deletes the key, the next session is offered it.
    - **A person's skill:** an agent's `patch` of a skill without `metadata.keeper_proposal` is not applied; it waits on the review path, and the approved change is written without the key.
    - **Refused by `agentskills`:** `rejected` with the validator's reason.
    - **Shared drive:** the same `create` waits for the owner's approval, which is the adoption: the approved skill is written without `metadata.keeper_proposal` and is offered.
    - **Test:** `skill_proposals_land_validated_stamped_and_unoffered`.
12. **The other host sees it within seconds.** After pushing, the lease holder sends `dev.keeper.agent.doorbell {drive, commit, reason: "memory"}` (AD-388) into its control room and every other control room that lists the drive (92.4, R29 F19).
    - **Test:** `consolidation_rings_the_doorbell`.
13. **An agent's own repetition needs a person** (keeper-agent with epic 93's approval path beneath, mutation-proved; R28 S-13): `an_agents_own_repetition_needs_a_person` — on tgdrive, a candidate that passes the score from three `agent`-integrity sessions on three days is not applied; the review session, its card and a T2 approval record for tgorka are written instead; tgorka's approval applies exactly the previewed change. The same candidate with one proposal from a session at `owner` integrity is applied by the night.

**Operator-verified:**
- [ ] On electra, `agentd-tgorka` with `always_on = true`. The next morning, `git -C $XDG_DATA_HOME/keeper-agentd/drives/tgdrive log -1 --grep 'Memory-Origin: consolidator@electra'` shows the night's commit and its `Source-Session` lines.
- [ ] For neuradrive: ruling R17's operator action (`agentd-neuraffica` pushes from its own checkout; owed under epic 90) is done. Otherwise the review card and the approved change never leave electra.

**Shell crate:** does not touch it.

**binds:** FR-805, NFR-118, AD-401, AD-396, AD-378, AD-388, AD-393, UX-DR138

**As built (rung `agents-95-consolidate`, 2026-10-06).** Rulings R128–R131; R-NEW-1 asked (the read is `sync_once`, DW-565).
- **Ported:** `keeper-ported::openclaw::gates` (structural gate, thresholds, weights, Q4's renormalised score, the 25 % loss check) with `UPSTREAM.md` (written from documentation; golden table).
- **keeper-core:** `agents::consolidate` (`plan`, `structural`, `night_due`, `lease_key`, `review_session_id`/`_name`, `VerdictFile`, `ApplyArgs`, `review_record`, `preview`, `review_card`); `MemoryFile::render`; `Proposal::hermes_op`; `tier::AgentTool::{MemoryApply, SkillApply}` at T2.
- **keeper-sync:** `Engine::commit_paths` + `CommitRequest`/`CommitPaths`/`blob_id`; `provenance::{MemoryTrailer, authored_message}`; `git::commit::{Authored, stage_and_commit_authored}`; `Engine::commit_with`.
- **keeper-agent:** `consolidate::{night_window, take_night, plan_home, add_review, run_home, decided, settle_decided, Consolidator}`; `HostRuntime::night`; `CopyPort::review_room`; `ServedSession::adopt_host_action` (the worker adopts a consolidator record, asks the record's readers, skips the empty checkpoint); agentd ticks the consolidator. The doorbell needed no change: the classifier already rings `memory` for the agents zone.
- **Acceptance → proof:** 1 `consolidate::tests::excluded_origins_never_reach_scoring`; 2 `openclaw::gates::tests::gates_and_score_table`, `consolidate::tests::the_gates_measure_proposals_as_q4_says`; 3 `promotion_is_one_batch_against_the_final_budget`; 4 `a_rewrite_losing_more_than_a_quarter_is_withheld`; 5 `consolidation::a_hand_edited_memory_file_is_never_clobbered`, `consolidation::a_concurrent_edit_skips_the_night`, keeper-sync `commit_paths::{a_concurrent_edit_skips_the_night, the_guard_compares_blob_ids_at_the_fetched_head}`; 6 keeper-sync `commit_paths::consolidation_commits_carry_the_trailers`, `consolidation::consolidation_commits_carry_the_trailers`; 7 `consolidate::tests::a_proposal_cannot_forge_a_trailer`, `provenance::tests::a_proposal_cannot_forge_a_trailer`; 8 `hosts::tests::consolidation_lease_arithmetic` (live test owed, DW-568); 9 `hosts::tests::the_desktop_never_consolidates`, `consolidation_lease_arithmetic` (missed nights); 10 `consolidation::on_a_shared_drive_nothing_changes_without_its_person`; 11 `consolidation::skill_proposals_land_validated_stamped_and_unoffered`; 12 `consolidation::consolidation_commits_carry_the_trailers` (`doorbell::rings` over the night's range); 13 `consolidation::an_agents_own_repetition_needs_a_person`. Acceptance 10/13's decision is written as the worker writes it; the worker leg's own test is DW-567.
- **Corrections to this text:** the rung is `agents-95-consolidate`, not `epic95-memory`; the review path's record is a host action, not a parked call (R128); the doorbell is the push's (codemap §3 row 13). Owed: DW-565…DW-569.


**As built (rung `agents-95-consolidate`, 2026-10-06, review fixes R95C-01…22).** Rulings R205 (the read is `sync_once`; declarations re-read, DW-565 recorded) and R206 (every finding accepted, R-NEW-2 rejected).
- **01 consume-once:** `consolidate::{Standing, waiting, decided}` apply only a record the worker logged `consumed`; refused/expired end it; the decision-only run pulls first. Proof: `consolidation::a_decision_is_carried_out_only_once_consumed`, `an_agents_own_repetition_needs_a_person`; worker: `agent_turns::parks::a_consolidator_record_is_consumed_once_through_the_worker` (DW-567).
- **02 approvers:** `ApplyArgs::{approvers, approvers_now}`, `ServedSession::host_action_approvers` (adopt, announce, decision seat). Proof: `consolidate::tests::approvers_and_targets_are_checked_against_the_drive`, `consolidation::a_decision_by_someone_it_does_not_name_changes_nothing`, the worker test.
- **03 labels:** `ApplyArgs::label`, `Review::label`, `consolidate::sink_refusal` before review and before apply. Proof: `a_proposal_reaches_only_whom_its_label_allows`, `consolidation::a_narrow_proposal_is_never_published_on_a_shared_drive`.
- **04 fence:** `consolidate::holding`, `Fence`, `Engine::commit_paths(…, fence)`, `CommitPaths::Fenced`. Proof: `commit_paths::the_fence_stops_every_effect`, `consolidation::a_night_without_its_lease_writes_nothing`, `hosts::a_holder_that_cannot_renew_stops_before_its_next_effect`.
- **05 completion:** `Remembered::{Done, Again}`, `owed`. Proof: `hosts::a_night_is_remembered_done_only_when_it_was` (run_round's wiring by inspection).
- **06 race:** `Acquired::Won::from_released`, `take_night` re-judges what it took over. Proof: `hosts::a_night_completed_during_the_acquisition_is_done`.
- **07/08 serving:** `HostRuntime::night` requires every agent served (DW-704), `NightHome::copy`. Proof: `hosts::a_host_consolidates_only_drives_it_serves_whole`.
- **09 rerun:** `review_room_of`, guarded `agent.toml`, previews named by record id. Proof: `consolidation::a_night_run_again_keeps_its_review_session`.
- **10–13 commit_paths:** durable intent + `put_back`/`recover_commit_paths`, per-effect guard recheck, `browse::Contained`, `git::commit::{commit_authored, index_follow_head}`. Proof: `commit_paths::{a_request_cut_off_between_its_effects_is_put_back, an_edit_during_the_request_is_neither_overwritten_nor_committed_as_it, a_folder_swapped_for_a_link_mid_request_redirects_nothing, an_authored_commit_holds_only_its_own_paths}`.
- **14:** `Authority::at_head`. Proof: `consolidation::the_night_reads_its_declarations_at_the_head`.
- **15:** `zone::session_facts`, gate skip first. Proof: `consolidation::a_proposal_of_an_archived_session_is_still_its_sessions`, `a_gate_proposal_is_skipped_whatever_its_session`.
- **16–20:** `waits` (any review-origin destructive), `Night::reviewing` + one review per file, one change per skill + engine overlap refusal, threat scan, over-cap review. Proof: `a_review_passs_replace_never_rides_an_automatic_promotion`, `one_review_per_file_at_a_time`, `one_change_per_skill_a_night`, `commit_paths::overlapping_paths_are_refused`, `a_skill_carrying_a_threat_is_rejected_at_night`, `promotion_is_one_batch_against_the_final_budget`.
- **21:** `tier::classify` fixes host actions at T2. Proof: `tier::tests::host_actions_are_fixed_t2_in_every_context`, `the_record_is_classified_centrally`.
- **22:** `approvals::{read_stored_record, read_stored_decision, stored_ids}` shared; `ApplyArgs::belongs_to`. Proof: `consolidation::the_consolidator_reads_only_protected_records`.
- **Live:** `keeper-agentd/tests/live_consolidate.rs::one_consolidator_per_drive_on_synapse` passed on delectra (DW-568 closed). New DWs: DW-700…DW-709.

**As built (rung `agents-95-consolidate`, 2026-10-06, re-review fixes R95CR-01…12, R207).** Ruling R207 (R95CR-01…12 accepted, every partial completed; replaces R206's undo journal; one maintenance claim per drive from curate's R95U-03).
- **R207.1 commit_paths, roll-forward only (R95CR-02/03/04/05/09, R95C-10/11/13):** `keeper-sync/src/engine/authored.rs` — `Engine::{commit_paths, commit_paths_held, settle_commit_paths, roll_forward, materialize}`, `Intent` v2 (request id, parent, tree, commit, per-path before/after/disk), `Cut::{Recorded, Published, Displaced}`; `git::commit::{build_authored, publish, index_follow, head_files_under, Built, Followed}`; `git::history::{reaches, commit_with_line}`; `lfs::stage::{attributes_text, prepare_authored, AuthoredStaging}`; settled at `Engine::open`, `tick_profile`, `sync_once` and `commit`. Proof: `commit_paths::{a_request_cut_off_before_its_publication_changed_nothing, an_unpublished_request_never_writes_over_a_persons_own_commit, a_request_cut_off_after_its_publication_is_finished_never_undone, an_unfinished_commit_holds_the_folder_until_it_is_finished, a_branch_moved_before_the_publication_is_never_overwritten, routed_writes_carry_their_rule_and_nothing_else, an_authored_commit_holds_only_its_own_paths, a_concurrent_edit_skips_the_night}`.
- **R95CR-06 fence and renewal:** `commit_paths` runs its git work in `spawn_blocking` with an owned `CommitFence`, asked once the lane is held and right before the compare-and-swap; rooms ask it first. Proof: `commit_paths::{the_fence_is_asked_right_before_the_publication, the_commit_leaves_its_callers_task_running}`, `hosts::a_holder_that_cannot_renew_stops_before_its_next_effect`, `consolidation::a_night_without_its_lease_writes_nothing`.
- **R95CR-07 the replacement boundary:** `browse::Contained::{create, displace, displaced}`, `Displaced::{restore, discard}` (link/`RENAME_NOREPLACE`, never a replacing rename). Proof: `commit_paths::a_save_at_the_replacement_is_never_overwritten`.
- **R95CR-08 the root held:** `browse::Root` (opened once, identity checked against `landing`'s root and before the index write); non-Unix refuses (DW-725). Proof: `commit_paths::a_root_or_folder_swapped_for_a_link_redirects_nothing`.
- **R95CR-01/10, R95C-01/17 once, through history:** `MemoryTrailer::ApprovalRecord`, `consolidate::{carried, Waiting::{carried, holds_target, holds_proposals}}`, `decided` skips a carried record, every terminal record frees its file. Proof: `consolidation::{an_approval_is_applied_once_and_frees_its_file, a_denied_review_frees_its_file_once_rejected}`; two-host takeover DW-721.
- **R95CR-11, R95C-03/14 authority at each effect:** `consolidate::authority_holds` before a room (declarations at `HEAD` and on the disk, every review's label against the readers); the engine re-reads every guard on the disk before the publication. Proof: `consolidation::a_declaration_changed_since_the_plan_makes_no_room`, `commit_paths::a_declaration_changed_before_the_publication_holds_it`.
- **R95CR-12, R95C-18 one review per skill across the drive:** `plan_home(home, drive, now)` reserves other homes' open skill reviews. Proof: `consolidation::one_review_per_skill_across_the_drive`.
- **R207.4 one maintenance claim (R95U-03), R95C-05/06:** `agents::claim::{maintenance_key, completion_key}`, `consolidate::JOB`, `maintain::{maintain, holding, Maintained}` (completion read before and after acquisition, renewal beside the work, live fence, window recorded only for settled work once the release was accepted), `consolidate::{run_round, night_of}` on it. Proof: `hosts::{consolidation_lease_arithmetic, one_maintenance_job_per_drive_whichever_starts_first, a_night_completed_during_the_acquisition_is_done, a_night_is_remembered_done_only_when_it_was}` (the last through `run_round`'s branches).
- **DW-720:** `engine::tests::pending_lists_what_is_still_coming_in_not_only_what_is_going_out` registers no filter.
- **Deleted tests (pinned the removed undo journal):** `commit_paths::{a_request_cut_off_between_its_effects_is_put_back, an_edit_during_the_request_is_neither_overwritten_nor_committed_as_it, a_folder_swapped_for_a_link_mid_request_redirects_nothing, the_fence_stops_every_effect}`; `hosts::a_night_is_remembered_done_only_when_it_was` rewritten through `run_round`.
- **Mutation (DW-569 closed, the rung-wide pass):** `/tmp/agents-salvage/mut-95cons-3.py`, 88 mutants over rounds 1–3 and R207 — 85 killed in `mut-95cons-3.log`; its three survivors closed by `commit_paths::an_unpublished_request_never_writes_over_a_persons_own_commit` (an unpublished record rolled forward; reachability by parentage) and the unreachable-renewal case of `hosts::a_holder_that_cannot_renew_stops_before_its_next_effect` (a failed renewal keeps the claim), all three killed in `mut-95cons-4.log`.
- **Gates:** fmt and clippy (`keeper-ported`, `keeper-core`, `keeper-sync`, `keeper-agent`, `keeper-agentd`, `-D warnings`) clean; full tests green but for the inotify-EMFILE watcher tests of keeper-sync (37) and `agent_turns::a_served_sessions_second_turn_opens_no_file_under_log`. **Live:** `live_consolidate::one_consolidator_per_drive_on_synapse` passed on delectra (the night and the curator at once: one `HeldBy`, one `Ran` recorded).

**As built (rung `agents-95-consolidate`, 2026-10-07, round 4: R95C3-01…15, R217).** Ruling R217 (every finding accepted, fixed as the review proposes). Proofs are `commit_paths::` (keeper-sync), `hosts::` (keeper-agent lib) and `consolidation::` (keeper-agent integration) tests.
- **R95C3-01 fence at the CAS:** objects and bytes are prepared (`Cut::Prepared`), the guards re-read, then `git::commit::publish(repo, base, built, last)` asks the fence after `ensure_head_unlocked`, right before the ref edit (`Publication::{Published, Moved, Refused}`). Proof: `a_lease_lost_before_the_branch_moves_moves_nothing` (replaces `the_fence_is_asked_right_before_the_publication`).
- **R95C3-02 renewal independent:** `maintain::holding` runs `Renewal` as its own task with owned ports (`CopyPort::keyed_claims → Arc<dyn ClaimPort>`), stopped between renewals and awaited; `consolidate::{pull, off_the_task}` move the pull (a blocking thread driving `sync_once`), the authority check, the review writes and the rejection read off the polling task. Proofs: `hosts::{the_renewal_runs_while_the_work_blocks_its_task, the_nights_pull_never_holds_up_the_renewal}` (the real recovery stuck on a FIFO record).
- **R95C3-03 one snapshot:** `git::commit::Base` (branch + its commit + tree) read once; guards, moved entries, attributes and the tree come from it; `build_authored` and `publish` use it. Proof: `a_commit_made_after_the_checks_is_never_built_on` (`Cut::Checked`).
- **R95C3-04 a later commit is never written over:** `roll_forward` reads `HEAD` on every path, the request's own included. Proof: `a_persons_commit_right_after_the_publication_is_never_written_over` (uninterrupted and after a kill, then a watcher pass).
- **R95C3-05/06 reachability, unknown holds:** `git::history::{reaches, lines_in_history}` walk the commit graph with no date bound and error on unreadable objects; `settle_commit_paths` keeps the record on an error; `consolidate::Application::{Carried, Waiting, Unknown}`, one walk per `waiting`; `Unknown` executes nothing and keeps the target and proposals. Proofs: `a_published_commit_is_known_by_its_ancestry_not_its_date` (backdated child, backdated merge, unreadable commit), `consolidation::{an_applied_approval_is_found_whatever_the_dates_say, an_unreadable_history_applies_nothing_and_frees_nothing}`.
- **R95C3-07 complete only when settled:** `Engine::unsettled_commit`, `consolidate::settled` (no record left, no decision outstanding) ends `night_of`. Proofs: `consolidation::{a_night_whose_commit_did_not_finish_is_not_complete, a_refused_rejection_leaves_the_night_unfinished}`, `hosts::a_night_is_remembered_done_only_when_it_was` (now with a real home and a successful pull), `hosts::a_night_whose_commit_did_not_finish_is_tried_again` (`run_round` over staged proposals whose published commit cannot be finished: tried again and unrecorded, then done and recorded once the next pull finished it; DW-825).
- **R95C3-08 monotone completion:** after the accepted release, `maintain::record_held` takes the claim again and `record` writes only a later window. Proof: `hosts::a_completion_never_goes_back_to_an_earlier_window` (DW-728 narrowed, DW-821).
- **R95C3-09 the repository held:** `browse::Root::holds_git_dir`, `Engine::{bound, bound_at}` before the repository opens, before the record, at the CAS and before every roll-forward. Proofs: `a_folder_swapped_before_its_repository_opens_publishes_nothing` (`Cut::Held`), `a_root_or_folder_swapped_for_a_link_redirects_nothing` (now: nothing read or written through the link; finished once the folder is back). DW-822.
- **R95C3-10/11 the index:** `git::commit::index_follow` holds `index.lock` from read to write (`Cut::Indexed`) and decides every path on the sorted read before any entry moves. Proofs: `staging_while_the_index_follows_is_never_lost`, `every_rewritten_file_follows_in_the_index`. DW-823.
- **R95C3-12 generated attributes:** `Engine::apart_from_generated` refuses a write or move at or around `.gitattributes` when routing generates it. Proof: `the_attributes_a_routed_write_needs_are_no_moved_path`.
- **R95C3-13 executable mode:** `materialize` → `Contained::create(.., executable, ..)`. Proof: `executable_files_stay_executable_on_the_disk` (move, rewrite, recovery, watcher). DW-824.
- **R95C3-14 staging:** `Intent::staging` = `.keeper.<request>-<n>.tmp` (excluded), `Contained::{create, clear}` with `Staging::{Created, Synced, Linked}` cuts; roll-forward clears every path's staging. Proof: `a_kill_inside_a_files_staging_leaves_nothing_to_commit`.
- **R95C3-15 uploads:** `Intent` v3 records `uploads`; `Engine::owe_uploads` queues them only after the CAS and again on settling. Proof: `a_refused_routed_write_owes_no_upload`.
- **Mutation:** `mut-95cons-5.log` 78 killed, 3 survived; the three re-pointed in `mut-95cons-6.log` / `mut-95cons-6b.log`, all killed: the guard at the base (`the_guard_compares_blob_ids_at_the_fetched_head` now commits a guard-only file the disk does not show — for a written path the pre-publication disk-against-base check refuses as well), `night_of` asking `settled` (`a_night_whose_commit_did_not_finish_is_tried_again`), the accepted release (`a_holder_that_cannot_renew_stops_before_its_next_effect` now has the drive taken over and handed back during the work, so the second acquisition alone would record).
- **Signature changes rung 3 adapts to:** `maintain::maintain(lease_port: &Arc<dyn ClaimPort>, done_port: &dyn ClaimPort, me: &Claimant, clock: &Arc<ServerClock>, rtt: &Arc<Rtt>, window, work)`; `maintain::holding(port: &Arc<dyn ClaimPort>, me, lease: &Arc<Lease>, clock: &Arc<ServerClock>, rtt: &Arc<Rtt>, work)`; `CopyPort::keyed_claims(&self, room, key) -> Arc<dyn ClaimPort>`. `claim::{maintenance_key, completion_key}` and `Engine::commit_paths` are unchanged.
- **Restack onto 1caff43e (`agents-95-memory` with R226/R227, on `agents-94-helpers` and `agents-94-workflows`), 2026-10-07:** one conflict (`tier::tests`, both tests kept); `AgentTool::ALL` is 31 rows (the helper, workflow and memory rows and `memory_apply`/`skill_apply`); the review session's `SessionAgent` carries `checkpoints: None` and no `outputs` — never a workflow's run. Interaction decisions: the consolidator stays a host job — never a helper, never inside a workflow's run, no model call, so R226's turn budget and R227's `viewed` pin do not meet it (a patch's `match` is still the SHA-256 the night compares); a workflow's run's proposals carry the origin R127 gives them and are rejected at the scheduled or delegated gate (`consolidate::tests::a_workflow_runs_proposals_are_never_scored`); the night reads only `proposals/<ulid>.md`, never the dotted part file R204's publication can leave (`consolidation::a_proposals_part_file_is_never_a_proposal`, DW-575 narrowed); a proposal taken back after the night read it is never applied — the night's guard and the engine's two disk re-reads of a moved path, right before the publication (`consolidation::a_proposal_taken_back_is_never_applied`, DW-577 narrowed). Mutation: `mut-95cons-7.log` (the three of round 6 and two interaction mutants killed; the night's guard alone survives, the engine re-reading a moved path itself) and `mut-95cons-7b.log` (all three re-reads removed: killed).

**As built (rung `agents-95-consolidate`, 2026-10-08, round 5: R95C4-01…09, R232, R233).** Ruling R232 (the restack's interaction decisions above) and R233 (01, 03, 04, 06, 07, 08, 09 fixed; 02 and 05 accepted residuals). Proofs are `commit_paths::` (keeper-sync), `hosts::` (keeper-agent lib) and `consolidation::` (keeper-agent integration) tests.
- **R95C4-01 the `.git` held:** `browse::Root` opens its `.git` with the root (`Root::open`) and `holds_git_dir` compares the one it holds with the one there and the one named at every later check. Proof: `a_git_folder_replaced_inside_the_folder_publishes_nothing` (only `.git` swapped).
- **R95C4-02 roll-forward's one `HEAD` read — accepted residual:** DW-900; `docs/sync.md` and `docs/agents.md` say "read once, right before the files follow". The existing proof up to that read stays (`a_persons_commit_right_after_the_publication_is_never_written_over`).
- **R95C4-03 completion under expiry and takeover:** `maintain::record(.., may_write)` asks the claim taken again between the completion read and the write; `record_held` counts nothing recorded after a lapsed claim or a refused release. Proofs: `hosts::a_completion_read_before_the_claim_lapsed_is_never_written` (A reads, its claim lapses, B records a later window, A resumes: nothing written, unrecorded) and `hosts::a_completion_whose_claim_was_taken_before_its_release_is_not_recorded` (A's write lands, B takes the drive before A hands the claim back: unrecorded). DW-903.
- **R95C4-04 a review's whole source set:** `add_review` guards every proposal the preview was made from; `consolidate::Waiting::{whole, withdrawn}` — a record that lost one holds nothing, is never outstanding and its rest is previewed again; `decided` applies only while every source is pending. Proof: `consolidation::a_review_that_lost_a_proposal_is_never_applied` (review → one taken back and synced → approve/consume → nothing applied, previewed again).
- **R95C4-05 withdrawal after the last re-read — accepted residual:** DW-901; "re-read right before publication", never atomic. The early-removal regression `consolidation::a_proposal_taken_back_is_never_applied` stays.
- **R95C4-06 fence before the room:** `run_home` asks the fence again after `authority_holds`, right before `room()`. Proof: `consolidation::a_lease_lost_while_the_room_is_checked_makes_no_room`. DW-902.
- **R95C4-07 a person's executable bit:** `materialize` takes the displaced file's own mode (`browse::Displaced::executable`): a rewrite's new bytes take it, a removed file whose bit changed stays. Proof: `a_persons_mode_change_is_never_undone`.
- **R95C4-08 a superseded change's displaced file:** `roll_forward` → `put_back` restores it, drops it when a person's file took the path and it holds the committed bytes, else keeps it and the record. Proof: `a_commit_taken_back_after_a_kill_gets_its_old_file_back` (displacement cut → personal reversal → recovery → watcher).
- **R95C4-09 tier table:** `docs/agents.md` lists `journal_append`, `memory_propose`, `skill_propose` at T1 and `memory_apply`/`skill_apply` fixed T2, no raise, never a model call.
- **Mutation:** `mut-95cons-8.log` all killed but the refused second release, which `a_completion_read_before_the_claim_lapsed_is_never_written` cannot see (its write is fenced off first, so `recorded` is false whatever the release says); `mut-95cons-9.log` kills it with `a_completion_whose_claim_was_taken_before_its_release_is_not_recorded`.
- **Signature changes rung 3 adapts to:** `consolidate::Waiting` gains `pub whole: bool` and `Waiting::withdrawn()`; `maintain::record` (private) takes `may_write`. `maintain::{maintain, holding}`, `CopyPort::keyed_claims`, `claim::{maintenance_key, completion_key}` and `Engine::commit_paths` are unchanged.

**As built (rung `agents-95-consolidate`, 2026-10-08, round 6: R95C5-01…04, R246).** Ruling R246 (all accepted). Proofs are `commit_paths::` (keeper-sync lib) tests.
- **R95C5-01 refusal cleanup bound to the held `.git`:** `authored::Intent::remove(repo, root)` asks `Engine::bound` before it unlinks, so `abandon` (every refusal after the record), the removal after a finished roll-forward and the settling refuse against a `.git` put in place of the one held and leave its record untouched; the request's own record stays in the `.git` it began with. Proof: `a_refusal_never_drops_another_gits_record` (`.git` swapped at `Cut::Prepared` with another request's record inside, a guarded file saved at the same time: refused, that record byte-identical, the held record kept, both `HEAD`s unchanged).
- **R95C5-02 recovery reconciled with `HEAD`:** `roll_forward` passes `put_back(root, intent, n, head)` the blob `HEAD` holds now. The transaction's own new file — still linked under `Intent::staging(n)` (`browse::{Contained, Displaced}::is_linked_as`) and holding its bytes — is moved to `Intent::taken(n)` and dropped only once still that file; anything else moved off goes back. The old file moved aside goes back when `HEAD` holds the before-image again, is dropped when `HEAD` holds no file (a committed deletion stays one), and is dropped beside a person's own file; the commit's bytes with no staging identity, or a third version at `HEAD`, keep the old file beside the path and the intent (DW-970). Proofs: `a_settling_follows_what_the_person_committed_since` (`Staged(0, Linked)` + reversal commit; the same + an in-place edit; `Displaced(0)` + deletion commit — each through `settle_commit_paths` and a later `commit_local` watcher pass, checking HEAD, disk, index, record and `.keeper*` leftovers) and `a_settling_that_cannot_tell_whose_a_file_is_keeps_the_record` (staging name gone + reversal; `Displaced(0)` + a third version: settling errors, record and moved-aside file stay).
- **R95C5-03 exact boundaries (docs only):** `docs/agents.md` (completion: read → claim asked → send, a delayed send can still replace a later completion, `recorded` is local, DW-903; proposals and review: each re-read and its publication are not one step, DW-901; recovery and refusal cleanup as above), `docs/sync.md` (`commit_paths` refusal cleanup and recovery), `maintain::{record, record_held}` rustdoc, the `add_review` comment, DW-901 and DW-903. No wording test.
- **R95C5-04 wording assertions deleted:** `keeper-core/src/agents/consolidate/tests.rs` no longer asserts `S-13`, `R127`, the `## Proposed, over the cap` heading or the over-cap explanation prose; every behavioural assertion (writes, settlements, approvers, proposals, the bounded after-image, the over-cap entries in the preview, the notes' listed entries) stays.
- **Mutation:** `mut-95cons-10.log` — all 7 killed by a test panic: the cleanup's `Engine::bound` removed, the publication guard kept (refusal returned `Ok`); the commit's own file kept on the path (the settling errs, `expect("settled")`); a committed deletion resurrected (`never brought back`); an in-place edit taken for the commit's file (`left: "a"`); the commit's bytes taken for a person's and a third version at `HEAD` ignored (the settling completes, `is_err()`); and R95C4-08's `put_back` call removed (re-anchored, killed by both the old and the new regression).
- **Signature changes rung 3 adapts to:** none public. Private to `keeper-sync/src/engine/authored.rs`: `Intent::remove(repo)` → `Intent::remove(repo, root)`; `Engine::put_back(root, path, before, aside)` → `Engine::put_back(root, intent, n, head)`; new `Intent::taken(n)`. `keeper-sync/src/browse.rs` gains `pub(crate)` `Contained::is_linked_as` and `Displaced::is_linked_as`. `Engine::{commit_paths, settle_commit_paths, unsettled_commit}`, `CommitPaths`, `CommitRequest` and every keeper-agent signature are unchanged.

**As built (rung `agents-95-consolidate`, 2026-10-08, round 7: R95C6-01…03, R250).** Ruling R250 (all accepted; recovery completes only when every path's occupant, mode, `HEAD` entry and before-image are fully attributed, anything unattributed keeps the intent and the evidence — DW-970's hold, widened). Proofs are `commit_paths::` (keeper-sync lib) tests; each composition goes through the settling, a later watcher pass (`commit_local` twice a settle window apart: the pass that would commit is refused, `HEAD` and the record unchanged — test helper `held`), then the person's resolution.
- **`put_back(root, intent, n, head)` reordered:** `head` is `HEAD`'s entry (blob id and mode). A file a stopped settling left at `Intent::taken(n)` goes back first; anything else moved aside goes back; with nothing of the commit's bytes at the path, the old file is dropped for a committed deletion, dropped beside a person's file where `HEAD` holds its bytes, put back where `HEAD` holds its entry exactly, and otherwise kept with the record. With the commit's bytes at the path nothing moves unless the file is still linked under `Intent::staging(n)` and `HEAD` holds the replaced entry or no file; the bit the commit gave it is `materialize`'s (the person's own on the old file, else the commit's), and the file is dropped only once moved off and still linked, those bytes, that bit.
- **R95C6-01 a committed deletion over an unattributed file:** the commit's bytes without the staging link hold whatever `HEAD` says, before the old file is touched. Proof: `a_deletion_committed_over_a_file_nothing_attributes_holds_the_folder` (stopped at `Staging::Linked`, staging name removed, `update-index --force-remove` committed: held, the file and the old one kept, `HEAD` without `MEMORY.md`; the person removes the file: settled, nothing left, a watcher pass commits nothing, the deletion stays).
- **R95C6-02 a third version over the commit's own file:** `HEAD` neither the replaced entry nor absent holds before anything is moved, with or without an old file moved aside. Proof: `a_third_version_over_the_commits_own_file_holds_the_folder` (a new file stopped at `Staging::Linked` with a third version committed through the index: held, the file never removed; `git checkout -- NEW.md` settles it, the third version stays; then a rewritten file the same way, held with its old file beside it until checked out and that file removed).
- **R95C6-03 the executable bit is part of ownership** (superseded by R264, round 8 below: modes are held, never transferred; the transfer described here is gone): the commit's own file whose bit differs from the one the commit gave it is the person's change: on a reversal the old file takes that bit (`browse::Displaced::set_executable`, new; `browse::Contained::executable`, new) and goes back; with no old file to take it the settling holds. A reversal whose `HEAD` mode is not the replaced entry's holds too. Proof: `a_persons_executable_bit_on_the_commits_own_file_is_kept` (set on `MEMORY.md` and cleared on an executable `tool.sh` at `Staging::Linked`, then taken back: the old bytes with the person's bit on the disk, index and `HEAD` the old mode, `status` one mode change, a watcher pass commits exactly that bit; a reversal committed as `100644` over a `100755` before-image stopped at `Cut::Displaced`: held, the path empty, until `git checkout -- MEMORY.md`; and a new file stopped at `Staging::Linked`, made executable, then taken back: held, the file and its bit kept, until the person removes it).
- **R250 a reversal whose old file is gone:** `HEAD` holding the replaced entry again needs the old file to give it back; with that file gone the commit's own file stays at the path — never an empty path a watcher would commit as a deletion — and the settling holds. Proof: `a_reversal_whose_old_file_is_gone_keeps_the_commits_file` (stopped at `Staging::Linked`, the `.keeper-displaced-…` file removed, taken back: held, the commit's bytes still at the path; `git checkout -- MEMORY.md` settles it, nothing left, a watcher pass commits nothing).
- **Docs:** `docs/sync.md` § 10 (recovery: what completes, what holds, how a person settles a path), `docs/agents.md` (the night's recovery), DW-970 (widened, all five proofs).
- **Mutation:** `mut-95cons-11b-{1,2,3}.log` (`mut-95cons-11b.py`, rerun after the round's last code edit; supersedes `mut-95cons-11.log`) — all 9 killed by a test panic: a committed deletion completing before the occupant is told (the settling completes, `held`'s `is_err`); a third version at `HEAD` ignored for the commit's own file, and the no-aside shortcut (a third version held only beside an old file) (both: the new file's settling completes); the executable bit left out of ownership, and the person's bit not carried to the old file (both: `the person's bit is kept`); `HEAD`'s mode ignored for a reversal, and a person's bit with no old file to take it dropped (both: the settling completes); a reversal with its old file gone dropping the commit's file (the settling completes); and the whole fix rolled back (`authored.rs` as at `201ded44`: all four tests fail).
- **Signature changes rung 3 adapts to:** none public. Private: `Engine::put_back(root, intent, n, head: Option<&str>)` → `head: Option<(&str, u32)>`. `keeper-sync/src/browse.rs` gains `pub(crate)` `Contained::executable` and `Displaced::set_executable`. `Engine::{commit_paths, settle_commit_paths, unsettled_commit}`, `CommitPaths`, `CommitRequest` and every keeper-agent signature are unchanged.

**As built (rung `agents-95-consolidate`, 2026-10-08, round 8: R95C7-01…03, R264).** Ruling R264 (amends R250's mode clause): no mode transfer — fail closed instead; modes are held, never transferred.
- **`put_back` never chmods anything:** `browse::Displaced::set_executable` is deleted; `browse::{Contained, Displaced}::executable` stay (each side's bit is read). With the commit's bytes at the path, still linked under `Intent::staging(n)` and `HEAD` the replaced entry or no file, the settling holds — before anything is moved — where the file's bit is not its committed after-image entry's, or the old file moved aside has another bit than its committed before-image entry, or either bit cannot be read (an I/O error holds as any other). A matching mode on both sides proceeds as before; the move-off re-check still compares the bit it read. `head: Option<(&str, u32)>` stays: a reversal is `HEAD` holding the replaced entry, mode included.
- **R95C7-01 interrupted recovery:** with no transfer there is no step between a mode and the file that carries it; a held path holds again on every settling. Proof: `commit_paths::a_mode_a_settling_cannot_attribute_holds_the_folder` (bit set on `MEMORY.md`'s commit file at `Staging::Linked`, taken back: held twice, then the file renamed to `.keeper-taken-<request>-0` as a kill after the move off leaves it: held again, the file back at the path with the person's bit, the old file non-executable; the person puts the bit back: settled, the old bytes non-executable, a watcher pass commits nothing).
- **R95C7-02 the old file's bit changed after linking:** held, never overwritten. Same test (set on the moved-aside `MEMORY.md`: held, the commit's file as it was, the person's bit on the old file kept, until put back; cleared on the moved-aside executable `tool.sh`: held, then the person removes the commit's file and the settling puts the old one back with their cleared bit, which a watcher pass commits as `100644`).
- **Bit cleared on the commit's file:** same test (executable `tool.sh` rewritten, cleared at `Staging::Linked`, taken back: held, both files with their bits; `git checkout -- tool.sh` and the old file removed: settled). A new file's bit and a reversal committed with another mode hold as in round 7 (kept in the same test).
- **R95C7-03:** disappears with the setter — no chmod is left in recovery, so no inode to misattribute.
- **Docs:** `docs/sync.md` § 10 and `docs/agents.md` (no bit is moved from one file to another; changing either file's bit holds; putting it back is a resolution), DW-970 (R264, proof renamed), R264 in the decisions file.
- **Mutation:** `mut-95cons-12-{1,2}.log` (`mut-95cons-12.py`) — all 3 killed by a test panic at `held`'s `the settling holds`: the commit's file's bit ignored (the settling completes and the old file goes back without the person's bit), the old file's bit ignored, and the whole fix rolled back (`authored.rs` and `browse.rs` as at `f46ee091`: the transfer completes the settling).
- **Signature changes rung 3 adapts to:** none public. Private: `keeper-sync/src/browse.rs` loses `pub(crate)` `Displaced::set_executable`. `Engine::put_back(root, intent, n, head: Option<(&str, u32)>)`, `Engine::{commit_paths, settle_commit_paths, unsettled_commit}`, `CommitPaths`, `CommitRequest` and every keeper-agent signature are unchanged.

**As built (rung `agents-95-consolidate`, 2026-10-08, round 9: R95C8-01…03, R268).** Ruling R268 (convergence round, completes R264): one attribution check guards every drop of the old file moved aside.
- **The check:** `browse::Displaced::holds(blob, executable)` (`pub(crate)`, new) opens the old file through no link, `fstat`s it, reads its bytes and compares their blob id with the committed before-image, then `statat`s the name and requires the same device, inode and change time and the before-image entry's executable bit; missing, not a file, or changed since the open is `false`, an I/O error propagates. Either holds — the old file and the intent stay (DW-970).
- **R95C8-01 FIXED:** `put_back`'s `committed` closure asks it, and `drop_old` asks it right before every `Displaced::discard` of the old file: the absent-path branch (`HEAD` no file, a settling retried after a kill), the person's-file branch (`HEAD` the before-image blob, the old file kept beside a person's file) and the committed-deletion branch after the commit's own file is dropped; a mismatch is `kept_beside(aside)`. The pre-move guard asks the same `committed` check (bytes and bit) instead of the bit alone. The non-destructive return of a changed-mode old file into an empty reversed path (`HEAD` the replaced entry) is unchanged. Proof: `commit_paths::an_old_file_whose_bit_a_person_changed_is_never_dropped` — bit set (`MEMORY.md`, `100644`) and cleared (`tool.sh`, `100755`) × a deletion committed beside the empty path (stopped at `Cut::Displaced`), a person's file saved at a reversed path (stopped at `Staging::Linked`, reversal through the index), and the post-discard retry (deletion committed, `settle_commit_paths_held` stopped at `Cut::Dropped(0)`, then the bit changed): each held twice through `held` (settling refused, record kept, the watcher passes commit nothing), the old bytes and the person's bit kept, the person's file kept; then resolved by the person — bit put back (the settling drops the old file) or the old file removed — and a watcher pass commits nothing (deletion) or the person's file (saved over).
- **R95C8-02 NARROWED, residual DW-1060:** because the check runs at the drop, a replacement or a bit change made after the pre-move guard is held. Test seam: `Cut::Dropped(n)` (the settling dropped the commit's own file, the old file not settled yet) and `Engine::settle_commit_paths_held(profile, at)` (`pub(crate)`; `settle_commit_paths` calls it with `&|_| true`); `put_back` takes `at`. Proof: `commit_paths::an_old_file_changed_while_its_settling_runs_stays` — deletion committed, at `Cut::Dropped(0)` the old file replaced by a person's file (renamed over it) or its bit set: the settling holds, the record and the old file stay as the person left it; the replaced one then goes to the path as anything of a person's moved aside does (a watcher pass commits it); the bit holds through `held` until the person removes the file. The `statat`→`unlinkat` window (two system calls) and a write within one change-time tick are DW-1060, the class of DW-900.
- **R95C8-03 DEFERRED, DW-1061, no code change:** a kill between `place`'s `linkat` and `unlinkat` in the `.keeper-taken` restoration leaves a same-inode alias that holds every settling; the manual remedy removes the `.keeper-taken-<request>-<n>` name only when `ls -i` shows the path's inode; the eventual fix recognises and finishes the partial restore.
- **Docs:** `docs/sync.md` § 10 (the old file is removed only while it holds its committed bytes and bit, read right before; saving over it holds; a settling run again after a kill after the move off the path or after the drop holds again; the `.keeper-taken` alias and its remedy; the last-instant residual), `docs/agents.md` (the same for the night), DW-970 (R268, the retry/resolution claims corrected, proofs added), DW-1060, DW-1061, R268 in the decisions file.
- **Mutation:** `mut-95cons-13-{1,2}.log` (`mut-95cons-13.py`) — all 6 killed by a test panic: the check skipped on the absent-path branch and on the person's-file branch (`an_old_file_whose_bit_a_person_changed_is_never_dropped`, at `held`'s `the settling holds`), skipped after the commit's file was dropped (`an_old_file_changed_while_its_settling_runs_stays`, the held settling completes), the check ignoring the bit (both new tests and `a_mode_a_settling_cannot_attribute_holds_the_folder`), ignoring the bytes (`an_old_file_changed_while_its_settling_runs_stays`), and the whole fix rolled back to `f2885ca1`'s `authored.rs` and `browse.rs` with only the test seam grafted on (both new tests).
- **Signature changes rung 3 adapts to:** none public. Private: `pub(crate)` `Cut::Dropped(usize)`, `Engine::settle_commit_paths_held`, `browse::Displaced::holds` added; private `Engine::put_back` gains `at: &dyn Fn(Cut) -> bool`. `Engine::{commit_paths, settle_commit_paths, unsettled_commit}`, `CommitPaths`, `CommitRequest` and every keeper-agent signature are unchanged.

### 95.3 — The weekly curator for skills

**Intent:** "i like hermes self improvement mehanism". **Rung:** **epic95-memory**. AD-402; AD-401/AD-416 (a gate's proposals expire here, Q10); FR-806; DW-385, DW-386, DW-440.

**Files:**
- `keeper-ported/src/hermes/curator.rs` (new): Hermes' deterministic state machine.
  - States: `active` → `stale` at 14 days unused → `archived` at 30, using `agent/curator.py`'s `DEFAULT_STALE_AFTER_DAYS` and `DEFAULT_ARCHIVE_AFTER_DAYS`.
  - A stale skill used again is `active`.
  - It never deletes, and it skips pinned and named skills.
- `keeper-core/src/agents/curate.rs` (new, pure):
  - managed = `metadata.keeper_proposal` present (95.2) and `metadata.keeper_pinned` not `"true"` — an agent's skill no person has adopted, which is never offered (Q5, R28 S-12), so nothing ever views it;
  - a managed skill's "last use" is therefore its last change: the latest of its last commit and its creation (a newer applied `patch` is a use);
  - named (Q8);
  - **the gate-proposal sweep** (Q10, R29 F4): every pending proposal whose `origin` is `gate` and whose `created_at` is 30 or more days before the sweep gets `verdict = "expired"`;
  - the sweep plan.
- `keeper-agent/src/curate.rs` (new):
  - weekly at `0 4 * * 0` (Q7), on always-on hosts, under the lease `curate:<drive>` (95.2's arithmetic, with the settle, S-05);
  - `metadata.keeper_stale: "<date>"` set and cleared with `Frontmatter::set_in` (`notes/frontmatter.rs:200`), leaving every other byte;
  - archiving is a move to `_skills/.archive/<name>/`;
  - an expired gate proposal moves to `proposals/done/` beside its `<ulid>.verdict.toml`;
  - one `commit_paths` with `Memory-Origin: curator@<host>`, then the doorbell.
- `docs/agents.md` § *Skills › The curator*, including how a person adopts an agent's skill (delete `metadata.keeper_proposal`; until then it is not offered, and the curator archives it 30 days after its last change) and restores an archived one (move the folder back).

**Acceptance:**
1. **Hermes' state machine.**
   - 13 days unused: `active`.
   - 14: `stale`.
   - 30: `archived`.
   - A stale skill patched again (a newer applied proposal): `active`, and the next sweep clears `keeper_stale`.
   - **Test:** `curator_state_machine` (pure, against `UPSTREAM.md`'s constants).
2. **The curator touches only what the agents made.**
   - **Touched:** a skill carrying `metadata.keeper_proposal`, unchanged for 31 days, is archived.
   - **Never touched:**
     - a skill a person wrote (no such key), unused for 90 days;
     - a skill a person adopted (the key deleted), unchanged for 90 days;
     - a pinned one;
     - one an agent's `[tools].skills` names;
     - one named in a `_workflows/` file.
   - **Test:** `the_curator_touches_only_what_it_made` (a real `_skills/` tree in a temp repository).
3. **An agent's skill ages from its last change, and adoption ends the clock** (R28 S-12).
   - A proposal applied 31 days ago with no later commit is archived; one patched 3 days ago is active; one a person adopted 40 days ago is untouched.
   - The answer comes from git and the skill files alone: no log is read (NFR-116), and deleting `.keeper/agents.db` changes nothing.
   - **Test:** `an_agents_skill_ages_from_its_last_change`.
4. **Archiving keeps every byte.**
   - After the sweep, `_skills/.archive/<name>/` holds every file of the skill, `scripts/` and `references/` included.
   - `git diff --find-renames HEAD~1` reports `R100` for each file, and nothing is deleted.
   - **Test:** `archiving_keeps_every_byte`.
   - **Risk:** a real rename through the sync engine.
5. **The stale mark changes one key.**
   - Setting `metadata.keeper_stale` changes only the `metadata` map's text.
   - `agentskills` still accepts the file (`metadata` is string → string).
   - Clearing it restores the original bytes.
   - **Test:** `the_stale_mark_changes_one_key`.
6. **One commit per sweep, one curator per drive.**
   - `Memory-Origin: curator@<host>`.
   - The lease arithmetic is 95.2's, under `curate:<drive>`.
   - **Tests:** `a_sweep_is_one_commit`, `curation_lease_arithmetic`.
7. **An archived skill is not offered.** 89.3's index skips `_skills/.archive/**`, and `skill_view` (94.2) answers it as not found.
   - **Test:** `archived_skills_are_not_offered`.
8. **A gate's proposals expire unread** (keeper-core over the sweep plan and keeper-agent over a fixture home, a fake clock; R29 F4): `gate_proposals_expire_unread` — a `gate`-origin proposal 29 days old is untouched by the sweep; at the first sweep on or after its 30th day it gets `verdict = "expired"`, moves to `proposals/done/` in the sweep's commit, and was never scored or read into `USER.md`/`MEMORY.md`; the nights in between (95.2) wrote no verdict for it. 99.2 re-runs this test on the gate rung.

**Shell crate:** does not touch it.

**binds:** FR-806, AD-402, AD-396, AD-401, AD-416

**As built (rung `agents-95-curate`, 2026-10-06).** R129's curator half (the sweep never starts while `consolidate:<drive>` is held) and R130 reused: no second lease, no new write path. R-NEW asked: a skill's clock leaves out the curator's own commits; the lease is renewed right before the commit.
- **Ported:** `keeper-ported::hermes::curator` (`transition`, `apply_automatic_transitions`, `DEFAULT_STALE_AFTER_DAYS`/`DEFAULT_ARCHIVE_AFTER_DAYS`) with `hermes/UPSTREAM.md`'s *Curator* section (4 upstream cases ported, 32 not, each named with the reason).
- **keeper-core:** `agents::curate` (`SCHEDULE` `0 4 * * 0`, `lease_key`, `origin_of_host`, `is_curators`, `managed` — `keeper_proposal` and not `keeper_pinned: "true"` —, `names_skill`, `sweep` → `SweepPlan` with its subject); `agents::skills::{metadata_value, set_metadata}` (95.2's `consolidate::stamp` now goes through it).
- **keeper-agent:** `curate::{sweep_window, take_sweep, plan_sweep, run_sweep, Curator}` over 95.2's `take_night`, `Home`, `NightRound`, `change_into`/`settle_into` and `Engine::commit_paths`; agentd ticks the curator beside the consolidator. The doorbell needed no change (the push rings `memory` for the agents zone).
- **Acceptance → proof:** 1 `hermes::curator::tests::curator_state_machine` (+ the four `hermes_upstream_*`), and the reactivation half in `curation::an_agents_skill_ages_from_its_last_change`; 2 `curation::the_curator_touches_only_what_it_made`; 3 `curation::an_agents_skill_ages_from_its_last_change`; 4 `curation::archiving_keeps_every_byte`; 5 `curate::tests::the_stale_mark_changes_one_key`; 6 `curation::a_sweep_is_one_commit`, `hosts::tests::curation_lease_arithmetic`; 7 `curation::archived_skills_are_not_offered` (89.3's index, and `skill_view` refusing); 8 `curate::tests::gate_proposals_expire_unread` (the plan, and 95.2's nights settling nothing of it), `curation::a_sweep_is_one_commit` (moved with its `expired` verdict in the sweep's commit, `MEMORY.md` untouched).
- **Corrections to this text:** the rung is `agents-95-curate`, not `epic95-memory`; the clock is git's alone — AD-402's "reads skill use from the logs" is corrected (a managed skill is never offered, so never viewed); the stale mark re-renders the `metadata` map, so acceptance 5 holds for keeper's canonical shape (codemap §3 row 21); the explicit dotted-folder skip landed in rung 1. Owed: DW-585…DW-587.

**As built (rung `agents-95-curate`, 2026-10-07, review fixes R95U-01…11, R208).** Restacked onto rung 2's R207 (`20aa02fd`); ruling R208 (every finding accepted). The curator's own lease code is gone: `curate::{take_sweep}`, `agents::curate::{lease_key, is_curators}` and `hosts::tests::curation_lease_arithmetic` are deleted.
- **01/02/03 one claim, owed until settled:** `curate::{run_round, week, sweep_drive, run_sweep, Curator}` — the one call of `maintain::maintain` on `claim::maintenance_key` with the completion `claim::completion_key(agents::curate::JOB, drive)`; the commit's publication is fenced (`commit_paths(…, fence)`, `Outcome::Fenced` not settled); `Remembered::{Done, Again}` as the night's, contention/failure/lost claim tried again after `claim::TTL`. Proof: `curation::a_lost_claim_writes_nothing_at_any_point` (lost after the pull, once the lane is held, right before the publication), `hosts::tests::a_week_is_remembered_done_only_when_it_was` (held elsewhere → owed, unrecorded, not asked before its wait; holder died → taken over and recorded under `curate:tgdrive`, never the night's; failed pull → owed, claim released), with rung 2's `hosts::tests::one_maintenance_job_per_drive_whichever_starts_first` for both interleavings and the simultaneous start; `curate::tests::the_week_is_the_latest_sunday_four_oclock` (acceptance 6's window).
- **04 incomplete protection:** `curate::protection` → `agents::curate::Sweep::incomplete` (over 2 000 workflow files, one over 256 KiB, a link, an unparsable `agent.toml`, an uncommitted workflow/`agent.toml`) protects every skill and notes why. Proof: `curation::an_incomplete_protection_read_archives_nothing`, `curate::tests::an_incomplete_protection_read_protects_every_skill`.
- **05/06 one revision, guarded, no walker:** `git::history::files_at` (git's objects of one commit), `consolidate::{Authority::at, text_at}`; every workflow and `agent.toml` read is a guard; the declaration is revalidated at the commit. Proof: `curation::a_reference_the_sweep_cannot_see_protects` (dirty removal, `_workflows` swapped for an outside link on the disk, a workflow and an unserved agent's skills committed after the plan, a changed `_drive.toml`).
- **07 the whole tree:** `consolidate::change_into(home, change, rev, request)` guards every file of a moved folder at the planned commit and its destination absent (the consolidator's approved archives too); `Engine::commit_paths` moves nothing when `HEAD` holds an unguarded file under a moved folder; `Skill::uncommitted` leaves a folder with a change on the disk alone. Proof: `curation::{archiving_guards_the_whole_tree, archiving_keeps_every_byte}` (binary and executable files, modes kept, `R100`), `commit_paths::a_move_takes_only_the_files_it_read`, `curate::tests::a_skill_of_unknown_age_or_with_a_change_on_the_disk_stays`.
- **08 expiry guards:** `consolidate::settle_into` guards the `done/` place absent beside the verdict; `curate::plan_sweep` leaves a proposal whose `done/` place or verdict is committed pending. Proof: `curation::an_expiry_leaves_a_persons_files_alone`.
- **09 the curator's own commits:** `MemoryTrailer::of_message` (keeper's trailer block only), `git::history::path_changes` (merges as git simplifies them, from the pinned commit), `curate::{by_curator, last_change, LOG_DEPTH}` (32; no real change found → unknown age, left alone); committer dates. Proof: `curation::the_curators_own_commits_are_known_by_their_trailer` (quoted line, merged stale mark, 32 curator changes, author ≠ committer), `history::tests::a_merge_carrying_one_sides_change_is_not_a_change_of_its_own`, `provenance::tests::a_memory_trailer_is_only_keepers_own`, `history::tests::files_at_list_one_commits_folder`.
- **10 server clock:** `curate::sweep_drive` reads `ServerClock::now` right before the plan. Proof: `curation::the_sweep_runs_at_the_servers_time` (±2 h skew).
- **11 fixed clock:** `curation.rs` seeds every fixture from one instant (`t0`), exact 30-day boundary (`a_sweep_is_one_commit`: a second short stays), differing author/committer dates; `curate::tests::a_gate_proposal_expires_at_exactly_thirty_days` (−1 s, exact, a `+02:00` spelling of the same instant).
- **Docs:** `docs/agents.md` § *Skills › The curator*; `docs/sync.md` § `Engine::commit_paths` (moved folders). DW-585 and DW-587 closed; new DW-740…DW-745.

**As built (rung `agents-95-curate`, 2026-10-07, restack onto `ce1a3491`, then `739e8cbc`, and re-review fixes R95U2-01…06, R229, R230).** Restacked onto rung 2's round 4 (R217): `curate::week` passes `maintain::maintain` the owned `Arc` ports and clocks, the curator's pull is `consolidate::pull` (off the polling task), docs/decisions/DWs/`UPSTREAM.md` merged with both rungs' text. Restacked again onto rung 2's R233 round (`7043378f`, then its amend `739e8cbc`): decisions, DWs, docs and `hosts.rs` tests merged with both rungs' text; no curator signature changed. Rung 2's `commit_paths::{the_attributes_a_routed_write_needs_are_no_moved_path, executable_files_stay_executable_on_the_disk, a_persons_mode_change_is_never_undone}` now guard every file they move, as R208's move rule asks (unguarded, they were `Guarded` before the check they prove). Ruling R229 accepts DW-740; ruling R230 accepts every finding. Inherited closes, by rung 2's tests: R95U-01's fence at the CAS (`commit_paths::a_lease_lost_before_the_branch_moves_moves_nothing`) and renewal on its own task (`hosts::the_renewal_runs_while_the_work_blocks_its_task`); R95U-02's unfinished transaction, now also the curator's (`curate::followed` over `Engine::unsettled_commit`, rung 2's proof `hosts::a_night_whose_commit_did_not_finish_is_tried_again`); R95U-05's one HEAD snapshot (`commit_paths::a_commit_made_after_the_checks_is_never_built_on`). Mutations: `/tmp/agents-salvage/mut-95cur-6.log`, `/tmp/agents-salvage/mut-95cur-7.log`.
- **R95U2-01 strict block:** `MemoryTrailer::of_message` takes only a last paragraph that is exactly keeper's block (provenance keys in order, tags, valued memory trailers, one `Memory-Origin`, nothing else); `curate::is_curator_origin` wants a host slug; `Provenance::parse` stays tolerant. Proof: `curation::only_the_curators_whole_block_is_the_curators` (prose, partial, twice, no host: kept; genuine: archived), `provenance::tests::a_memory_trailer_needs_keepers_whole_block`.
- **R95U2-02 TREESAME:** `git::history::path_changes` walks only the TREESAME parent of a commit equal to one. Proof: `history::tests::a_discarded_side_is_no_change` (revision ids), `curation::a_discarded_patch_does_not_keep_a_skill` (archived as kept).
- **R95U2-03 disk membership:** `browse::members` and `authored::stray` in `Engine::commit_paths_held` at the guards and right before the publication — the engine change named for rung 2's rebase. Proof: `commit_paths::a_move_holds_for_a_file_on_the_disk_it_never_read` (one there before, one put there at `Cut::Prepared`: no publication, no move, no record, the new file kept; the check at the guards as R95U3-02 below says), `curation::archiving_guards_the_whole_tree` (post-plan uncommitted file: no publication, folder whole). DW-741 closed.
- **R95U2-04 guarded or unfinished is owed:** `curate::week` settles only `Outcome::Committed` with nothing unsettled (`curate::followed`); `Skipped` is retried the same window. Proof: `hosts::tests::a_week_held_at_its_publication_is_owed_again` (real home: pull → held → unrecorded; published but its files not followed → unrecorded; both gone → done and recorded the same week).
- **R95U2-05 clocks by hand:** `ServerClock::on_wall`; `curation::{a_lost_claim_writes_nothing_at_any_point, the_sweep_runs_at_the_servers_time}` read no real clock (`at_server`); the lost-claim drive lives seventy years ahead of any machine's clock (git's dates stop at 2099), so its fence is reached only through the clock the sweep is handed. R208's "one fixed instant" now holds for every curation fixture except `hosts::tests::a_week_held_at_its_publication_is_owed_again`, whose proposal is 31 days old by the real clock (a day's margin; the claim fixture runs on it).
- **R95U2-06:** the exact-subject assertion of `curation::a_sweep_is_one_commit` is deleted.
- **Docs:** `docs/agents.md` § *Skills › The curator* (retry, block shape, simplification, disk additions); `docs/sync.md` (moved folder's disk membership). New DW-885…DW-887; DW-740 accepted (R229), DW-742 narrowed.

**As built (rung `agents-95-curate`, 2026-10-08, review fixes R95U3-01…03, R251).** No restack this round: the rung still sits on `739e8cbc`.
- **R95U3-01 one reading of `metadata`:** `skills::metadata_of` (new, `pub`) is the single-block/distinct-key classification `skills::adoption` held inline for R204 — `None` for a construct the parser does not model, a key said twice or `metadata` said twice; `skills::adoption` and `skills::metadata_value` read through it. `curate::ownership` → `curate::Ownership::{Managed { stale }, Kept, Unreadable}` replaces `curate::managed`; `curate::sweep` leaves an `Unreadable` skill unchanged and notes why, choosing no occurrence of a conflicting key. Proof: `curate::tests::ambiguous_ownership_metadata_keeps_a_skill` (pure: a repeated `keeper_pinned`, a second `metadata` block — neither archived nor marked, one note each, the readable one beside them archived), `curation::ambiguous_ownership_metadata_is_never_archived` (real sweep: same bytes, no `.archive/<name>`, the readable one archived).
- **R95U3-02:** the `Cut::Checked` stage-order assertion of `commit_paths::a_move_holds_for_a_file_on_the_disk_it_never_read` is deleted; its early case is an ordinary `commit_paths` call. The check at the guards is proved by a durable-state difference instead: with the file there before and the process killed at `Cut::Recorded`, nothing is left to settle (`Engine::unsettled_commit` false) — without that check the request reaches its record and the kill leaves one holding the folder's commits. Uninterrupted, the listing right before the publication holds the same state, so `curation::archiving_guards_the_whole_tree` cannot tell the two apart and is not claimed for the early check.
- **R95U3-03:** the phrase and echo assertions are deleted, nothing re-pinned: `curate::tests::an_incomplete_protection_read_protects_every_skill` (the supplied sentence), `curation::the_curators_own_commits_are_known_by_their_trailer` ("last change"), `curation::archiving_guards_the_whole_tree` ("exists already"), `curation::tidy_stays` (the supplied fragment; its argument only labels the case now), `curation::a_reference_the_sweep_cannot_see_protects` ("not committed"), `curation::archived_skills_are_not_offered` (the reason's text; `ToolOutcome::Refused` and no file read stay).
- Mutations: `/tmp/agents-salvage/mut-95cur-8.log`, `/tmp/agents-salvage/mut-95cur-9.log`. Docs: `docs/agents.md` § *Skills › The curator* (unreadable `metadata` stays). No DW opened.

**As built (restack onto `419c9a30`, 2026-10-08).** Restacked onto rung 2's final round (R246, R250, R264, R268; R271): decisions, DWs and `docs/agents.md` merged with both rungs' text, `browse::members` and `authored::stray` beside rung 2's `put_back`/`Displaced::holds` unchanged, no curator signature changed; a moved folder settles as its files under rung 2's `put_back` — a kill while its archive copy is staged, the move then taken back, leaves a source file the commit had already removed deleted on the disk (DW-1070, R273; no engine change on this rung). Mutations: `/tmp/agents-salvage/mut-95cur-10.log`.

### 95.4 — `drive_search` over the drives' OKF bundles (keeper-ported::okf)

**Intent:** "nixi needs to be told to use what drive context - but can multiple"; "make sure the sensitive part goes only to the private bots/drives". **Rung:** **epic95-knowledge**. AD-403 (DW-212 taken for agents); AD-396 (`okf` lands here, ported from the drive's own tools); AD-390 (labels); AD-159 (bounds); FR-807; D-21.

**Files:**
- `src-tauri/crates/keeper-ported/src/okf/` (new), ported from `tgdrive:.okf/bin/`:
  - `yaml.rs`: the drive's fallback YAML subset, `okf_lib.py:151-279`, which agrees with PyYAML on the drive's config;
  - `config.rs`: `load_config` with bundles, `exclude`, `guides`, `no_index` and `listed` (`:781-816`);
  - `matcher.rs`: `_match` (`:819-830`), `is_excluded` with guides winning (`:833-836`), `bundle_for` with the innermost bundle (`:770-778`);
  - `doc.rs`: the frontmatter split, `title` (frontmatter, first `# ` heading, then the stem with `-`/`_` as spaces; `:300-308`), `description` (`:310-315`);
  - `index.rs`: the listing grammar of `okf_index.py:101-174` (bundle frontmatter; `## Bundles`, `## Staging`, `## Directories`, `## Documents`; `* [title](link) - description`; `%20` for spaces);
  - `links.rs`: `resolve` (`okf_links.py:218-241`).
  - `UPSTREAM.md`:
    - source: the drive's `.okf/bin/` at the named tgdrive commit;
    - licence: the owner's statement (OA-95-2), or, until it is made, "written from the scripts' documented behaviour" (ruling R21);
    - Q9's finding;
    - the generator for the parity fixtures.
- `keeper-core/src/agents/search.rs` (new, pure):
  - the query: literal terms, folded as `notes/search_index.rs`'s `build_match` folds them, never a regex;
  - the result shape: path, title, OKF `type`, up to three matching lines, the drive, and the label;
  - the bounds: `k` 10 by default and at most 25; lines of at most 240 characters; the rendered result at most 80 KiB (AD-159);
  - the disclosure sentence;
  - a result's label: the drive's readers; `agent` integrity when `generated.by` is not a person or `human_reviewed: false`, otherwise `owner` (AD-390).
- `keeper-agent/src/search.rs` (new):
  - **Per drive in scope:** read `.okf/config.yaml` through `browse::resolve`.
  - **The candidates:** included bundles' Markdown, minus the exclusions, minus keeper's own rules (a session's `workspace/`, `.keeper/`, `.git/`, an LFS pointer, an unmaterialised file, a file over 1 MiB).
  - **The ranking:**
    - inside the notes vault: `SearchIndex::open_read_only` (`search_index.rs:171`), `query` and, when `check_sink(Model { local })` (92.1; R28 S-04) allows the configured embeddings provider for the session's label and it embeds the query within 1 s, `cosine_top_k` and `fuse` (`:292`, `:362`, `:559`); a `local_only` label and a provider that is not local means lexical only, said so, and the query never leaves the host;
    - elsewhere: the bundles' `index.md` listing lines first, then a bounded lexical scan (at most 2 000 files, 16 MiB and 1.5 s, in bundle order).
  - **Then:** merge, label, and join the labels into the session's label.
- `docs/agents.md` § *Searching the drives*.

**Acceptance:**
1. **The drive's own tools agree with the port, on the drive's own config.**
   - **The config:** `load_config` over a fixture copy of `tgdrive:.okf/config.yaml` equals `okf_lib.load_config`'s output, with 9 bundles, 16 exclusions and the folded `note: >-` scalar.
   - **The paths:** `is_excluded` and `bundle_for` over a 40-path table equal the Python's answers, generated by the drive's code. The table includes:
     - `00-inbox/x.md` excluded, while `00-inbox/README.md` is a guide;
     - `30-work/clients/acme/a.md` excluded;
     - `notes/a.sync-conflict-1.md` excluded at any depth;
     - `10-notes/.keeper/x.md` excluded;
     - `50-library/books/x/README.md` excluded;
     - `60-sessions/_template/README.md` in `tgdrive-sessions`;
     - `80-agents/nixi/MEMORY.md` in the root bundle `tgdrive`;
     - `60-sessions/active/s/workspace/a.md` **not** excluded (Q9).
   - **Test:** `okf_matching_matches_the_drives_own_tools` (keeper-ported).
   - **Risk:** the real config and the drive's own answers. If the owner does not want the config in this repository, the fixture becomes an `#[ignore]` test reading `$KEEPER_TGDRIVE`, named in the PR (OA-95-2).
2. **Excluded files are never opened.**
   - **Setup:** on a fixture drive, the query occurs only in `00-inbox/secret.md`, `99-temp/x.md`, `recordings/x.md` and `30-work/clients/acme/x.md`, each of mode `000`.
   - **Expected:** the search returns no results and no error. Any open of those files would have failed.
   - **Test:** `excluded_files_are_never_opened` (keeper-agent, real permissions).
3. **keeper's own rules hold where the drive's do not** (Q9).
   - Never searched:
     - a session's `workspace/`, though the drive's pattern is inert;
     - `.keeper/` and `.git/`.
   - **Test:** `session_workspaces_are_never_searched`.
4. **A pointer is never read as text, and nothing is fetched.**
   - **The fixture:** an LFS pointer file (`version https://git-lfs.github.com/spec/v1`, `oid`, `size`) whose pointer text contains the query.
   - **Expected:**
     - it is not a hit;
     - a pointer whose *name* matches is reported as "not on this device (<size>)" without its text;
     - no network request is made.
   - **Test:** `pointers_are_never_read_as_text` (real pointer bytes, keeper-sync's pointer probe).
5. **The vault's own index ranks the vault.**
   - **Setup:** a temp vault indexed through keeper-core's `SearchIndex`, inside a drive with an OKF config.
   - **Vault hits:** come from the index (FTS5; with vectors when the fake provider embeds within 1 s; lexical only, said so, when it does not).
   - **Hits outside the vault:** come from listings and the scan.
   - **Test:** `the_vault_index_ranks_the_vault`.
6. **Bounded and disclosed** (AD-159).
   - `k` defaults to 10, and a `k` of 100 is clamped to 25.
   - A scan that reaches its cap says "searched 2 000 of 5 312 files in tgdrive; stopped at the cap".
   - A line is cut at 240 characters with `…`.
   - **Test:** `drive_search_is_bounded_and_says_so`.
7. **A query is matched literally** (the rule `drive_grep` follows, `spec-61-11-…:34`). `a.*b` finds the four characters `a.*b`.
   - **Test:** `a_regex_query_is_matched_literally`.
8. **Scope and labels.**
   - **Out of scope:** a drive not in the session's scope is refused by name.
   - **Two drives:** searching tgdrive and neuradrive in one call labels each result with its own drive's readers and joins the session label to `{tgorka}`, with a `label` line.
   - **An agent-written note:** a hit in a harvested note (`human_reviewed: false`) carries `agent` integrity.
   - **Test:** `results_are_labelled_and_scope_is_enforced`.
9. **A drive without OKF configuration.**
   - Its notes vault only is searched, with "this drive has no OKF configuration; searched its notes only".
   - **Test:** `a_drive_without_okf_searches_its_notes_only`.
10. **On Linux, lexically** (DW-388). On an agentd host there is no vault `search.db`. The result says "lexical: no notes index on this host", and nothing tries to create one.
    - **Test:** `agentd_searches_lexically`.
11. **Derived and disposable** (D-21). After 50 searches, the fixture drive's `git status --porcelain` is empty.
    - **Test:** `drive_search_writes_nothing`.
12. **A query stays on a local model when the label says so** (keeper-agent over the fake provider; NFR-115's embeddings row, R28 S-04): `a_local_only_session_never_embeds_at_a_remote_provider` — a session whose label is `local_only`, searching a vault whose configured embeddings provider is not local, gets lexical hits and "lexical: this session stays on local models", and the fake provider receives no request; the same search in a session that is not `local_only` embeds the query.

**Shell crate:** does not touch it. The desktop host reads the vault index through `keeper-agent`, from the profile's vault path.

**binds:** FR-807, NFR-115 (the embeddings model call), AD-403, AD-396, AD-390, AD-391, D-21

**As built (rung `agents-95-search`, 2026-10-06).** Rulings R133, R136, R137 (option b: OA-95-2 is open).
- **Ported:** `keeper-ported::okf::{yaml, config, matcher, doc, index, links}` with `UPSTREAM.md` (`licence: written from documentation; no upstream code is copied`): written from `OKF-0.2-digest.md`, the config's comments and generated listings; the scripts were run, never read. Fixtures `tests/fixtures/okf/` (`config.json`, `match.jsonl` 4 260 rows, `paths.jsonl`, `drive-paths.jsonl`, `docs.jsonl`, `links.jsonl`, `index.jsonl`, `yaml.json`) from `generate.py`. `matcher::excludes_folder` tells a walk which folders it need never enter. Running the tools found where the drive's fallback YAML parser and PyYAML disagree (a trailing comment, `''`); PyYAML's answer is the drive's on any host with PyYAML, and the port's.
- **keeper-core:** `agents::search` (`Query` — `build_match`'s terms, matched literally and folded; `k_of`, `clip_line`, `merge`, `render`, `Said`'s sentences; the bounds 10/25/3 lines/240/80 KiB, the scan's 2 000 files/16 MiB/1.5 s, 1 MiB per file, 1 s to embed); `notes::search_index::SearchIndex::paths_of`; `tier` row `DriveSearch` at T0 and its summary.
- **keeper-sync:** `bots_fs::search_walk` (name order, depth first, the visitor's `Step::{Enter, Skip, Stop}` decides before anything is opened, its start first; a `ScanBudget` shared by every read and walk of a call), `bots_fs::search_landing` and `ScanBudget::read` (the no-follow reader, below); pointers by size, never hydrated; dataless files never opened.
- **keeper-agent:** `search::{SearchTools, SearchDrive, Embeddings, configured_embeddings, http_embeddings}` served through `run_named` (a T0 read, each drive granted as a `drive_read` of its root); `agent::file_label` shared with the drive reads; the review pass's reads include `drive_search` (`REVIEW_READS`, R126's review run). 94.4's context-free helper was not below this rung when it was built (R209 rejects R-NEW-1 as completion equivalence); since the restack onto `c57046b8` a helper is offered `drive_search` and runs it as the session's own (the restack's As built below). **agentd:** `report::tools` counts it implemented.
- **Acceptance → proof:** 1 `okf::tests::okf_matching_matches_the_drives_own_tools` (`#[ignore]`, `$KEEPER_TGDRIVE`; run green 2026-10-06) with `okf_config_reads_as_the_drives_load_config`, `okf_patterns_match_as_the_drives_match`, `okf_paths_are_placed_as_the_drives_tools_place_them`, `okf_documents_read_as_the_drives_read_doc`, `okf_links_resolve_as_the_drives_resolve`, `okf_listings_read_back_what_the_drives_index_wrote`, `matcher::tests::a_folder_is_excluded_only_when_everything_under_it_is`; 2 `search::tests::excluded_files_are_never_opened` and keeper-sync `a_search_walk_opens_only_what_its_visitor_enters`; 3 `session_workspaces_are_never_searched`; 4 `pointers_are_never_read_as_text`; 5 `the_vault_index_ranks_the_vault` (an HTTP embeddings stub, fast and 1.5 s late); 6 `drive_search_is_bounded_and_says_so`, core `results_are_bounded_and_the_cap_is_said`; 7 `a_regex_query_is_matched_literally`, core `a_query_is_literal_terms`; 8 `results_are_labelled_and_scope_is_enforced`, `agent_turns::drive_search_joins_each_hit_into_the_session_label`; 9 `a_drive_without_okf_searches_its_notes_only`; 10 `agentd_searches_lexically`; 11 `drive_search_writes_nothing` (the index's own `.keeper/` aside, DW-710); 12 `a_local_only_session_never_embeds_at_a_remote_provider`.
- **Corrections to this text:** the label rule is R133's (`label_drive_read`, so a hit reads at most `agent`), not "owner unless agent-written"; the literal guarantee is the scan's, the vault index matches FTS5 tokens; listings are read at bundle roots (DW-711); outside the vault hits keep bundle order (DW-712). Deferred: DW-710…DW-714.
- **Review fixes R95S-01…14 (R209–R211, 2026-10-06).** keeper-sync: `bots_fs::search_landing` (a path's drive-relative landing through `browse::resolve`); `ScanBudget::read` and `search_walk` read through `nofollow` — `openat(O_NOFOLLOW)` a name at a time from the canonical root, `AT_SYMLINK_NOFOLLOW` stat before the open (regular, not `SF_DATALESS`, size) and the handle's `fstat` after it (same `(st_dev, st_ino)`), pointer test and text from the same bytes, `O_NONBLOCK` so a FIFO is never waited on; every byte read counts — the config's and the listings' too — and the next read takes at most what is left (`Unread::Capped`); `ScanBudget::clock` (`Clock`); the deadline and `max_entries` stop the walk while a folder is read (`SearchWalk::walk_capped` = the count is not the total). keeper-core: `Said::ScanCapped { of: Option<usize> }`, `Said::IndexUnusable`, `Said::UnreadableOkf` now refuses the drive. keeper-agent `search`: `kept_out` folded (`eq_ignore_ascii_case`); `read_admitted` admits every read where asked and where landed (`Plan::{admits, walks, reads_listing}`), then reads the landing; `read_config` (absent → notes; anything else unreadable, or past the call's bytes left → the drive refused); `open_index` (exactly `<vault-landing>/.keeper/search.db`, a regular file); `rank` (query or `paths_of` error → said, vault scanned); `text_hit`/`pointer_hit`/`scan_hit` label from the bytes read (`label_of`: requested ⊔ landed), a listed document joined with its listing's label; no reopen (`file_head` removed); `SearchTools::with_clock`.
  - **Finding → proof:** 01 `listings_and_bundle_folders_are_admitted_before_they_are_opened` (an excluded bundle's folder and `.GIT/` never listed: a clock that ticks at every entry looked at would run out on their 1 600 entries each), keeper-sync `a_link_put_in_an_offered_entrys_place_is_never_followed` (a skipped start opens nothing); 02 `a_path_is_admitted_where_it_lands_as_well_as_where_it_is_asked`; 03 `a_link_put_in_an_offered_entrys_place_is_never_followed` (file and folder replaced by links after their offer, a landing whose folder became a link, a FIFO); 04 `listings_and_bundle_folders_are_admitted_before_they_are_opened` (`.GIT`, `.Keeper`, `.KEEPER`, `60-Sessions/…/Workspace`, `WORKSPACE`); 05 `a_hits_label_is_the_bytes_it_was_made_from`; 06/07 `listed_and_indexed_pointers_and_odd_files_are_read_as_the_scan_reads_them`; 08 `an_unreadable_or_unsupported_okf_config_searches_nothing`; 09 keeper-sync `the_search_budget_counts_every_byte_and_caps_the_next_read_at_what_is_left`, `a_config_is_read_within_the_calls_bytes_left`; 10 keeper-sync `a_search_walk_stops_at_its_deadline_and_its_entry_bound`, `drive_search_is_bounded_and_says_so` (frozen and ticking clocks); 11 `an_index_that_cannot_answer_leaves_the_vault_scanned`; 12 `okf_matching_matches_the_drives_own_tools` (whole `load_config` vs `generate.py --drive-config`, `#[ignore]`, run over `/workspace/tgdrive`); 13 `drive_search_writes_nothing` (whole drive, the two sidecars allowed, R211); 14 `ranked_notes_keep_their_title_and_lines`. Deferred: DW-715…DW-718; DW-710 closed by R211.
- **Review fixes (round 2: R95S2-01…05, R218, 2026-10-07).** 01 keeper-sync `bots_fs::search_landing` answers `Ok(None)` only where `canonicalize` says `NotFound` and `browse::landing` finds no dangling link; any other failure (a `.okf/` that may not be searched, an I/O error) is `FsRefusal::Unreadable`, so `read_config` refuses the drive. 02 `ScanBudget::admit` is the one admission — files opened, bytes read and the deadline, checked and the open counted once — behind `ScanBudget::read` (config, listings, ranked and listed documents) and the walker alike (`ScanBudget::take`); `Searching::index_hits`/`listing_hits` stop at the first `Unread::Capped` and the drive says `Said::Incomplete`; the config's refusal names the bounds (`agents::search::bounds_words`). 03 `bots_fs::search_file` admits `<vault>/.keeper/search.db` with the content reader's check (`nofollow::file_at`: no-follow descent, `AT_SYMLINK_NOFOLLOW` stat, regular, not `SF_DATALESS`) before `open_index` hands its path to SQLite; the reopen-by-path window stays DW-717. 04 `SearchIndex::open_bounded` (busy wait ≤ the call's time left, a progress handler every `STEPS_PER_CHECK` = 16 steps interrupting on the call's clock → `SearchIndexError::Bounds`) and `Meter` (`query_within`, `cosine_top_k_within`, `paths_of`: every row stepped counted as its values — ids and paths at their length, numbers at 8 bytes, vectors at their blob), charged to the call by `ScanBudget::charge`; a stopped meaning ranking is `Said::MeaningCapped`, a stopped index `IndexUnusable`; rusqlite's `hooks` feature on keeper-core. 05 `a_config_is_read_within_the_calls_bytes_left` runs on `frozen()`.
  - **Finding → proof (round 2):** 01 `a_config_folder_that_cannot_be_searched_searches_nothing` (a `.okf/` at mode 000 and a readable `10-notes/private/x.md` it excludes: not found, the drive refused; then a dangling `config.yaml` link, refused); 02 `a_config_is_not_opened_once_the_calls_time_is_up` (a clock past the deadline after its first look), `a_later_drive_opens_nothing_once_the_calls_files_are_spent` (2 050 files then a second drive, frozen clock), `a_listing_longer_than_the_calls_files_stops_at_them_and_says_so` (2 100 listed documents, those past the 2 000th at mode 000: none opened, said), `ranked_notes_and_listings_stop_at_the_calls_bytes_and_say_so`; `drive_search_is_bounded_and_says_so` now reads "searched 1 999 of 5 312" (the config is the first of the 2 000); 03 keeper-sync `nofollow::tests::a_file_whose_content_is_elsewhere_is_never_handed_over` (the materialization seam); the macOS run stays owed (DW-715, DW-830); 04 keeper-core `a_bounded_reader_is_handed_no_more_than_its_meter`, `the_indexs_rows_count_against_the_calls_bytes` (140 vectors of 128 KiB: meaning stopped and said, the next folder's file not opened), `the_indexs_ranking_stops_when_the_calls_time_is_up` (the clock passes the deadline while the query is embedded); 05 `a_config_is_read_within_the_calls_bytes_left`. Mutants: `/tmp/agents-salvage/mut-95srch-r2.log`. Deferred: DW-830…DW-834.
- **As built (review fixes round 3: R95S3-01…06, R236, rung `agents-95-search`, 2026-10-07).** 01 keeper-core `notes::search_index::Meter` admits before it fetches: `Meter::step` checks a row's numbers (8 bytes each) before every step, `Meter::fetch` admits a value by the length SQLite stores (`octet_length`) before a second statement of the same read snapshot fetches it — `run_match` (note id by chunk rowid), `cosine_top_k_within` (the blob by `chunk_rowid`, only where its stored width is the query's; a kept candidate's id), `paths_of` (the path) — so `taken` never passes the allowance and an oversized value ends the read unfetched; `hit_row` removed. 02 keeper-sync `ScanBudget::admit_open` (the one admission, counted as an open, no document-size limit) before `SearchIndex::open_bounded` in keeper-agent `open_index`, which now returns the `Said` to report (`Said::IndexCapped` when refused). 03 `ScanBudget::exhausted` (non-consuming; `admit` now built on it) asked before each ranked note, each listing and each listing line in `Searching::index_hits`/`listing_hits`. 04 `Plan::indexed` removed: the vault is scanned after the ranked notes on every drive, configless included (`Searching::done` keeps a landing from being read twice by the index, a listing or the walk), and `search_drive` says `Said::IndexPartial` when the index ranked and the drive's reading or scan stopped early. 05 `read_admitted` turns a landing that cannot be asked about into `Unread::Skipped` (counted as not searched); `SearchWalk::unreadable` counts folders not opened, not listed, listed only in part or holding an entry whose kind could not be asked (`NotFound` stays an absence); `Searching::scan` adds bundles whose landing or walk failed and says `Said::Unlisted`. 06 `SearchTools::answer` returns the call's `Answer { query, hits, said }`, which `run` renders; the search tests assert `Said` facts and hits, every prose pin and `bounds_refusal` deleted; `agents::search`'s test no longer pins a grouped sentence.
  - **Finding → proof (round 3):** 01 `notes::search_index::tests::a_bounded_reader_is_handed_no_more_than_its_meter` (exact bound answers with `taken` equal to it; one byte short, zero, a 1 MiB path and a 128 KiB vector against 64 KiB each end with `Bounds` and `taken` within the allowance), and `the_indexs_rows_count_against_the_calls_bytes` (the vectors past the bytes are never fetched, the words still rank the vault at this one vector width — R248 reopens DW-833 for widths that leave the paths too few bytes — and a 512 KiB file no longer fits); 02 `the_index_is_opened_only_within_the_calls_bounds` (an unreadable `search.db`: a later configless drive after 2 050 files, a config that took the call's last file, and a clock past the deadline at the index's admission each say `IndexCapped` and never `IndexUnusable`, which an open would produce); 03 `candidates_stop_at_the_bounds_though_none_is_read` (3 000 listing lines to missing documents and 3 000 bundles with no listing under a ticking clock; three ranked notes all gone after the index's open took the last file — each `Incomplete`); 04 `an_index_that_may_be_stale_never_stands_for_the_vault` (an empty index beside a matching note, with and without a config; a note renamed and one added after indexing; a current index with nothing to find answers empty with no disclosure; a scan stopped by the files after the ranked note says `IndexPartial`); 05 `folders_that_cannot_be_read_are_said_not_taken_for_empty` (a folder at 000, one at 0400, a bundle at 0100, a bundle under a 000 folder: `Unlisted { folders: 5 }` and the listed document and the bundle's listing behind them `Skipped { files: 2 }`; the same folders readable and empty say neither); 06 every converted test, and `drive_search_is_bounded_and_says_so` / `agents::search::tests::results_are_bounded_and_the_cap_is_said` as the narrow proof that the rendered result carries every fact said. Deferred: DW-930…DW-934. The round-3 mutation logs (`mut-95srch-r3/r4/r4b.log`) prove the meter's accounting (admission missing, admission after charging); they do not prove that nothing is extracted before admission — round 4's do.
- **As built (review fixes round 4: R95S4-01…07, R248, rung `agents-95-search`, 2026-10-08).** 01 keeper-core `SearchIndex::reader` reads `PRAGMA encoding` once at open and refuses anything but `UTF-8` with `SearchIndexError::Encoding` (a stored length is the length handed over only in UTF-8), so `open_index` says `Said::IndexUnusable` and the vault is scanned past it. 02 `Meter::fetch(length, fetch)` is the one extraction path — the closure, counted on the test thread (`tests::EXTRACTED`), runs only after `admit(length)` and returns the bytes it handed over; `Meter::text` serves ids and paths, the vector blob goes through `fetch` by its width. 03 keeper-ported `okf::index::entry_line` reads a line in its length (every `](` before the same `)` shares that `)`, its tail and the last space before it; no `)` left ends the line); `okf::index::lines` yields a listing's lines one at a time (`Lines`, an entry or `None`), `parse` collects it; `Searching::listing_hits` asks `ScanBudget::exhausted` after every line, whatever it is. 04 keeper-sync `ScanBudget::walk_spent` (time or entries spent, non-consuming) is asked before each scan start is resolved; once it says so no start is resolved and the count is no total. 05 keeper-agent `Done` (`Pointer(size)` or `Read`) is what a landing's one read came to: `read_admitted` and the scan never read a landing twice, and evaluate a kept pointer again under a later name (the scan after its walk, from `again`). 06 `Searching::scan` gives `ScanCapped { of: None }` whenever a folder was not read whole (`unlisted > 0`), the mid-enumeration seam included by construction (its count is the same `SearchWalk::unreadable`). 07 docs only: DW-833 reopened and narrowed, DW-931 broadened to a lexical ranking below its `LIMIT`, `docs/agents.md` corrected (the words' ranking survives a stopped meaning ranking only where the paths still fit; the UTF-8 rule; the per-line and per-start checks).
  - **Finding → proof (round 4; mutation log `mut-95srch-r5.log`):** 01 `notes::search_index::tests::an_index_whose_text_is_not_utf8_is_never_read` (a UTF-16le index holding the path `港` is refused at open; in UTF-8 the path is admitted at 8 + 3 bytes and refused at 8 + 2 within the allowance) and `search::tests::an_index_not_stored_as_utf8_is_said_and_scanned_past`; 02 `a_path_past_what_is_left_is_never_extracted`, `a_vector_past_what_is_left_is_never_extracted` (nothing extracted before the refusal — the order-only mutant that keeps charging after admission is killed by the extraction count); 03 keeper-ported `okf::index::tests::every_short_line_reads_as_the_proved_grammar` (every line of up to seven of `[ ] ( ) space - a` reads as the grammar proved against the drive's listings), `a_line_of_delimiters_reads_in_its_length` (three lines of half a million `](` within 2 s; DW-991), and `search::tests::a_listing_is_read_within_the_bounds_a_line_at_a_time` (3 000 delimiter lines run out a ticking clock; the matching line after them is never read); 04 `no_bundle_is_walked_once_the_time_is_up` (3 000 bundles each holding a file cost fewer than 50 looks at the clock, and `ScanCapped { of: None }`); 05 `a_landing_read_under_one_name_is_evaluated_under_another` (a ranked note now a pointer is one size-only `Found::Listing` hit under the listing's matching link to it; a ranked binary note is `Skipped { files: 1 }`, not read again under its listing link); 06 `a_folder_not_read_leaves_the_capped_count_unknown` (2 100 files at a frozen clock beside a folder at 000: `ScanCapped { of: None }` and `Unlisted { folders: 1 }`); 07 the ledger and docs. Deferred: DW-990…DW-992.
- **As built (review fixes round 5: R95S5-01…03, R255, rung `agents-95-search`, 2026-10-08).** 01 + 02 keeper-agent `Searching::scan` keeps the walk's hits through one `keep` (a `RefCell` over the drive's `hits` and `seen`, shared by the walk's `visit` and `found` callbacks): a landing the call read already as a pointer (`Done::Pointer`) is evaluated inside `visit`, where the walk offers it — after keeper-sync's `nofollow::walk` has asked the time for that entry, before the next — so it takes its place in walk order among the files the walk reads, and nothing is evaluated after a walk stops. Round 4's `again` queue and its post-walk flush are gone. Such a pointer opens nothing, so the file and byte caps still do not stop it; the deadline does. 03 keeper-ported `okf::index::Lines` yields `Option<LineEntry<'t>>`, whose `section: &'t str` borrows the listing's heading, so a short entry under a long heading costs its own length; `parse` converts each into an owned `Entry` (`From<LineEntry>`), so the owned `Listing` still copies the heading into each entry, as before. `Searching::listing_hits` compares the borrowed section, unchanged otherwise.
  - **Finding → proof (round 5; mutation logs `mut-95srch-r6.log`, `mut-95srch-r6b.log`; script `mut-95srch-r6.py`):** 01 `search::tests::a_pointer_read_already_is_not_evaluated_once_the_time_is_up` (forty pointers a listing read first, met again through the bundle `needle` (a link to `plain`) as the call's last forty clock looks, counted on a still clock; with the time running out at the twenty-first, exactly the twenty met before it are results and the scan says `ScanCapped { of: None }`). The mutant that keeps the walk's look but drops its stop is killed (25 results). The round-4 shape, a post-walk flush, survives this test: the queue there was filled under the same per-entry looks, so on an injected clock it evaluates the same pointers, only later in wall-clock time. That ordering is what 02's test proves. 02 `a_pointer_read_already_keeps_its_place_in_the_walk` (bundles `needle`, `plain`, `catalog`; `catalog/index.md` reads `plain/a.md`, a pointer, under a name without the term; through `needle` the results are `plain/a.md, plain/z.md`, and `k = 1` keeps `plain/a.md`); the round-4 post-walk flush is killed (`z.md, a.md`). 03 keeper-ported `okf::index::tests::a_long_heading_is_never_copied_per_entry` (20 000 one-line entries under a 512 KiB heading: every entry's section points into the listing's own bytes); the mutant that copies the heading into each entry is killed on entry 0.
- **As built (restack onto `c57046b8`, 2026-10-09).** Restacked onto rung 3's final round (94.4's helper R203/R214/R215, R226, consolidate's and curate's rulings through R271 and R273 below it). Conflicts, each a union of both sides: `deferred-work.md` (both sides' DWs in number order), the decisions file (every `**R…**` line where it sat; R273 recorded, 95.3's restack line names it), `docs/agents.md` (the tier table's T0 row and the helper's reads gain `drive_search`), keeper-agent `agent.rs` (rung 1's `agent_offer` builds the offer now, and appends `search::spec()` where `[tools].allow` serves it; `REVIEW_READS` 8 beside `REVIEW_BUDGET_SPENT`), keeper-core `tier.rs` (`AgentTool::ALL` 32) and keeper-ported `lib.rs` (`okf` beside `openclaw`). The search code and keeper-sync's `bots_fs` are cd9bfb71's unchanged.
  - **R209 verdict 1, the helper's offer (R280):** keeper-core `agents::helper::TOOLS` holds `DRIVE_SEARCH`; keeper-agent `helper::Reads` answers it through `Parent::search(wire, label)`, the session's own `AllowedTools::search_drives` — its `[tools].allow`, tier (T0), audit row and per-drive `grant_read`, never parked — made by the session's label joined with what the helper has read so far, so a helper that read a `local_only` file no longer sends its query to a remote embeddings model; the hits' reads join the helper's step and reach the session at its `tool_call` line. Proof: `agent_turns::helpers::a_helpers_drive_search_reads_only_what_the_session_may` (tgdrive and private found, neuradrive in scope but not granted refused, nothing of it found or in the label) and `a_helpers_search_after_a_local_only_read_never_embeds_remotely`; mutants dropped from the offer, refused as no read, grant check skipped, reads not in the step, made by the session's label alone — all killed (`mut-95srch-rs-1.log`, `-2.log`, rerun on the final text `-3.log`).
  - **Interactions (R281, R282):** search writes nothing (R211, `drive_search_writes_nothing`), so it cannot race consolidate's or curate's `commit_paths` into a write; and its document candidates are `*.md` only (`Plan::admits`), so a commit's `.keeper.<request>-<n>.tmp`, `.keeper-displaced-…` and `.keeper-taken-…` names are never read as documents, whether scanned or reached through a listing's link. Listings and configuration have their own admission, and no consistent commit snapshot is promised (DW-1075): `search::tests::a_commits_transient_files_are_never_searched` (mutant: the `.md` test dropped — killed, `mut-95srch-rs-final.log` — the kill proves the extension check on document candidates only). Reads stay admitted where asked and where they land on the new keeper-sync (`search_landing` over rung 2's `browse::resolve`): round 1's R95S-01/02 mutants rerun killed (`mut-95srch-rs-r1.log`).
  - **Mutation rerun:** every round's script on the restacked tree (`mut-95srch-rs-r1.log`, `-r2`, `-r3`, `-r5a`, `-r5b`, `-r6`; driver `mut-95srch-rs-drive.py`), and the rest of round 1 in `mut-95srch-rs-r1b.py` (`mut-95srch-rs-r1b-a.log`, `-b.log`): its mutants whose code later rounds moved re-pointed at it and "AC10 an index is created" made to build, all killed but one new variant — a link inside the drive in the index's place taken for no index — killed by a case added to `a_path_is_admitted_where_it_lands_as_well_as_where_it_is_asked` (`mut-95srch-rs-r1c.log`). R95S2-02's three early returns at a capped read survived the rerun (since R236 a capped read is caught at the next candidate, so only the last one decides): `ranked_notes_and_listings_stop_at_the_calls_bytes_and_say_so` caps the last candidate — a drive with no OKF config, a single bundle's listing, a listing's last document — and all three are killed, as is round 1's "T0 row", which did not build in that run (`mut-95srch-rs-final.log`). `grouped`'s thousands survives: a wording mutant, whose pin round 3 removed (R95S3-06) and which is not put back. R95S-03's handle-identity mutant survives as before (DW-716); round 5's post-walk flush survives its one test and is killed by the other, as recorded.

### 95.5 — From session to knowledge (harvest + the promote panel)

**Intent:** "The data from sessions can be used after to update the knowledge in the main drive (or drives)". **Rung:** **epic95-knowledge**. AD-404 (with Q1–Q3); AD-391 (the promote sink); FR-808; the phase-7 FR-243 and FR-244; UX-DR137.

**Files:**
- `keeper-core/src/agents/knowledge.rs` (new, pure):
  - **The candidate note's frontmatter:**
    - `type`: a type from the drive's registry, else `Note` (`tgdrive:.okf/registry/types.md:39`);
    - `title` and `description`;
    - `sources: [{id, resource, title}]`, where `resource` is a drive-relative path inside the source session, or `log/<chunk>#<line id>`;
    - `generated: {by: "agent:<agent>@<host>", at}`;
    - `human_reviewed: false`;
    - `status: draft`.
  - **Validation.**
  - **The tick:** `human_reviewed: true`, plus a `verified` entry `human:<localpart>`, through `Frontmatter::set_in`.
- `keeper-agent`:
  - 90.2's `session_write` gains one rule for `artifacts/knowledge/**`: the host stamps `generated` and `human_reviewed: false` over whatever the model wrote, refuses `verified` and any `human:`/`user:` actor, checks that `sources` resolve, and refuses a note over 64 KiB ("a knowledge note holds at most 64 KiB", R28 S-32), so a person reviewing it can read all of it;
  - 92.5's per-close harvest brief asks for candidate notes, and its turn may now write them.
- `keeper-core/src/sessions/promote.rs` (existing) gains `promote_panel(...) -> PromotePanelVm`:
  - each table row (`parse`, `:61`) with its state: `ok`, `stale` (the source's content differs and is newer by commit time or, for `workspace/`, mtime), `missing source` (quiet), `missing target` (loud), or `unreadable` (verbatim, `:34-41`);
  - unlisted `workspace/` files;
  - `artifacts/knowledge/**` notes with their review state;
  - rows whose target is outside the session (`10-notes/…`).
- `keeper-agent` (the session runtime):
  - `promote_in` (FR-243): copy under the sync engine's stability gate, then `upsert_row` (`promote.rs:136`);
  - `promote_out` (FR-808): copy an artifact into the same drive's notes vault through `WriteScope::create` (`files_write.rs:464-466`), after `check_sink`, then record the row.
- The shell:
  - `sessions_promote_panel({rootId, sessionId})`;
  - `sessions_promote({rootId, sessionId, source, target, note, expected})` — `expected`, the SHA-256 of the version the person read, makes promoting a harvested note that person's review of it (R212);
  - `sessions_knowledge_review({rootId, sessionId, path, reviewed})`;
  - registration in `lib.rs`.
- The front:
  - `src/components/sessions/promote-panel.tsx` (new), opened from the session detail;
  - the archive dialog's per-row review uses it (`session-actions.tsx:61-62`);
  - client wrappers, `dev/mock-shell.ts`, `promote-panel.test.tsx`.
- `docs/sessions.md`: § *The promote panel* replaces the item in *What is not here yet* (`:1070-1073`), which keeps unread marks, history, capture and the current session (DW-391).
- `docs/agents.md` § *Knowledge from sessions*, which says what *reviewed by a person* proves: that the tick was committed by a reader's keeper, not that the person typed it (R28 S-31).

**Acceptance:**
1. **A harvested note never claims a person.**
   - **The writes:** in a steward's harvest session (92.5), the turn writes `artifacts/knowledge/<source-slug>/<note>.md` through `session_write`. Two separate attempts (R212, correcting R-NEW-1):
     - **A forged generator:** a fake model writes `generated: {by: human:tgorka}` and `human_reviewed: true`; the note is stored with `generated.by` `agent:tola-grey@electra`, `human_reviewed: false`, and no `verified` key.
     - **An attempted review:** a write carrying `verified` or `verified_by` — in any spelling a YAML reader takes as that key, or one keeper cannot read the meaning of — is refused, and nothing is written.
   - **Bad sources:** a `sources[].resource` outside the named source session, or a source list keeper cannot read whole, is refused.
   - **The size cap** (S-32): a note of exactly 64 KiB is stored; one byte more is refused with the sentence and nothing is written.
   - **Test:** `a_harvested_note_never_claims_a_person` (keeper-agent).
2. **A harvest never writes the closed session** (Q2).
   - A harvest turn's `session_write` aimed at the source session is refused ("a session is written only by its own agent"). The source session's files and log are byte-identical after the harvest.
   - **Test:** `harvest_never_writes_the_closed_session`.
3. **keeper reads a harvested note as unreviewed** (Q3).
   - `okf_facts` on it: generated by `an agent`, no reviewer, `human_reviewed` false (R212: the structured facts, not the sentence's wording).
   - 89.4's label wrapper gives it `agent` integrity.
   - After the person's tick, both say reviewed by a person.
   - **Test:** `keeper_reads_a_harvested_note_as_unreviewed`.
4. **The panel tells the truth** (FR-244).
   - **The fixture:** a real session folder with a five-row `## Promote` table:
     - one row in order;
     - one whose `workspace/` source is newer than its target;
     - one whose source was cleaned up;
     - one whose target is missing;
     - one unreadable line;
     - plus two unlisted `workspace/` files.
   - **Expected:** `ok`, `stale`, quiet `missing source`, loud `missing target`, and the line shown verbatim; the two files listed as promotable.
   - **The real rename:** an artifact renamed in the tree, whose table cell 52's rename rewrote (`docs/sessions.md:503-504`), keeps its row under the new name.
   - **Tests:** `promote_panel_rows` (pure over the fixture), `a_renamed_artifact_keeps_its_row`.
5. **Promoting in records one row** (FR-243).
   - **The happy path:** promoting `workspace/draft.md` to `artifacts/report.md` copies it byte for byte and upserts one row, and every other byte of `README.md` is unchanged (NFR-39).
   - **Re-promotion:** overwrites the target and leaves one row.
   - **Still being written:** a source written within the stability window is refused with "still being written; try again in a moment".
   - **Test:** `promote_copies_and_records_one_row` (real files and mtimes).
6. **Promoting out respects the label** (FR-808; AD-391; NFR-115's *promote* sink).
   - **Into the vault:** a harvested note promoted into `10-notes/knowledge/<note>.md` in the same drive is copied with its frontmatter — as the version the person read, with their review in the copy (R212) — and the row `| artifacts/knowledge/<…>.md | 10-notes/knowledge/<note>.md | knowledge |` is recorded.
   - **Outside any vault:** a target is refused with "keeper creates files only inside a notes vault" (DW-390).
   - **Another drive:** a target is refused.
   - **A wider audience:** a note whose session label readers do not include the target drive's readers is refused with the reason. For example, a session label `{tgorka}`, narrowed by what the session read, against a target vault whose drive's readers are `{tgorka, marta}`. The test drives the promote path with that label, because the process-per-principal mount rule (AD-377) makes the cross-drive read itself hard to stage. A label, a pinned audience or a log frontier that cannot be established refuses as well (R212).
   - **Tests:** `promote_out_into_the_vault`, `promoting_out_respects_the_label`.
7. **Only a person marks a note reviewed.**
   - **The tick:** promoting a harvested note to notes is the person's "Reviewed by me" (R212): the vault copy gets `human_reviewed: true` and `verified: - by: human:tgorka`, `at: …`, every other byte as the candidate has it; the candidate is never touched.
   - **The untick, and the tick again:** on the vault copy, guarded — it removes that person's entry and sets `false`, or puts it back, keeping an edit that landed meanwhile.
   - **An agent:** cannot do either (#1).
   - **Stale after promotion:** a candidate the steward edits after promotion shows `stale` on its row.
   - **Test:** `only_a_person_marks_a_note_reviewed`.
8. **The panel, on screen** (UX-DR137).
   - **Vitest, through the mock shell:** every row state, the review tick, the target picker limited to the drive's vault folders, the refusal sentences shown in place, and a 64 KiB knowledge note opened whole, with no truncation (S-32).
   - **A real browser** (house rule): the panel for the fixture session.
   - **On hesperia:** a real-WKWebView proof is owed.
   - **Test:** `promote-panel.test.tsx`.

**Operator-verified:**
- [ ] On hesperia, after a delegated session in tgdrive closes, Dr Tola Grey's harvest session shows a candidate note in the panel. Ticking and promoting it into `10-notes/` produces a note whose provenance line in a ⌘9 tool read says "reviewed by a person (human:tgorka)".

**Shell crate:** yes — the three commands and their registration, awaiting CI's macOS job and `check:rust:macos` on hesperia. The panel is front-end. Everything that decides is in keeper-core and keeper-agent.

**binds:** FR-808, FR-243, FR-244, NFR-115 (the promote sink), AD-404, AD-391, UX-DR137

### As built (rung `agents-95-knowledge`, 2026-10-06) — 95.5, the Rust half

Rulings R138 (Q16), R139 (Q17), R140 (Q18). Symbols:
- `keeper-core/src/agents/knowledge.rs` (pure): `stamp` (host-stamped `generated: {by: agent:<agent>@<host>, at}`, `human_reviewed: false`, registry type else `Note`, `status: draft` when absent; refuses `verified`/`verified_by`, a `human:`/`user:` source author, a source outside the closed session, > 64 KiB as stored), `review` (the tick/untick), `registry_types`, `source_slug`, `agent_actor`, `signer_of`, `reviewer`.
- `keeper-core/src/notes/okf.rs`: `write_verified`, the canonical `verified` writer (block list of `{by, at}`, `verified_by` removed), read back by `okf::read`.
- `keeper-core/src/sessions/promote.rs`: `promote_panel` → `SessionPromoteVm` (`PromoteRowVm`, `PromoteState`, `KnowledgeNoteVm`, ts-rs), `fact_of`, `target_in_session`.
- `keeper-agent`: `sessions::write::session_write_with`'s compose is fallible; `cards::CardTools::harvested` is the `artifacts/knowledge/**` rule at the landing (`signer`, `drive_root` on `CardTools`); `promote::{panel, promote_in, promote_out, review}` (stability: settle window on the source's `FileSample` + `read_verified`; vault writes through `VaultWriter` and `WriteScope::create`/`route`; label through `check_sink(Sink::DriveWrite)`).
- Seed: `steward-menu.toml`'s `HV` prompt writes candidate notes.
- Shell (by inspection): `sessions_promote_panel`, `sessions_promote`, `sessions_knowledge_review` + `#[cfg(not(desktop))]` twins, registered in `lib.rs`; `agent_ports::drive::NotesVaultWriter` made crate-visible.
- TS: `src/lib/ipc/gen/{SessionPromoteVm,PromoteRowVm,PromoteState,KnowledgeNoteVm}.ts`, `client.ts` (`sessionsPromotePanel`, `sessionsPromote`, `sessionsKnowledgeReview`), `dev/mock-shell.ts` (`PROMOTE_PANEL`, every state, a 64 KiB note).

Per acceptance item:
1. `a_harvested_note_never_claims_a_person` (keeper-agent `cards.rs`, through `session_write`) + `the_host_stamps_what_a_model_cannot_be_trusted_to_say` (core). Reading: `generated` and `human_reviewed` are stamped over; a `verified` write is refused, not stripped (R-NEW-1 asked).
2. `harvest_never_writes_the_closed_session` (`..`, a link, the drive-root spelling; closed session byte-identical).
3. `keeper_reads_a_harvested_note_as_unreviewed` (core; label wrapper's fact is `human_reviewed: false`, §3 row 30).
4. `promote_panel_rows`, `a_renamed_artifact_keeps_its_row` (core).
5. `promote_copies_and_records_one_row` (keeper-agent, real files and mtimes).
6. `promote_out_into_the_vault`, `promoting_out_respects_the_label` (keeper-agent, fake `VaultWriter`). The outside-vault refusal is `WriteRefusal::OutsideVault`'s existing sentence ("keeper creates new files only inside the vault it manages").
7. `only_a_person_marks_a_note_reviewed` (core) + the vault-copy tick in `promote_out_into_the_vault`; stale-after-promotion in `promote_panel_rows`.
8. Rung `agents-95-panel` (vitest, browser, WKWebView owed).

Deferred: DW-730…DW-734 (DW-731…DW-734 closed by the review fixes below).

### As built (rung `agents-95-knowledge`, 2026-10-06) — review fixes (R95K-01…18, R212)

Ruling R212 (`review-95know.md`): R-NEW-1 accepted with the AC correction above (#1); R-NEW-2 rejected. Per finding — symbol, then the test that fails without it:
- **01/02/03** — `keeper_agent::promote::audience` (+ `Audience::refusal`): no `agent.toml` is a person's session, one that does not read refuses; `_drive.toml` checked against this device's pin (`mount::pin_matches`; `OutOf.pin` is `Result<Option<&DrivePin>, String>`), unpinned / differing / unreadable refuses; an agent's label read from its whole log (`log::reader::read_session`, a skipped or torn line or a conflict refuses), never the bounded index refresh; `promote_out` refuses when the log's frontier moved while the source was read. The shell only calls it (`sessions_ipc::drive_pin`, `reviewing_person`); the panel's `out_refused` is the same refusal. Tests: `an_audience_that_cannot_be_established_refuses`, `a_label_past_the_index_s_reach_still_binds`, `promoting_out_respects_the_label`.
- **04** — `promote::lands_as_named` (keeper-sync `browse::landing` must equal the typed path) on promotion and tick. Test: `a_link_in_the_vault_never_carries_a_promotion` (a link out of the drive, a link into an agent home).
- **05** — `PlanStep::CopyChecked {from, to, sha256}` (executor: stage beside the target, hash, rename or refuse); `promote_in` hashes the settled source with `verify_while_reading` and re-samples it; `promote_out` publishes the `read_verified` bytes. Test: `a_checked_copy_copies_only_what_was_read`.
- **06** — `promote_out(…, &Request {expected}, Some(&Reviewer), …)`: a harvested note needs the revision the person read (`KnowledgeNoteVm.revision`) and a person, and its vault copy is written already reviewed (`knowledge::review`); drift refused. Test: `a_harvested_note_is_promoted_only_as_it_was_read`, `promote_out_into_the_vault`.
- **07** — `promote::record_then`: the row first, in the same held zone and journaled plan as the copy; a README edited meanwhile is re-read and re-spliced (3 attempts). Tests: `a_failed_copy_leaves_a_row_promoting_again_finishes`, `a_failed_publication_leaves_a_row_promoting_again_finishes`.
- **08/16** — `VaultWriter::amend` (shell: `notes_ipc::amend_block`, by inspection) for every tick/untick; `knowledge::review` canonicalizes a legacy `verified: true` + `verified_by` on a write. Tests: `a_tick_keeps_an_edit_that_landed_meanwhile`, `a_legacy_review_is_written_canonical`.
- **09** — `Frontmatter::unclaimed_lines` + `knowledge::plain_key`: an explicit `? key`, a flow map, a merge key, an anchor or an escaped key refuses (`KnowledgeRefusal::Unreadable`); unknown nameable keys kept. Test: `a_review_keeper_cannot_name_is_refused`.
- **10** — `okf::strict_sources` (`KnowledgeRefusal::SourcesUnreadable`); `CardTools::harvested` resolves each source by `browse::landing` with no link. Tests: `every_source_is_read_or_the_note_is_refused`, `a_harvested_note_never_claims_a_person` (the intra-zone link).
- **11** — `FileFact.changed_ms`: `promote::committed_ms` (keeper-sync `git::history::{file_log, blob_at}`, the oldest of the newest commits holding today's digest, review keys aside), mtime for `workspace/` and uncommitted content. Test: `staleness_is_by_what_changed_not_by_mtime`.
- **12/13** — `PanelFacts.files: Result<FileFact, String>`, `KnowledgeFile`, `PanelFacts.problems`; `PromoteState::Unknown`, `PromoteRowVm.problem`, `KnowledgeNoteVm.{revision, reviewed_by_me, problem}`, `SessionPromoteVm.problems`; only row cells are read, streamed (`promote::digest_of`, a 64 KiB head), notes up to 64 KiB. Tests: `what_could_not_be_read_is_said_not_absent`, `a_digest_streams_and_ignores_only_review_keys` (core), `what_the_panel_cannot_read_it_says` (agent).
- **14** — `promote::upsert_row` → `Result<_, RowRefusal>` (`NoTable`, `Unrepresentable`), checked before any effect. Tests: `a_row_is_written_only_as_it_reads_back`, `a_row_the_table_cannot_hold_is_refused_before_any_copy`.
- **15** — `refs::promote_edits` rewrites a target only in the session frame. Test: `a_rename_in_the_session_never_moves_a_drive_target`.
- **17** — `dev/mock-shell.ts`: `PromoteFixture` per root and session (bytes, changed times, unreadable files, vault copies with reviewers, the label chip, `outRefused`, `problems`; test seam `stagePromoteFixture(rootId, sessionId, stage?)`), `promotePanel` derives the VM as Rust does (row states, a note's `revision`, its `problem` when unreadable or over 64 KiB, reviewers of a readable copy only), `sessions_promote` refuses as `promote_out` does (unrepresentable cell, not an artifact, outside the vault, a name taken by no row of this source, no or a drifted `expected`) and reviews a harvested note it publishes, a re-promotion resetting the copy's reviewers to the promoter, `sync_read_text` serves the fixture's exact bytes (the 64 KiB ledger). No TS test pins the fixture; the panel rung's browser proof exercises it.
- **18** — the sentence pins in `keeper_reads_a_harvested_note_as_unreviewed` and the cards/promote tests replaced by structured facts and refusal kinds; the still-writing, size-cap and outside-vault sentences stay (the ACs quote them).

Deferred: DW-735…DW-739.

### As built (rung `agents-95-knowledge`, 2026-10-07) — re-review fixes (round 2: R95K2-01…10, R219)

Ruling R219 (`review-95know-2.md`). Per finding — symbol, then the test that fails without it:
- **01** — keeper-sync `browse::resolve_known` (absence only on `NotFound`, a dangling link refused, an unsearchable folder `Unreadable`) behind `zone::read_text` (the `_drive.toml` reader) and `promote::locate`; `log::reader::read_session` reports a `log/` it cannot look at and an unreadable listing entry; `promote::frontier` → `Result`, an unreadable log refusing. Tests: `a_declaration_that_cannot_be_looked_at_is_not_an_absence`, `a_log_that_will_not_list_has_no_frontier` (agent), `a_log_that_cannot_be_looked_at_is_not_an_empty_log` (core).
- **02** — `sessions::exec::{stage_beside, publish_stage, write_durable}`: the stage created exclusively after whatever was at its name is removed, `copy_checked` copies/hashes through the handle (`plan::sha256_hex_of`), the rename only of that file; the README's write too. Test: `a_planted_stage_is_never_written_through` (symlink and hard link).
- **03** — `promote::{Pending, PENDING_REL, keep_pending, finish_pending, publish}`: `.keeper/promote-out.json` kept before the row and the copy, cleared after both; finished with the recorded version or refused with why by the next `promote_out`/`review`. Test: `an_interrupted_promotion_out_publishes_the_version_it_reviewed` (failed write + drift; before the row + removal; after the copy; another file at the target).
- **04** — shell, by inspection: `notes_vault::write_note_if` + `file_write` (striped per-(vault, path) lock taken by `write_vault_file`/`write_note`) at `src-tauri/crates/keeper/src/notes_vault.rs:2401-2470`; `notes_ipc::amend_block` publishes through it at `src-tauri/crates/keeper/src/notes_ipc.rs:4854`; `agent_ports.rs:261-266` documents the seam. Test (macOS only): `two_amendments_of_a_closed_note_both_land`.
- **05** — `okf::{scalar, unescape_double}`: every YAML 1.2 double-quoted escape decoded, an undefined escape or a bare `"` refused by `strict_sources`. Tests: `an_escaped_author_is_read_as_yaml_reads_it` (core), `a_harvested_note_never_claims_a_person` (the real `session_write`, stored bytes unchanged).
- **06** — `FileFact.changed_ms: Option<i64>`, `fact_of(…, Option<i64>)`; `promote::{History, history_of}` (16 commits all holding the digest → unknown) and `file_fact(…, reviewed_here)` (a vault copy dated by commits only); `row_vm` → `unknown` with why. Tests: `review_commits_past_the_history_read_never_date_the_copy`, `a_tick_before_the_first_commit_never_dates_the_copy` (agent), `an_unknown_content_time_is_never_read_as_in_order` (core).
- **07** — the panel's row cells through `resolve_known`. Test: `a_row_file_behind_a_closed_folder_is_unknown_not_missing` (source, session target, drive target; a genuinely absent target stays `missingTarget`).
- **08** — `promote::upsert_row` breaks the line before a row appended at a table ending the file. Test: `a_row_appended_where_the_file_ends_reads_back` (empty table, table with a row).
- **09** — `okf::BlockEntry.listed`: a flow map or `-`-less map is `VerifiedShape::Simplified`, so `knowledge::review` writes the block list. Test: `a_single_map_review_is_written_as_a_list`.
- **10** — `dev/mock-shell.ts`: `promoteKey(rootId, sessionId)` shared by `promoteFixture`, `stagePromoteFixture` and `promoteFileText(profileId, subpath)` (`sync_read_text` passes `payload.id`); vault reads from that root's fixtures. Test: `src/test/mock-shell-promote-reads.test.ts`.

Deferred: DW-835…DW-839.

### As built (rung `agents-95-knowledge`, 2026-10-07) — re-review fixes (round 3: R95K3-01…08, R235)

Ruling R235 (`review-95know-3.md`). Per finding — symbol, then the test that fails without it:
- **01** — keeper-sync `browse::resolve_known` + `absent_root`: a root that is not a folder, a link to nothing, or under a file or a dangling link is `Unreadable`; absent only when the nearest existing ancestor is a folder. Tests: `a_root_the_disk_cannot_vouch_for_is_no_absence` (sync), `an_agents_folder_the_disk_cannot_vouch_for_is_no_absence` (agent: dangling root, file root refuse; absent root, absent declaration promote).
- **02** — `promote::finish_pending` re-derives `vault_relative` through `WriteScope::create` under the live subfolder and refuses drift; `publish(…, subfolder, …)` → `VaultWriter::write(profile, subfolder, rel, text)`, which the shell's `NotesVaultWriter` refuses when `vault.config.subfolder` differs (`agent_ports.rs`). Test: `a_moved_vault_is_never_written_by_an_interrupted_promotion` (vault moved to `20-notes`, another's file there kept).
- **03** — `promote::publish`: only `NotFound`, identical bytes, or a `replaces` target reach the writer; any other read error refuses. Test: `an_unreadable_target_is_never_written_by_an_interrupted_promotion`.
- **04** — `promote::{Unearned, UNEARNED_REL, unearned, mark_unearned, retire}`, `Pending.session_id`; `promote_out`'s `repromoted` excludes unearned rows; a successful `publish` unmarks. Test: `an_interrupted_promotion_out_publishes_the_version_it_reviewed` (collision case extended: the same source→target retried is refused, foreign bytes kept; after the file goes it publishes and re-promotion replaces).
- **05** — `finish_pending` checks `<session>/README.md` through `resolve_known`; not there → terminal refusal through `retire`. Test: `a_session_moved_away_never_blocks_the_zone` (archive and delete, then another session's promotion and review).
- **06** — `promote::set_aside`: a record that does not decode refuses with the error, moved to `.keeper/promote-out.<ulid>.unreadable.json`; when the move fails it stays and every promotion refuses. Test: `an_unreadable_promotion_record_is_refused_and_kept`.
- **07** — `VaultWriter::write` is `Ok` only once durable; shell `notes_vault::write_vault_file_durable` (`write_atomic(…, Durability::Durable)`: temp synced, renamed, folder synced) behind `NotesVaultWriter::write`; `publish` writes an already-identical target again rather than clearing. Test: `a_copy_not_yet_durable_keeps_its_promotion` (fault-injected flush failure, written now and already there). The shell's sync itself by inspection.
- **08** — shell `notes_vault::CHECKED` (`#[cfg(test)]` hook between `write_note_if`'s check and write); `notes_ipc::tests::two_amendments_of_a_closed_note_both_land` runs the competing `amend_block` on another thread inside that window, with no editor (`BodySub`) open: with `file_write` held it waits and recomposes; without it, it lands inside the window and the outer write drops its `project: taxes`, which the test asserts. By inspection only — the shell crate does not build on Linux; run and its lock-removal mutant await CI's macOS job (hesperia had 16 GB free on 2026-10-08, under the 30 GB a check needs).

Deferred: DW-920…DW-922.

### As built (rung `agents-95-knowledge`, 2026-10-08) — re-review fixes (round 4: R95K4-01…08, R244)

Ruling R244 (`review-95know-4.md`). Per finding — symbol, then the test that fails without it (mutants in `/tmp/agents-salvage/mut-95know-6.py`):
- **01** — keeper-sync `browse::resolve_known` asks about the root in its plain spelling (`root.components().collect()`) before `absent_root` decides absence, so `80-agents/`, `./80-agents` and `80-agents//` answer as `80-agents`. Tests: `a_root_the_disk_cannot_vouch_for_is_no_absence` (sync; every case in four spellings), `an_agents_folder_the_disk_cannot_vouch_for_is_no_absence` (agent; the profile's subfolder spelled `80-agents`, `80-agents/`, `./80-agents`).
- **02/03/04** — one mechanism: the README row's optional fourth cell (`PromoteRow::Entry::published`, `promote::upsert_published_row`, `render_row(…, published)`, `split_row` reads a fourth cell only as a digest) records the `digest_of` of the copy the source published, written after the copy is durable (`publish` → `record_then(…, Some(digest))`); `upsert_row` keeps it while the row names the same target and drops it for another; `refs::promote_edits` carries it through a rename. `promote::its_copy` — the synced row and the synced file agreeing — is the only authority: `promote_out`'s admission, `publish` (recovery included) and `review` (inside the guarded amend, as the copy is at the write) proceed over an existing file only when it is that copy; `promote_panel` takes a copy's reviewers only then. `Pending.replaces`, `Unearned`, `UNEARNED_REL`, `unearned`, `mark_unearned` and `retire` are deleted. Tests: `a_row_no_copy_stands_behind_grants_nothing_on_another_mac` (02: refused here or not yet tried, then a synced copy without `.keeper/`), `a_set_aside_record_leaves_its_row_without_authority` (03: garbled and missing-`session_id` records), `a_review_never_writes_into_a_file_the_note_did_not_publish` (04: tick and untick refused, foreign bytes kept, no review shown; then publication and its review out and in), `a_tick_keeps_an_edit_that_landed_meanwhile` (a body edit landing in the amend window is kept and the review refused; a review landing there is kept beside the tick), `promote_panel_rows`, `a_published_copy_is_recorded_kept_and_dropped`, `a_rename_in_the_session_never_moves_a_drive_target` (core), `promote_out_into_the_vault`.
  `dev/mock-shell.ts` decides as Rust does: `PromoteRowFixture.published` (the copy, review keys aside), set by a promotion out; a re-promotion or a review over a copy that is not it is refused, and the panel shows no review from it (`mock-shell-promote-reads.test.ts` still passes).
- **05** — `finish_pending` reads the session at the recorded path through `scan::recorded_id` (fallible; `None` for a record that names no id) and refuses terminally, the record cleared and nothing written into that session, unless it is `Pending.session_id`; a record that cannot be read keeps the promotion. Tests: `a_session_at_a_reused_path_is_never_finished_into` (archive and delete, target free and occupied), `an_unreadable_session_record_keeps_its_promotion`.
- **06** — keeper-agent `sessions::exec::make_dirs_within(root, dir)` syncs every entry from `root`'s child down to `dir` on every call; the shell's `notes_vault::write_held_as` uses it for `Durability::Durable` (by inspection), so `write_vault_file_durable` is `Ok` only once every folder on the way is on the disk, and a failure keeps the pending record (`a_copy_not_yet_durable_keeps_its_promotion`). Test: `folders_a_durable_write_makes_are_synced_up_to_its_root` (vault, then the new middle folder, unsyncable; the retry still refused).
- **07** — `set_aside(zone, text, why)` writes the evidence copy with `exec::write_durable` before removing the active record; a copy whose sync failed is removed again and the record stays, so every promotion and review is refused until it can be set aside. Test: `a_record_is_set_aside_only_once_its_copy_is_durable` (`.keeper/` at 0300: rename succeeds, directory sync fails).
- **08** — shell `#[cfg(test)] notes_vault::WAITING`, run by `file_write` when its `try_lock` would block; `notes_ipc::tests::two_amendments_of_a_closed_note_both_land` waits for the competitor through a 60 s watchdog that fails the test, goes on only once it is seen at the lock (or, without the lock, landed) and asserts it was at the lock. By inspection only; its run and the lock-removal mutant await CI's macOS job.

Deferred: DW-960 (a vault copy a person edited is no longer the note's), DW-961 (the journal executor's `make_dirs` resume).

### As built (rung `agents-95-knowledge`, 2026-10-08) — re-review fixes (round 5: R95K5-01…08, R252, R253)

Rulings R252 (a person-edited copy loses the note's authority; pre-R244 rows grant nothing) and R253 (`review-95know-5.md`, all eight accepted). Per finding — symbol, then the test that fails without it (mutants in `/tmp/agents-salvage/mut-95know-7.py`, logs `mut-95know-7-*.log`; the mutants that first run did not bring to an assertion — the two core digest builds killed for memory, a mutant that did not type-check — rerun by `mut-95know-7b.py`, logs `mut-95know-7b-*.log`):
- **01/02** — keeper-core `sessions::promote::copy_digest(source, bytes)` is the row's fourth cell: for a harvested note (`knowledge::is_note(source)`), SHA-256 over a tag, the length-framed byte order mark and frontmatter interior once the review keys' lines are out (`without_reviews`, `Frontmatter::inner_text`; whitespace-only is none), then the body — the whole text, never a head, never the target's name; any other source's copy byte for byte. `digest_of` is "differs" only and no longer returns reviewers (`FileFact::reviewers` gone); the panel's reviewers come from `copy_fact` (`CopyFact`, `PanelFacts::copies`), which keeper-agent reads whole up to `COPY_BYTES` (DW-962). Tests: `a_copy_digest_binds_where_the_frontmatter_ends_and_no_review_moves_it` (core: the review-only-block collision pair, a note's metadata pushed into a second block, tick → untick → another's tick across `HEAD_BYTES`, no frontmatter, BOM, empty block), `a_copy_whose_frontmatter_moved_is_not_the_notes` (agent: published copy → boundary-moving edit → re-promotion and review refused, edit kept), `a_review_never_unseats_its_own_copy` (agent: `.txt` target and a frontmatter the tick carries past the head; untick, tick, panel, then re-promotion of the changed note).
- **03/06** — `promote::standing(source, published, there)` is the only authority: `promote_out` replaces an occupied target only when the source's row names it and its digest matches (no equality bypass); `publish` accepts its own pending bytes only as the finish of an operation admitted onto an absent target or a standing copy (DW-963 names the remaining window). `dev/mock-shell.ts` `standing`/`copyLoss` decide the same. Tests: `identical_bytes_never_adopt_a_file` (no row, three-cell row, quarantined operation, another session's synced copy; then a changed-source retry; file and README kept); `mock-shell-promote-reads.test.ts` "never adopts a vault file that holds the unreviewed note's exact bytes".
- **04** — keeper-agent `sessions::exec::make_dirs_within(drive, vault, dir)` syncs every entry from the drive's child down (from the vault's root when it lies outside the drive); the shell's `notes_vault::write_held_as` passes `vault.local_path`, `vault.root` (by inspection), so a vault root that registration made, and its configured parents, are on the disk before the pending record goes. Test: `folders_a_durable_write_makes_are_synced_from_the_drive_down` (absent nested vault root; drive unsyncable, then retry after partial creation; each configured folder unsyncable).
- **05** — `promote::CopyLoss` (`Unrecorded`, `Changed`) and `CopyLoss::explain(source, target)`: `promote_out` refuses with it when the source's row names the occupied target (a name taken otherwise), `review` refuses with it, and `KnowledgeNoteVm::foreign_copy` (TS `foreignCopy`) says it in the panel; each names the next action that keeps the file. Test: `a_copy_that_lost_its_authority_says_why` (person-edited copy: re-promotion, review and panel say `Changed`; three-cell row: `Unrecorded`).
- **07** — `promote::entry_of(table, source)`: the source's first row, the one `upsert_row` updates, gives index, target and digest together to `promote_panel`, keeper-agent's `panel` (copies read only for it), `recorded_copy` (promotion and recovery) and `review`. Test: `a_second_row_lends_the_first_nothing` (first row to `a.md` without receipt, second to `b.md` with one; same content, different reviews; panel and review agree on `a.md` and `Unrecorded`).
- **08** — wording assertions deleted from `promote/tests.rs` (`is not the copy`, `is kept at` → the set-aside file's own name, `could not be set aside`, `cannot be read`, `could not be read`, the vault-outside sentence); the earlier rounds' mutants rerun so each fails at a refusal-kind or state assertion (`08-*` in `mut-95know-7-run.log` and `mut-95know-7b-run.log`, the latter also `08-unreadable-target-as-absent` for `an_unreadable_target_is_never_written_by_an_interrupted_promotion`).

Deferred: DW-962 (the panel reads a copy up to 1 MiB; no cap on a promoted note), DW-963 (identical bytes landing before a pending promotion finishes), DW-964 (the panel UI for `foreignCopy`, rung `agents-95-panel`). Shell edit by inspection; macOS compile and run await CI's macOS job.

### As built (rung `agents-95-knowledge`, 2026-10-08) — re-review fix (round 6: R95K6-01, R261, R262, R263)

Rulings R261 (a pending promotion finishing onto its own pending bytes is that promotion; DW-963's residual), R262 (rows with a `c2f4ecff`-era fourth cell grant nothing; no migration, never shipped) and R263 (`review-95know-6.md`: R95K6-01 fixed in place as the unfinished half of R95K5-02). Symbol, then the tests that fail without it (mutant in `/tmp/agents-salvage/mut-95know-8.py`, logs `mut-95know-8-*.log`):
- **R95K6-01** — keeper-core `notes::frontmatter::Frontmatter::remove_in` keeps an emptied first block (`---\n---\n`) when dropping it would let what follows be read as frontmatter, so `agents::knowledge::review` — its `human_reviewed` rewrite (`remove_all_in` then `set_after_in`) and `okf::write_verified`'s removals — edits only the note's own frontmatter and leaves the body byte for byte; a block-shaped body never becomes metadata, and the copy keeps the receipt its row records. `copy_digest`'s framing and every receipt are unchanged. Tests: `removing_the_last_key_removes_the_empty_block` (a block-shaped body, with and without a byte order mark, keeps the emptied block); `a_copy_digest_binds_where_the_frontmatter_ends_and_no_review_moves_it` (the review cycle tick → untick → another's tick now also over `---\nhuman_reviewed: true\n---\n---\ntitle: T\n---\nBody.\n` and its empty-block variant); `a_review_never_unseats_its_own_copy` (agent: both values promoted, unticked, ticked — after each the copy's body is the note's byte for byte, no foreign copy, the panel's review right — then the changed candidate promoted over the copy).

Deferred: DW-1040 (a review's write does not re-check that the composition keeps the receipt's digest). No shell edit; no TypeScript touched.

- **As built (restack onto `8d8639c1`, 2026-10-09…10).** Restacked onto rung 4's final search round; R266 recorded (converged at e72d3348; R95K7-01…03 deferred as DW-1041…1043, DW-1040 names DW-1041's counterexample). Conflicts: `deferred-work.md` and the decisions file, each a union (DWs in number order, every `**R…**` line where it sat); promote tests gained `SessionAgent.{checkpoints, outputs}` from the workflows rung. **One resolver (R288):** keeper-sync `browse::resolve_known(root, subpath) -> Result<Known, BrowseRefusal>` (`Known::{Landed(Landing), Absent, RootAbsent}`, `Landing::{path, into_path, relative}`) is the one "not there vs not known" resolver; `bots_fs::search_landing` is deleted. It keeps the stricter half of each: absence only on `NotFound` (search's R218 — `ENOTDIR` now refuses for knowledge's readers too, DW-834), a dangling link refused, the root asked about in its plain spelling and positively absent only under a folder (knowledge's R235/R244), the landing under the canonical root and spellable; knowledge's callers (`zone::read_text`, `promote::locate`, `finish_pending`, `review`) read a root that is not there as an absence (`Known::landed`), search's (`search::landing`) refuse it (`Known::under_root`, a drive's checkout that is gone). Tests: `browse::tests::{a_root_the_disk_cannot_vouch_for_is_no_absence, only_not_found_is_an_absence}`, keeper-sync `bots_fs::a_link_put_in_an_offered_entrys_place_is_never_followed`, and both sides' existing landing/absence regressions; per-caller bypass mutants in `mut-95know-rs.py`. **Interactions:** R289 — knowledge makes no commit and calls no `commit_paths`: its writes (the harvest's `session_write`, the session journal, the vault writer, `.keeper/promote-out*.json`) are a session's or a person's working-tree writes, committed by the watcher like a person's (`.keeper/` stays out of git), and a promotion never lands where a maintenance commit writes — `_skills/`, a home's memory files and proposals — wherever the vault is (`WriteScope::with_agents`; `promote::tests::a_promotion_never_lands_where_maintenance_commits`), so curate's sweep and consolidate's guarded commits meet knowledge's files only as a person's edits (R207, R246…R268). R290 — neither the helper (R203) nor a nudge's review pass gains `session_write`, the only agent-side knowledge writer (`helpers::a_helper_cannot_write_send_or_delegate`, `the_review_pass_never_gains_the_session_writer`). R291 — `WriteScope::create` asks only the vault, so a vault at the drive's root let a promotion write `_skills/`, a home or a session's `workspace/`: keeper-sync `WriteScope::fenced(subpath)` is the zones' fences alone (`classify` asks it first), and `promote_out` asks it of the target, `finish_pending` of an interrupted promotion's recorded target (refused, record cleared, nothing written; same test, its second half). Mutation rerun of every round's script plus the restack's own (resolver bypass per caller, `int-create-unfenced`, `int-finish-unfenced`): `mut-95know-rs-run.log`, `mut-95know-rs-*.log`. **Round 8 (R292, `review-95know-8.md`):** (R95K8-01) the fence's zones are read by the profile's own component rule — keeper-sync `files_write::normalise_subfolder` joins `profile::subfolder_components` (now `pub(crate)`: trimmed, `.` and empty parts dropped), so a zone configured `./80-agents`, ` 80-agents `, `./60-sessions` or ` ./60-sessions` is the folder the profile's `agents_root`/`sessions_root` open; `a_promotion_never_lands_where_maintenance_commits` runs every target, fresh and as an interrupted promotion's record, under four zone spellings with the targets in their physical spelling (refused, nothing written, the record cleared; mutant `r8/zone-normalise-old`). (R95K8-02) the shell's `sync_create_entry` creates through `WriteScope::create` without `fenced`: pre-existing and a person's own action, DW-1093. (R95K8-03) `helpers::a_helper_cannot_write_send_or_delegate` no longer pins the refusal's sentence; mutants `r8/int-helper-unknown-not-refused`, `r8/helper-dispatches-session-write` and `r8/review-allows-session-write` (`mut-95know-rs-r8-*.log`). No shell edit (no shell caller of either resolver or of `WriteScope::fenced`; keeper-core and the shell are as rung 5 left them); no TypeScript touched. **Round 9 (R295, `review-95know-9.md`):** (R95K9-01) the write scope reads its vault and zone prefixes as the native path their roots are joined at (keeper-sync `files_write::native_prefix`; a parent, root or prefix component refuses every write), so a Unix vault `notes\.` is that one folder — `promote::tests::a_promotion_lands_only_where_it_was_checked` and `a_recovered_promotion_lands_only_where_it_was_checked` (fresh promotion and recovery to `notes/keep.md` refused, `notes\./keep.md` unchanged), `host::tests::an_agent_hosts_write_lands_only_where_it_was_routed`, `files_write` `a_backslash_in_a_unix_subfolder_is_part_of_the_folders_name` and `a_subfolder_naming_no_folder_in_the_profile_refuses_every_write`, mutants `mut-95know-rs-r9-*.log`; (R95K9-02) DW-1094, (R-NEW-1) DW-1095; no shell or TypeScript edit. Converged at d68890ef (review-95know-10.md, R297).

### As built (rung `agents-95-panel`, 2026-10-06) — 95.5, the UI half

`PromotePanel` in `src/components/sessions/promote-panel.tsx` renders the knowledge rung's VM,
refreshing it after writes and session changes. `PromotionRow` keeps per-file refusals beside
the target; `KnowledgeCard` opens the whole note and records the person's review;
`VaultDestination` browses only beneath the VM's vault through `syncBrowse`. `SessionDetail`
opens the panel in its **Promote…** dialog. `SessionActions` embeds it in the archive dialog,
requires explicit per-row promotion/skip decisions and passes the selected pairs to
`sessionsArchive`; a refusal leaves the dialog open, and a fresh table read invalidates prior
choices. No Rust, generated bindings or IPC signatures changed in this rung.

The frontend tests are colocated in `promote-panel.test.tsx` and use the real typed client over
the stateful mock shell rather than a second set of command mocks:

- **AC4, table truth:** `keeps missing sources unpromotable, repairs missing targets and replaces a stale row once` covers quiet source-gone, loud target-missing, unreadable rows, a newer knowledge note, refreshed repaired rows and the target form following a renamed table cell. The backend's existing rename test still owns rewriting cells; the renderer has no second path map.
- **AC5, promote in:** `keeps a refused workspace file editable in place and promotes an unlisted file to its chosen target` covers the stability refusal, editable target and unlisted-to-recorded transition.
- **AC6, promote out:** `opens all 64 KiB and offers only this drive's vault after a person's review`, `promotes an ordinary artifact without inventing a knowledge review`, and `does not offer a destination outside the label, or hide a failed panel read behind an empty list` cover the vault picker, generic artifacts, unavailable destinations and failed reads.
- **AC7, the person's tick:** `waits for the saved review and preserves it when Rust refuses an untick` covers the mixed prior-review state, explicit own review, rejection and tick/untick. `keeps an unreviewed copy visible when the second write fails and lets the person finish its review` covers DW-733 on this base's two-command contract.
- **AC8, screen and entry:** the whole-note test asserts all 65,536 bytes and the final marker; `opens the promote review from the session detail` exercises the actual entry and full-note open. `requires every archive choice, passes chosen promotions and retains the dialog on refusal` proves the archive gate, selected copy list and failure recovery.

**Visual decisions:** existing shadcn primitives and Keeper tokens only; source/destination
columns at wide widths become stacked paths below 640 px, with wrapping rather than ellipses.
Amber `held` ink means **Newer here**, destructive ink means **Target missing**, and muted ink
means **Source gone**. The session label leads, knowledge has its own group, whole-note text is
a keyboard-scrollable read-only field, and the archive's destructive action stays outside its
scrolling review body. Loading, unavailable, empty, read-error, partial-success and long-path
states have explicit sentences instead of an empty spinner or a silent missing control.

**R139:** this base records the review after copying. The panel requires consent first and
reports a second-write failure honestly; the knowledge rung's reviewed fixes must replace this
sequence with its atomic, revision-bound promotion when the coordinator restacks. Its new
`reviewedByMe` also replaces the base VM's explicitly mixed prior-review state.

**Proof on Linux Chromium (mock shell):** the full app's Sessions → detail → Promote entry
at 1280 px, and the real SessionsPane harness at 420 px. Dialog widths were 768/388 px,
each equal to its scroll width; archive footers stayed inside the dialog while the review body
scrolled. Both opened all 65,536 note bytes including the final marker; only vault folders were
offered; tick/untick, workspace promotion, generic-artifact promotion, audience refusal,
copied-but-unreviewed recovery, empty/loading/read-failure and long-string states were exercised.
ARIA snapshots named all five row states and explicit archive skips/queued targets. Final runs
had no console errors or failed network requests. `/healthz` returned 200. Evidence:
`/tmp/agents-salvage/panel95-browser/{results,edges}.json` and the adjacent ARIA snapshots/PNGs.
Ten mutants in `/tmp/agents-salvage/mut-95panel.py` were killed by assertions; the whole-note
mutant also failed the detail-entry test. Removing the archive reread invalidation failed the
disabled-archive assertion; retaining an old row's target form failed the renamed-target value
assertion. Restoring the exact source passed all nine tests.

The required full `vitest run --maxWorkers=1` run passed 6,570 tests in 393 files; its one failed
suite was the known symlinked-worktree load failure in `setup-code-scanner.test.tsx` (denied
`zxing_reader.wasm?url` outside the worktree), with no test executed in that suite.
After the final row-identity and archive-reread guards, the three affected frontend suites
(`promote-panel`, `session-detail`, `sessions-pane`) passed all 61 tests. `tsc --noEmit` exited 0;
`biome ci .` checked 961 files, exited 0, and reported four unused-suppression warnings in
untouched files. Logs: `/tmp/agents-salvage/gate-95panel-{biome,tsc,vitest-scoped}.log`.
Browser tabs and the dev server were closed, and all throwaway harness/config files were removed.

**Owed:** real WKWebView / harvested-note verification on hesperia (DW-780). The existing phone
stack has no Sessions route (DW-781); narrow component proof is not a claim that phone navigation
ships here. No Rust target directory or cargo run was needed.

### As built (rung `agents-95-panel`, 2026-10-07) — restack onto 7f964245; review fixes R95P-01…10, R216

Restacked onto the knowledge rung's round-2 fixes (7f964245). Rust now decides every offer and
the panel renders it; the base paragraph's "no Rust, bindings or IPC changed" no longer holds.
`keeper_core::sessions::offer` (new: `DestinationVm`, `VaultCopyVm`, `UnlistedVm`,
`ArtifactOfferVm`, `NoteTextVm`, `compose_target`, `row_revision`, `snapshot_revision`,
`refused_in`); `PromoteRowVm.{revision,refused}`, `KnowledgeNoteVm.{copy,destination,unavailable}`,
`SessionPromoteVm.{unlisted: UnlistedVm[], artifacts, revision}`, `ArchiveDecision.before`
(bindings regenerated). `keeper_agent::promote::offer` (new: `complete`, `read_note`,
`promote_to`, `review_as_read`, `archive`), `promote::admit_in` shared by `promote_in` and the
archive, `sessions::verbs::archive_with`. Shell (by inspection): `sessions_archive` (+`revision`,
runs `offer::archive`), `sessions_promote` (into the session only), new `sessions_promote_to` and
`sessions_knowledge_read`, `sessions_knowledge_review` (+`expected`); `lib.rs` handler list.

- **01** archive promotions are the panel's: `an_archive_promotes_as_the_panel_does` (README row
  retargeted, retained bytes), `an_archive_refuses_what_a_promotion_refuses` (`workspace/keep.md`,
  `README.md`, `../elsewhere.md`, a source still being written).
- **02/03** candidate vs vault copy, review as read, missing-copy restore:
  `a_note_is_read_reviewed_and_restored_as_the_version_read`; panel `restores a missing notes copy
  by promoting the note again, never by reviewing an absent file`, `reads the candidate and the
  notes copy separately when they differ`, `reviews only the notes copy as read, and keeps Rust's
  last saved review when a write is refused`.
- **04** `binds consent to the version read: a newer candidate clears it and a late read shows stale`.
- **05/06** `an_archive_refuses_a_checklist_that_changed`, `revisions_move_only_with_what_they_bind`;
  panel `is not ready while a reread is out, keeps choices an unchanged reread confirms and drops
  changed ones` (ends with an archive of the checklist as read), `is not ready while a panel write is out`.
- **07/08** `the_panel_offers_only_what_the_promotion_takes`, `a_target_is_composed_only_inside_the_vault`;
  panel `says why a file is not offered, and what the panel could not see, with nothing to click`.
- **09** `moves focus into each editor and back to its row action`; browser keyboard pass.
- **10** the mock-echo assertions are gone; rendered outcomes only.

The mock shell serves the same VM (and a plain-TypeScript SHA-256 for a dev server reached over
plain http, where `crypto.subtle` is absent). Browser proof at 1280 px (full app) and 420 px (real
SessionsPane harness, DW-781): `/tmp/agents-salvage/panel95-browser-2/`. Deferred: DW-782…785.

### As built (rung `agents-95-panel`, 2026-10-07) — re-review fixes (round 2: R95P2-01…09, R234)

Ruling R234 (`review-95panel-2.md`). Per finding — symbol, then the test that fails without it:
- **01** — `keeper_agent::promote::review(…, expected, …)` (`offer::review_as_read` removed):
  `expected` is `keeper_core::sessions::offer::copy_revision` (the copy's path and bytes), compared
  with each text the guarded amend hands over, in the held zone after `finish_pending`; drift
  refuses with `promote::COPY_CHANGED`. Test: `a_review_lands_only_on_the_copy_as_read` (an
  edit landing inside the amend's retry via `Vault.meanwhile`, a pending publication, a retarget
  to identical bytes).
- **02** — `knowledge::MAX_REVIEWED_BYTES` (64 KiB + 16 KiB): `offer::vault_copy`/`read_note`'s
  copy bound; `promote_out` and `review` refuse a copy that would pass it. Test:
  `a_note_at_the_cap_is_read_and_reviewed_after_promotion` (65,536-byte candidate → promote →
  read copy → untick → tick; a copy at the bound read, one byte past refused).
- **03** — `sessions::exec::inventory` (every entry, kind-tagged stamp, links not followed,
  problems said, `INVENTORY_CAP`), `promote::workspace_facts`, `offer::refused_unlisted`,
  `snapshot_revision(readme, stamps, targets)`. Test:
  `the_archive_checklist_holds_everything_the_emptying_removes` (empty checklist, then
  `.draft.md`, `.staging/output.md`, a link).
- **04** — `PlanStep::EmptyDirKeep.accepted`, `ArchiveDecision.accepted`,
  `verbs::archive_with` (inventory under the held zone, passed to `before` as `Workspace`); the
  executor refuses an emptying whose directory holds anything not accepted at its stamp, at the
  step and on resume. Test: `an_archive_resumed_after_a_crash_keeps_work_that_arrived_since`
  (after a completed checked copy; a decided file rewritten; a folder arriving before any step;
  a half-emptied workspace completes).
- **05** — `offer::target_fact`, `row_revision(…, target_fact)`, `promote::row_files(…, dated)`
  shared by the panel and the archive. Test: `losing_a_target_takes_the_choice_with_it`
  (refresh after deletion; drift between checklist and archive); core
  `revisions_move_only_with_what_they_bind`.
- **06** — `offer::{ChoiceVm, NoteIntentVm, PanelIntentVm, ReadState, decisions, decide,
  archive_promotions}`, `promote::offer::panel_for`; `PromoteRowVm.choice`, `UnlistedVm.choice`,
  `KnowledgeNoteVm.{candidateRead, copyRead, consented}`, `SessionPromoteVm.{intent, complete}`;
  `offer::Archive.{choices, root}`. `promote-panel.tsx` forwards intent and renders (no
  synthetic rows, no revision comparison, no completeness rule). Tests: core
  `a_choice_holds_only_for_the_item_it_was_made_on`, `consent_holds_only_to_the_version_read_and_shown`;
  agent `an_archive_needs_one_choice_for_every_row`, `the_panel_decides_reads_and_consent`;
  the panel tests keep their outcomes, now waiting for Rust's answer before a read enables
  consent, consent enables promotion or a copy read enables the tick.
- **07** — `keeper-core/src/sessions/promote-vectors.json` (digests computed independently with
  Python's hashlib) loaded by core `every_promote_vector_matches`, agent `every_text_vector_matches`
  and `src/test/mock-shell-promote-contract.test.ts` (WebCrypto present and stubbed away);
  `dev/mock-shell.ts`: byte-accurate `bytes` (PNG signature; `artifacts/trace.log`, valid UTF-8
  with a NUL), `promoteRowRevision`/`promoteSnapshotRevision`/`promoteCopyRevision`/
  `promoteIsText`/`promoteDecisions`, the reader bounds and refusals, archive admission as Rust's.
- **08** — `KnowledgeCard`'s post-promotion focus to **Read notes copy** (heading fallback).
  Test: `keeps focus in a knowledge card after its promotion: on its notes-copy reader`
  (keyboard: new publication, missing-copy restore, re-promotion).
- **09** — the archive test's captured-payload assertion deleted; it asserts the rendered
  queued target and the refusal.

Shell (by inspection): `sessions_promote_panel` (+`intent` → `offer::panel_for`),
`sessions_archive` (`choices`, `root: &profile.local_path`), `sessions_knowledge_review`
(→ `promote::review`), each `#[cfg(not(desktop))]` twin; macOS awaits CI (hesperia had 12 GB
free). Deferred: DW-910…914.

Browser proof (2026-10-08, mock shell serving the contract above): 1280 px full app and a real
420 px viewport over the real `SessionsPane` in a throwaway harness (DW-781 stands),
`/tmp/agents-salvage/panel95-browser-3/`. At both widths: dialog 768/388 px equal to its scroll
width, no page overflow; `chart.png` (PNG bytes) "not a text file" with no button,
`trace.log` (UTF-8 with a NUL) offered; candidate and notes copy read apart, the copy's untick
lands; keyboard: row editor in/cancel/back/complete, then **Read notes copy** focused after a
new publication and after a missing-copy restore; Archive disabled until all nine items are
chosen, disabled while a reread is held, kept after an unchanged reread, archived. Only the
two dev-server font 403s (symlinked `node_modules`) as errors.

Mutation (`/tmp/agents-salvage/mut-95panel-4-*.log`, the script restores on SIGTERM/SIGHUP/SIGINT):
13/13 killed by assertion in the tests named above — 01 amend unbound, 02 copy read at the
candidate cap, 03 hidden entries skipped, 04 emptying ignores its inventory, 05 row revision
without the target, 06 refused item keeps a promotion / consent without a current read /
undecided row skipped / ready without Rust's `complete`, 07 mock row revision without the
target / NUL not text / fallback SHA padding, 08 focus not moved after a knowledge promotion.

### As built (rung `agents-95-panel`, 2026-10-08) — re-review fixes (round 3: R95P3-01…08, R249)

Ruling R249 (`review-95panel-3.md`). Per finding — symbol, then the test that fails without it:
- **01** — `PlanStep::EmptyDirKeep { path, decided: Option<Emptying> }` (the field is new, so a
  journal written before it — no record, or R234's bare `accepted` inventory — deserialises with
  `decided: None`); `exec::empty_as_decided` refuses `None` before touching anything: the plan is
  abandoned, the workspace kept, the session not moved, a new checklist needed. Test:
  `an_earlier_keepers_archive_journal_removes_nothing` (exec; both older journal shapes, verbatim
  JSON, with work arriving before the resume).
- **02 + 04** — `plan::Emptying { entries, root, targets, drive_targets, zone_in_drive }`;
  `exec::identity` (`<dev>:<ino>` of a real folder); `exec::inventory` refuses to list a `top`
  that is a link or not a folder; `exec::empty_as_decided` checks the folder is the decided one
  (`identity`) at its own place (`at_its_place`: canonical path = zone + plan path), then the
  targets, then the inventory, then (test seam `exec::seam::after_check`) removes only
  `entries`, deepest first, each file/link re-stamped (`stamp_of`) before `remove_file`, each
  folder by `remove_dir` (never recursive), then refuses if anything but `.gitkeep` is left.
  `verbs::Workspace.root`, `verbs::archive_with` (the `before` closure now returns the steps and
  the `Emptying` targets). Tests: `work_arriving_while_the_workspace_is_emptied_is_kept` (seam:
  a top-level arrival, one inside a decided folder, a decided file rewritten),
  `an_emptying_is_bound_to_the_folder_it_was_decided_on` (replaced by a twin with the same
  names and stamps; a link in the zone; a link out of it; a link on the way),
  `an_archive_resumed_after_a_crash_keeps_work_that_arrived_since` (half-emptied: a folder, a
  file — recovers), agent `a_linked_workspace_is_never_listed_or_emptied` (a root link in the
  zone and out of it: problem, archive refused, the other folder intact).
- **03** — `promote::offer::archive` records in `Emptying.targets`/`drive_targets` what each
  row whose source is in the workspace said of its target (`offer::target_fact`), then
  `sha256:<hex>` for every target the plan's own `CopyChecked` writes; `zone_in_drive` (shared
  with `row_files`, now also trying the canonical drive) finds drive-relative targets again;
  `exec::moved_target`/`still_says` revalidate at the emptying and every resume. Test:
  `a_resumed_archive_empties_only_while_its_targets_say_what_they_said` (all skipped / promoted,
  crash at the emptying via the seam, the target deleted or replaced → refused, source kept,
  the old intent incomplete; nothing changed → archives).
- **05** — `promote::promote_panel` gives every repeat of a line its own revision
  (`offer::occurrence`) and refusal (`offer::repeated`); `decide` and `archive_promotions` then
  agree. Tests: agent `a_row_written_twice_is_two_choices_the_archive_takes` (real README, one
  skip incomplete, both decided → archived); panel `keeps a row written twice as two rows, each
  archived only on its own choice`; vector scenario "a row written twice".
- **06** — `keeper-agent/src/promote/command-vectors.json`, recorded by
  `every_command_vector_holds` over real files (`KEEPER_WRITE_VECTORS=1` records) and replayed
  through the mock's handlers over IPC by `src/test/mock-shell-promote-commands.test.ts`: whole
  panel VMs (offers, target facts, revisions, copies, problems), candidate/copy reads,
  publication of a byte-backed note and artifact, reviews, oversized/unreadable/missing copies
  (scenario "oversized, unreadable and missing copies": a vault copy keeper may not read is
  the panel's copy problem and the read's refusal),
  archive refusals (undecided, stranger, twice, refused row, admission, stale snapshot,
  unlistable workspace) and success. `dev/mock-shell.ts`: `promoteDigest` (review keys aside),
  `cellBytes`, `fileText` for every text read and publication, `file:<len>:<mtime ns>` stamps,
  `dirs`, `promoteProblems`, unreadable ordinary artifacts, a missing copy's own sentences,
  the archive's listing refusal, repeat identities, `head`. The independent SHA/fallback
  vectors stay.
- **07** — `promote::review`'s growth guard. Tests: `a_review_never_grows_the_copy_past_what_keeper_reads`
  (lands exactly at 81,920 and reads whole; one byte longer refused, bytes kept); vector
  scenarios "a review that lands exactly at the reviewed bound" / "…would cross…" against the
  mock's guard.
- **08** — deleted: `an_archive_plan_moves_the_folder_last`'s copied-step and verb echo,
  `every_promote_vector_matches`' and `every_text_vector_matches`' non-empty checks, and the
  contract test's two `length > 0` checks.

Shell crate: unchanged (no keeper-core/agent signature it calls changed). Deferred:
DW-1000…1002.

Browser proof (2026-10-08, the mock shell above): 1280 px full app and a real 420 px viewport over
the real `SessionsPane` in a throwaway harness (removed; DW-781 stands),
`/tmp/agents-salvage/panel95-browser-6/` (rerun of `panel95-browser-5`). At both widths: dialog
768/388 px equal to its scroll width, no page overflow; `artifacts/locked.md` (unreadable) says
"could not be read" with no button, `trace.log` still offered; a `## Promote` line written twice
shows two rows, the repeat saying why it promotes nothing; with every other item skipped and the
first row skipped, the repeat stays undecided and **Archive session** disabled; skipped too, it
enables and archives. Only the two dev-server font 403s (symlinked `node_modules`) as errors.

Mutation (`/tmp/agents-salvage/mut-95panel-5-*.log`, `mut-95panel-5b-*.log`; both scripts restore
on SIGTERM/SIGHUP/SIGINT, no mutant text left): 26/26 killed by assertion in the tests named
above — 01 an earlier journal empties everything; 02 a decided folder removed recursively / what
is left not checked / a half-done emptying not resumed; 03 targets not revalidated / the plan's
own promotion taken for drift / no target recorded; 04 folder identity ignored / place not
checked / inventory follows a root link; 05 a repeat shares its revision (Rust and mock); 06 mock
target fact hashes raw bytes / unreadable artifact offered / publication reads text not bytes /
missing copy reads as unpromoted / archive ignores listing problems / stamps in ms / unreadable
copy read / unreadable copy in the panel; 07 review growth unbounded / guard refuses at the
bound / guard lets one byte past (Rust and mock, the mock's off-by-one both ways).

### As built (rung `agents-95-panel`, 2026-10-08) — re-review fixes (round 4: R95P4-01…03, R265)

Ruling R265 (`review-95panel-4.md`). Per finding — symbol, then the test that fails without it:
- **01** — `exec::Folder` (Unix: an `OwnedFd`; `reach` opens the zone, then each part of the plan
  path with `O_NOFOLLOW|O_DIRECTORY`; `child`, `stamp` = `statat(AT_SYMLINK_NOFOLLOW)` +
  `readlinkat`, `names` = `Dir::read_from`, `remove` = `unlinkat`/`AT_REMOVEDIR`, `sync`, `keep`
  = `O_CREAT|O_EXCL|O_NOFOLLOW`; off Unix a path, DW-1002); `exec::walk` (the inventory over a
  held folder, recording each folder's identity), `exec::down` (a folder reached from the held
  root, each folder on the way the one the check recorded), `exec::identity`/`identity_of`
  (`<dev>:<ino>` from one `stat` formatter). `exec::empty_as_decided` reaches and holds the
  workspace before its checks, binds it to `Emptying.root`, walks it held, and after the seam
  re-stamps and unlinks every decided entry relative to its held parent; a held folder no longer
  at its place once emptied refuses. `exec::inventory` (the panel's and the planner's) shares
  `walk`; `at_its_place`/`stamp_of` are gone. Test: `a_folder_swapped_in_after_the_check_is_never_emptied`
  (exec, Unix; at `seam::after_check` the workspace, or `workspace/sub`, swapped for a link to a
  same-stamp twin in the zone or outside it, or for the twin itself → refused, every twin byte
  kept, the session not moved); `an_emptying_is_bound_to_the_folder_it_was_decided_on` keeps the
  before-resume cases. DW-1001 rewritten to the exact remaining exposure; `PlanStep::EmptyDirKeep`'s
  doc and `docs/sessions.md` no longer claim more.
- **02** — `dev/mock-shell.ts`: `utf8Text` (`TextDecoder("utf-8", { ignoreBOM: true, fatal: true })`,
  `null` when not UTF-8) behind `fileText`, `promoteIsText`, the candidate read
  (`readNoteFixture`: cap on stored bytes, then unreadable, then `… could not be read: stream did
  not contain valid UTF-8`), the panel's knowledge rows (Rust's order: size, then the read),
  publication (`promoteInFixture` publishes only exact text) and `sync_read_text` (a not-UTF-8 file
  is `binary` with `text_file`'s sentence); `readRefusal` takes stored bytes; `promoteDigest`
  decodes the head with `utf8Prefix` (the valid UTF-8 prefix, exactly) and finds its fence after
  a BOM. Test: vector scenario "a byte-order mark kept, and a candidate that is not UTF-8"
  (recorded by `every_command_vector_holds`: a BOM-prefixed harvested note and artifact read,
  published and re-read with the review in; a malformed candidate's panel problem, read and
  promotion refusals; a malformed oversize candidate refused for its size), replayed by
  `src/test/mock-shell-promote-commands.test.ts`.
- **03** — `promote/tests.rs`: `every_command_vector_holds` and its helpers (`vector_bytes`,
  `put_at`, `outcome_of`, `vector_choices`, `command_outcomes`) in one `#[cfg(unix)] mod
  command_vectors`. No behaviour; no Windows build run (none here).

Shell crate: unchanged (no keeper-core/agent signature it calls changed: `exec::inventory` and
`exec::identity` keep theirs). No rendered surface changed, so no browser run. No new DW.

Mutation (`/tmp/agents-salvage/mut-95panel-6.py`, logs `mut-95panel-6-*.log`, runs
`mut-95panel-6-run-{ts,a,b}.log`; restores on SIGTERM/SIGHUP/SIGINT, files byte-identical after):
9/9 killed by assertion — 01 (in `a_folder_swapped_in_after_the_check_is_never_emptied`) unlink by
path / the holder reached by path / the place not checked after / a folder's identity ignored;
02 (in the vector scenario above) BOM dropped / malformed read as U+FFFD / cap on decoded text /
the digest head loses the BOM / the digest fence ignores the BOM.

### As built (rung `agents-95-panel`, 2026-10-10) — restack onto fe760de0 (R270)

Restacked onto knowledge's final commit as a semantic union on its API: `promote::panel` takes the row files from `row_files` and each harvested note's copy fact (`PanelFacts::copies`, `copy_fact`) so the panel tells `foreign_copy`; `promote::review` refuses a copy that is not the note's (`promote::standing`, `CopyLoss::explain`) before its as-read check (`offers::copy_revision`, `COPY_CHANGED`) inside the guarded amend; `offer::out_target` reads a note's one row (`promote::entry_of`); a foreign copy offers no promotion and says why (`unavailable`, R298); `exec::still_says` uses `browse::resolve_known`'s `Known`; the mock mirrors all of it (`standing`, `copyLoss`, `promoteCopyDigest` for a published row's fourth cell). Tests: `a_review_lands_only_on_the_copy_as_read`, `a_review_never_writes_into_a_file_the_note_did_not_publish`, `the_panel_takes_a_notes_copy_from_its_one_row`, `every_command_vector_holds` (rows recording their copy's publication), `mock-shell-promote-reads.test.ts`; mutation `mut-95panel-rs.py` (`mut-95panel-rs-*.log`): 31 inherited mutants killed (4 round-5 mutants superseded by round 6's 6-01, not re-run) plus 8 reconciliation mutants killed. R95P5-01 deferred as DW-1050 (R270); DW-1141 opened. Converged after restack (review-95panel-6.md, R300); R95P6-01 deferred as DW-1142. keeper-core's lib tests and the bindings regeneration check await delectra; macOS awaits CI.

## UX decisions

UX-DR137 is the architecture's; UX-DR138 is this epic's.
- **UX-DR137 — the promote panel's agent additions** (AD-404; R29 F20). It refines **UX-DR90** (the phase-7 promote panel: `epics-sessions-phase7.md:120-124`, `ux-designs/ux-keeper-2026-07-03/EXPERIENCE-SESSIONS.md` § *Promote panel* and § *UX-DR90*), which stands as decided: two columns, workspace → artifacts, one row per `## Promote` row, per-row actions, the stale and missing badges, a loud missing target, unlisted workspace files one action away, and the README table as the source of truth that the panel renders and never owns. UX-DR137 adds only what agents bring:
  - **Knowledge notes:**
    - a group of their own, each with its OKF title, "written by Dr Tola Grey on electra";
    - the tick *Reviewed by me*;
    - *Promote to notes…*, which opens a folder picker over the drive's vault;
    - the note opens whole — a note holds at most 64 KiB, and the panel never truncates one (S-32).
  - **The staleness badge on a knowledge row:** UX-DR90's stale badge, worded *Newer here*, also marks a harvested note that changed in the session after it was promoted into the vault.
  - **The label:** the session's label chip is at the head of the panel. A target outside the label is not offered, rather than offered and refused. AD-27 applies: no dead control.
- **UX-DR138 — a memory change waiting for its person.**
  - **The preview:** the approval card's preview (93.3) for a consolidation on a shared drive, or for a private drive's change no person's words stand behind (95.2, S-13), is an artifact listing `USER.md` or `MEMORY.md` before and after, entry by entry. Each entry is marked added, replaced (old and new side by side) or removed, with the source sessions as links.
  - **The summary sentence**, keeper's template (93.1's Q12): "Dr Lucyna Novak would remember 2 new things about neuraffica."

## Operator actions

These are owed outside this repository. Each is named where a story depends on it, and none is assumed done.
- **OA-95-1 — the electra hosts are always-on and pushing** (95.2, 95.3). This is epic 90's operator action, restated:
  - `agentd-tgorka` and `agentd-neuraffica` run with `always_on = true` in their `agentd.toml`;
  - neuradrive's checkout pushes, per ruling R17.

  Without it, nothing consolidates, and a shared drive's review never reaches its owner.
- **OA-95-2 — the owner states the licence of `tgdrive:.okf/bin/`, and whether `.okf/config.yaml` may live in this repository as a fixture** (95.4). The answer is one line in `keeper-ported/src/okf/UPSTREAM.md`, for example "MIT, by the owner, who wrote both". Until it is given, the module is written from the scripts' documented behaviour (ruling R21), and the config test reads `$KEEPER_TGDRIVE` under `#[ignore]`.
- **OA-95-3 — the drive's session-workspace exclusion** (Q9, DW-389). Either teach `tgdrive:.okf/bin/okf_lib.py`'s `_match` a `**` segment in the middle of a pattern (the README already promises gitignore-like rules), or replace `60-sessions/**/workspace/**` (`config.yaml:75`) with lines the literal-prefix rule honours. Then run `okf index --check` and `okf validate` in the drive. `keeper-ported::okf` follows whichever the owner chooses, through its parity fixture.
- **OA-95-4 — OKF bundle membership of `80-agents/`** (research §13 #45). As configured, `80-agents/**` falls into the root bundle `tgdrive` and is searchable, journals and proposals included. Whether it becomes its own bundle, is excluded, or stays is the owner's drive configuration. `drive_search` follows `config.yaml`.

## What stays out

- **An agent writing its own core memory, skills or soul.** Refused (AD-362, AD-364). An agent proposes; consolidation or a person writes.
- **An agent's skill offered before a person adopts it.** Refused (R28 S-12, Q5).
- **LLM rewriting of `MEMORY.md`** (OpenClaw's "tool-free completion" that chooses merges). Not ported. The consolidator applies the proposals the agent wrote, as Hermes' batch does, and nothing composes new prose overnight.
- **Promoting into another drive in one step.** Refused (95.5 #6). A note goes into its own drive; another drive is a person's own copy.
- **A search index built on agentd.** Deferred (DW-388).

Deferred, with the ledger entries opened here so a later planner finds them; `_bmad-output/implementation-artifacts/deferred-work.md` carries DW-385…DW-392 and DW-440 in full.
- DW-385 — signed or hash-pinned skills, with quarantine on mismatch (placed by the architecture).
- DW-386 — LLM consolidation of skills in the curator (placed by the architecture).
- DW-387 — a TTL on journals and on outside-derived proposals beyond gate sessions (placed by the architecture).
- DW-388 — agentd builds no notes search index; `drive_search` on Linux is lexical.
- DW-389 — the drive's own OKF tools never exclude a session's workspace.
- DW-390 — an artifact can be promoted out only into the drive's notes vault.
- DW-391 — unread marks, per-file history and capture into the session log (FR-235, FR-236, FR-241) stay unbuilt beside the panel.
- DW-392 — the consolidation measures four of OpenClaw's six ranking signals.
- DW-440 — a person adopts an agent's skill by editing its file; keeper has no adopt action.

## The failure shape this epic must not repeat

**Memory that someone else wrote.** Stored injection survives sessions (MINJA, Zombie Agents; research §9.3). A review that finds any of the following is a blocker:
- a path by which an agent's turn writes `USER.md`, `MEMORY.md`, `_skills/**` or a verdict file;
- a proposal written before its threat scan, or the scan skipped for a skill body;
- a promotion from an `untrusted`, scheduled, delegated or gate session, or any verdict but `expired` on a gate proposal;
- on a private drive, a change applied with no `owner`/`peer` proposal behind it and no person's approval;
- a skill offered while it carries `metadata.keeper_proposal`;
- a night that drops more than a quarter of what was there, or a guard that compares bytes read without the night's pull instead of blob ids at the fetched head;
- a shared drive's memory changed without its person's consumed approval.

**A note that pretends.** A review that finds any of the following is a blocker:
- a harvested note whose `generated.by` is a person, or that carries `verified` written by an agent;
- a knowledge write into a session its agent does not own;
- a promotion wider than the note's label;
- a harvested note over 64 KiB stored, or a panel that truncates one.

**Search that reads what it must not.** A review that finds any of the following is a blocker:
- a file under an exclusion opened by `drive_search`;
- an LFS pointer read as text, or anything hydrated;
- a regex compiled from a query;
- a result without its drive's label.

## Sprint-status entry

Applied under `development_status:` in `_bmad-output/implementation-artifacts/sprint-status.yaml`, above the epic-94 block and below epic 96's: `epic-95` and its five story keys.

## Stack rungs

Rungs by layer, bottom → top, on top of epic 94's last rung (`epic94-workflows`). Each compiles alone.
1. **`epic95-memory`** — 95.1, 95.2 and 95.3.
   - `keeper-ported/src/hermes/` (memory, threats, curator) and `openclaw/`, each with `UPSTREAM.md` and fixtures;
   - `keeper-core/src/agents/{memory,proposal,consolidate,curate}.rs`, and the adoption rule in 89.3's `agents/skills.rs`;
   - `keeper-sync`'s `Engine::commit_paths` and its trailer keys;
   - `keeper-agent/src/{memory,consolidate,curate}.rs` (the evidence rule, the blob-id guard, the gate-proposal sweep);
   - `docs/agents.md` § *Memory*, § *Skills*.

   The PR names the Synapse lease test as `#[ignore]` and pastes its run.
2. **`epic95-knowledge`** — 95.4 and 95.5.
   - `keeper-ported/src/okf/` and its fixtures (OA-95-2);
   - `keeper-core/src/agents/{search,knowledge}.rs` and `sessions/promote.rs`'s panel model;
   - `keeper-agent/src/search.rs`, the `artifacts/knowledge/` rule and `promote_in`/`promote_out`;
   - the shell's three commands, named in the PR as awaiting CI's macOS job and `check:rust:macos`;
   - `src/components/sessions/promote-panel.tsx`, its tests and the mock shell;
   - `docs/sessions.md`, `docs/agents.md`.
