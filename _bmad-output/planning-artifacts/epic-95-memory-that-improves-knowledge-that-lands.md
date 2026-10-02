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
  - `sessions_promote({rootId, sessionId, source, target, note})`;
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
   - **The writes:** in a steward's harvest session (92.5), the turn writes `artifacts/knowledge/<source-slug>/<note>.md` through `session_write`. A fake model writes `generated: {by: human:tgorka}` and `verified: [...]`.
   - **What is stored:** the stored note's `generated.by` is `agent:tola-grey@electra`, `human_reviewed` is `false`, and there is no `verified` key.
   - **Bad sources:** a `sources[].resource` outside the named source session is refused.
   - **The size cap** (S-32): a note of exactly 64 KiB is stored; one byte more is refused with the sentence and nothing is written.
   - **Test:** `a_harvested_note_never_claims_a_person` (keeper-agent).
2. **A harvest never writes the closed session** (Q2).
   - A harvest turn's `session_write` aimed at the source session is refused ("a session is written only by its own agent"). The source session's files and log are byte-identical after the harvest.
   - **Test:** `harvest_never_writes_the_closed_session`.
3. **keeper reads a harvested note as unreviewed** (Q3).
   - `okf_facts` on it: generated by `an agent`, `human_reviewed` false, sentence "nobody has reviewed it".
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
   - **Into the vault:** a harvested note promoted into `10-notes/knowledge/<note>.md` in the same drive is copied with its frontmatter, and the row `| artifacts/knowledge/<…>.md | 10-notes/knowledge/<note>.md | knowledge |` is recorded.
   - **Outside any vault:** a target is refused with "keeper creates files only inside a notes vault" (DW-390).
   - **Another drive:** a target is refused.
   - **A wider audience:** a note whose session label readers do not include the target drive's readers is refused with the reason. For example, a session label `{tgorka}`, narrowed by what the session read, against a target vault whose drive's readers are `{tgorka, marta}`. The test drives the promote path with that label, because the process-per-principal mount rule (AD-377) makes the cross-drive read itself hard to stage.
   - **Tests:** `promote_out_into_the_vault`, `promoting_out_respects_the_label`.
7. **Only a person marks a note reviewed.**
   - **The tick:** "Reviewed by me" on a candidate sets `human_reviewed: true` and appends `verified: - by: human:tgorka`, `at: …`, leaving every other byte.
   - **The untick:** removes that person's entry and sets `false`.
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
