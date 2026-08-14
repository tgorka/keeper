---
topic: keeper sessions — LLM work sessions inside synced folders (60-sessions zone)
date: 2026-08-12
purpose: verified ground truth for bmad brainstorming/prd/ux/architecture — what is on disk today and what the owner asked for
sources:
  - stakeholder ask (owner, 2026-08-12, verbatim below)
  - /Volumes/merope/tgdrive/60-sessions/{README.md,AGENTS.md,_template/} (hesperia, read 2026-08-12)
  - /Volumes/merope/neuradrive/60-sessions/{README.md,_template/} (hesperia, read 2026-08-12)
  - docs/notes.md, docs/sync.md, _bmad-output/planning-artifacts/* (keeper repo)
---

# Keeper Sessions — ground truth

## 1. Stakeholder ask (owner, 2026-08-12, translated from Polish, faithful)

> Create a new feature: **sessions** — sessions for LLM work, manual or hybrid, in synced
> folders (choose which folder like in notes — reuse the option pattern: *this folder has
> sessions*). Look at the structure, README and AGENTS.md in tgdrive and neuradrive on
> hesperia to know what to build. Build a new menu option and a user interface in keeper to
> **see what is being worked on** and to **manually edit** (look at how notes edit md files
> — the full same options including quick capture). When I run an agent locally or edit a
> file outside keeper, I want to **see the changes in keeper** (live). I want an **archive
> session** option, and **create new** (pick a pattern from previous sessions — see
> template) — a session can offer "create new following the same pattern" (or a different
> one, but with a reference back to this session). Look at how keeper links files —
> especially to notes — and make the analogy. When archiving: **if any workspace file is
> referenced as an artifact, copy it into artifacts/ so it can be synced**. I also want to
> **list sessions with search** (the session's main README can carry tags and properties —
> the tags/properties of the whole session) — also use **pins** (like notes). Look at the
> UI/UX of the whole app for consistency. Research the internet for best working practices.
> Use BMAD (brainstorming, research, analysis, UX, architecture, implementation, bmad-loop).
> Use gh pr / gh stack so everything is on a stack; PRs **ready for review, not draft**.

## 2. What exists on disk (verified on hesperia, 2026-08-12)

Two git+LFS drives synced by keeper, same shape: `tgdrive` (personal) and `neuradrive`
(company, two people). Both have a `60-sessions/` zone:

```text
60-sessions/
  README.md     # zone contract (below)
  AGENTS.md     # agent rules for the zone
  _template/    # the skeleton to copy: README.md, workspace/, artifacts/, refs/, prompts/
  active/       # sessions still running: YYYY-MM-DD-<slug>/
  archive/      # finished sessions, filed by close year: archive/YYYY/
```

Inside a session (from `_template/`):

```text
YYYY-MM-DD-<slug>/
  README.md     # THE record: summary, key decisions, dated log, what worked, follow-ups, promote table
  workspace/    # scratch — gitignored (60-sessions/**/workspace/), NOT versioned, NOT backed up
  artifacts/    # promoted output — versioned; final reports, research, progress notes
  refs/         # inputs worth keeping: excerpts, small csv/json, pointers into other zones
  prompts/      # reusable prompts, numbered NN-slug.md
```

Template README skeleton: title; `Date`/`Tool/model`/`Goal` bullets; sections `## Summary`,
`## Key decisions`, `## Log` (one dated `### YYYY-MM-DD — what moved` entry per sitting,
newest last), `## What actually worked`, `## Follow-ups`, `## Promote` (a
`| workspace | → artifacts | note |` table). neuradrive's variant adds "who ran it".

### Zone rules that BIND the feature (from README/AGENTS.md of the zone)

- **Promotion is an explicit copy under a stable name**, recorded in the `## Promote`
  table. Re-promote under the same name whenever current state is worth sharing — one
  shareable "current" path, git history keeps every promoted version. Iterations between
  promotions die with the workspace, by design.
- **Finishing = delete or archive, nothing else.** Delete when nothing is worth keeping
  ("archiving empty sessions is how this zone rots"). Archive = promote finals per the
  table, empty `workspace/` (leave `.gitkeep`), close the README, move to `archive/<year>/`.
- **Continuation, not growth**: when the goal shifts or the README stops summarising on one
  screen, open a new dated folder, link the two READMEs both ways, archive the old one.
- **Big binaries never live in a session** — file them in their zone (`40-media`, …) and
  reference by repo-root-relative path from `refs/` or the README.
- How much the README must carry depends on who remembers: agentic tool keeps a transcript
  → record decisions + reusable prompts; browser chat keeps nothing → paste as you go.
- Drive-wide: **keeper owns git history** (agents/humans only touch files; keeper scans,
  commits `sync(tgdrive@hesperia): …`, pushes). Moving a referenced file is a two-part
  edit. `60-sessions/**/workspace/` is already excluded in the drive's ignore rules.

## 3. What keeper already has that sessions must reuse (not rebuild)

From `docs/notes.md`, `docs/sync.md`, and the phase-5/6 planning artifacts:

- **"A vault is a folder you already sync, plus a flag"** (AD-54). Sessions must be the
  same move: a `SyncProfile` carrying a *sessions* flag + a subfolder name (default
  `60-sessions`, configurable — neither drive spells it differently today, but the flag
  must not hardcode the zone name).
- **The whole notes editing surface**: CodeMirror 6 markdown editor with live preview,
  format toolbar, slash menu, mermaid, tables, CSV/JSON embeds, properties panel over
  frontmatter (frontmatter never in the buffer; byte-preserving writes), tag combobox,
  wikilinks `![[…]]`, attachment planning, viewer registry (AD-87/88), raw↔rendered.
- **Quick capture**: prewarmed hidden window, global hotkey, Escape commits+hides, buffer
  survives dismissal/restart, "open X as a capture window". The owner explicitly wants
  capture parity for sessions (e.g. capture into the active session's README log).
- **Live external change pipeline**: watcher → `NoteChangeBatch`/`NoteBodyBatch` channels,
  250ms coalescing, clean-buffer live apply / dirty-buffer merge + diff bar, unread marks
  cleared against the exact `headRev`, history/blame from commit trailers (origin:
  device/agent), conflicts as rows.
- **Pins/archived as frontmatter booleans** (`pinned`, `archived`), ULID `id` identity,
  `keeper.*` reserved namespace — sessions README frontmatter should follow the same tiers.
- **Query language** (tag:/path:/field:/date:/origin:/is:/text:/link:) and spaces-as-saved-
  queries; the sessions list search should reuse the same grammar and parser.
- **Capability gating**: `CapabilitiesVm.notes` pattern → `CapabilitiesVm.sessions`, true
  only where sync is available and a profile is flagged.
- **IPC shape**: ts-rs generated `Session*Vm/Req/Batch/Op` types, `sessions_<verb>`
  commands, channel subscriptions, `keeper://` events — mirror the notes naming exactly.
- **Vault-link**: notes link files by repo-root-relative path; sessions must be linkable
  the same way (a note can link a session; a session README links artifacts/refs by
  relative path; moving a referenced file is a two-part edit).

## 4. The deltas — what sessions adds that notes does not have

1. **A session is a folder, not a file.** The unit of listing/pinning/tagging/archiving is
   the directory; its `README.md` frontmatter carries the session-level tags/properties.
2. **Lifecycle verbs**: create-from-template (or "same pattern as session X", with a
   back-reference in both READMEs), archive (with the promote-check below), delete,
   continue (new session referencing the old one both ways).
3. **The promote airlock**: at archive time, any `workspace/` file referenced as an
   artifact (in the `## Promote` table, or linked from README/artifacts) is **copied into
   `artifacts/`** so it survives and syncs; then workspace is emptied (`.gitkeep`), the
   folder moves to `archive/YYYY/`. keeper must show what will be promoted/lost before
   doing it.
4. **Status surface**: "see what is being worked on" — active sessions with freshness
   (last file change, last log entry, unread since last look), workspace vs artifacts
   activity, who/what wrote (origin trailers).
5. **Templates at the folder level**: `_template/` is a directory skeleton, not a note
   template; "create new like session X" means copy X's shape/prompts selectively with a
   `continues`/`continued-by` (or similar) frontmatter reference.

## 5. Constraints and non-goals (grounded)

- keeper never runs the LLM/agent — sessions are *observed and edited*, not executed.
  Manual or hybrid means: human edits in keeper, agent edits on disk, both visible live.
- keeper never moves/renames a session folder unasked; archive is an explicit user action.
- `workspace/` contents are unversioned by drive rule — keeper must not commit them, and
  the archive flow must respect that the working tree is their only copy.
- The zone's ignore rules already exist in the drives; the feature must work with an
  existing populated `60-sessions/` (adopt, don't scaffold) and with an empty one.
- Sessions must stay Obsidian/agent-compatible: plain folders, plain markdown, frontmatter
  only — the file is the API, same as notes.
