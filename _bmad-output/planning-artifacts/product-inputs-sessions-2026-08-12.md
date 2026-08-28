---
title: "Product inputs — Sessions phase (Phase 7)"
status: final
created: 2026-08-12
sources:
  - _bmad-output/brainstorming/brainstorm-keeper-sessions-2026-08-12/ground-truth.md
  - _bmad-output/brainstorming/brainstorm-keeper-sessions-2026-08-12/.memlog.md
  - _bmad-output/brainstorming/brainstorm-keeper-sessions-2026-08-12/brainstorm-intent.md
---

# Product inputs — Sessions (Phase 7)

Stakeholder ask, verbatim intent, and the numbering spine every Phase 7 document binds
to. This file is the contract between the PRD amendment, the architecture amendment, the
experience extension and the epics: **numbers are allocated here and nowhere else.**

## 1. Stakeholder ask (owner, 2026-08-12)

> Create a new feature: sessions — sessions for LLM work, manual or hybrid, in synced
> folders (choose which folder like in notes — the folder gains an option: *has
> sessions*). Take the structure from tgdrive/neuradrive `60-sessions/` (README, AGENTS,
> `_template/`). Build a menu option and UI to see what is being worked on and to edit
> manually — the full notes editing options including quick capture. When an agent runs
> locally or a file is edited outside keeper, the changes must show in keeper. Archive a
> session; create a new one following a previous session's pattern (or another pattern,
> with a reference back). Link files like notes do. On archive, any workspace file
> referenced as an artifact is copied into artifacts/ so it syncs. List sessions with
> search — the session README carries the session's tags and properties; use pins. Keep
> the UI/UX consistent with the whole app. PRs ready-for-review on a gh stack.

## 2. Upstream synthesis

The divergent session (89 logged entries, 6 techniques) is in
`_bmad-output/brainstorming/brainstorm-keeper-sessions-2026-08-12/.memlog.md`; the
distilled input is `brainstorm-intent.md` beside it; the verified on-disk contract of the
zone (both drives read on hesperia) is `ground-truth.md` beside that.

Four verdicts carry the phase:

1. **Reuse over rebuild** — sessions reuse the notes editor, capture, watcher channels,
   frontmatter tiers, ULID identity, query grammar, trash and conflict surfaces
   wholesale. The new domain is folder-level: a sessions index, lifecycle verbs, the
   promote/archive airlock, and lineage.
2. **A sessions root is a sync profile plus a flag** (the AD-54 move repeated) — default
   subfolder `60-sessions`, adopt-only, `_template/` and `_*`/`.*` skipped.
3. **Files are the only truth** — status is folder location, freshness is subtree mtime
   (workspace vs artifacts split), lineage is `continues`/`continued-by` ULID refs,
   promotion is the README's `## Promote` table. Nothing a Finder edit could desync.
4. **The archive flow is the zone's own checklist made visible** — promote per table
   under the stability gate, staleness/hot-workspace warnings, empty workspace with
   `.gitkeep`, move to `archive/<year>/`; empty-handed sessions are offered deletion;
   unarchive is the escape hatch, continuation the preferred reopen.

## 3. Numbering spine (allocated here; do not renumber downstream)

Prior phases end at FR-221, NFR-35, AD-106, UX-DR84, Epic 46.

### 3.1 Functional requirements — FR-222 … FR-252

**Roots & capability**

- FR-222 A sync profile can be flagged *has sessions* with a subfolder name (default
  `60-sessions`); flagging is adopt-only against an existing subfolder, and creating the
  zone skeleton from nothing is a separate, explicitly confirmed action.
- FR-223 `CapabilitiesVm.sessions` gates every sessions surface; absent, not broken,
  when sync is unavailable or no profile is flagged.
- FR-224 Multiple sessions roots (e.g. tgdrive + neuradrive) list side by side; every
  sessions operation is scoped `(rootId, sessionId)`.

**Index & model**

- FR-225 Keeper indexes `active/` and `archive/YYYY/` under the flagged subfolder as
  sessions; `_template/` and any `_*`/`.*` entry are never sessions. The index cache
  lives in the zone's `.keeper/` and rebuilds from disk.
- FR-226 A session's identity is a ULID `id` in its README frontmatter, written once;
  identity, pins, unread marks and lineage survive the `active/ → archive/YYYY/` move
  and a folder rename.
- FR-227 Session-level tags and properties are the README's frontmatter, with the notes
  three-tier ownership rules and byte-preserving writes.
- FR-228 A session row derives: title (README H1), status (folder location), freshness
  split by subtree (workspace vs artifacts/README), snippet (Summary), tags, pinned,
  unread, origin of last change, conflict presence.

**List, search, spaces**

- FR-229 The sessions list searches with the notes query grammar over session fields:
  `tag:` `field:` `date:` `origin:` `text:` plus `is:active` / `is:archived` /
  `is:pinned` / `is:unread` / `is:conflict` / `is:stale`.
- FR-230 `text:` sweeps README, `artifacts/`, `prompts/` and `refs/` (bounded parallel
  scan); `workspace/` is excluded by default and includable by an explicit toggle.
- FR-231 Default session spaces ship (Active, Archived, Stale, Agent-written), editable
  and restorable like notes default spaces.
- FR-232 A session can be pinned/unpinned from row and detail; pinned sorts first
  within its status group.

**Detail & editing**

- FR-233 Session detail shows the session tree (README, `artifacts/`, `refs/`,
  `prompts/`, and read-only `workspace/`) and opens any text file in the full notes
  editor (properties panel, toolbar, slash menu, mermaid, viewers); non-text files use
  the existing viewer registry.
- FR-234 External changes (agent or other machine) appear live: list rows update
  streamed, an open buffer takes clean external writes silently and raises the diff bar
  when dirty — the notes pipeline, unchanged.
- FR-235 A session accrues an unread mark against the exact revision last seen, cleared
  per the notes `headRev` contract; the detail offers "changes since I last looked".
- FR-236 Per-file history and blame (device/origin trailers) are available inside a
  session for versioned files.
- FR-237 `workspace/` is listed read-only with coarse freshness; keeper never commits,
  syncs, or edits workspace content.

**Lifecycle**

- FR-238 New session: from `_template/` — folder `active/YYYY-MM-DD-<slug>/` copied
  verbatim, slug from title with collision counter, README pre-dated; zero prompts
  beyond the title.
- FR-239 New session from a previous session: structure-only copy (README headings,
  `prompts/` contents, `refs/` pointers — never Summary/Log/Decisions content), with
  `continues: <id>` written in the new README and `continued-by: <id>` appended in the
  source README, both rendered as navigable lineage.
- FR-240 Log-today: one action appends (or opens) today's `### YYYY-MM-DD — ` entry in
  a chosen session's `## Log` and drops the caret there; available from menu, palette
  and tray.
- FR-241 Quick-capture parity: a session README opens as a capture window; the capture
  chord can target the current session's log; buffers survive dismissal per the capture
  contract.
- FR-242 The "current session" per root is an explicit, sticky user choice surfaced in
  the tray and used as the default capture/log target; absent a choice, the
  most-recently-touched active session.
- FR-243 Promote: any `workspace/` file can be copied to a stable `artifacts/` name at
  any time; the README's `## Promote` table row is written/updated in the same action;
  copies pass the sync engine's stability gate.
- FR-244 The promote panel shows per-row staleness (source newer than target) and rows
  whose source is gone (promoted-then-deleted is normal, flagged silently).
- FR-245 Archive walks a visible checklist: run outstanding promotes per the table,
  warn on hot workspace (recent writes) and on staleness, empty `workspace/` leaving
  `.gitkeep`, move the folder to `archive/<close-year>/`. Each step is shown; the move
  is the only step that cannot be skipped.
- FR-246 A session with an empty Promote table and empty `artifacts/` at archive time
  is offered **delete instead** (zone rule), through trash.
- FR-247 Delete: a session delete moves the folder into the zone's `.keeper/trash/`,
  recoverable; never a hard unlink.
- FR-248 Unarchive moves a session back to `active/`; the UI prefers offering a
  continuation and states why.
- FR-249 Archive/promote/create complete locally and report sync truthfully through the
  existing journal surfaces; parked uploads surface on the session row, never block.

**Linking & cross-surface**

- FR-250 A note can link a session (wikilink resolved by session id/title) and a
  session README links notes and files by the notes rules (session-relative within the
  folder, repo-root-relative across zones); backlinks resolve across both indexes.
- FR-251 Sessions appear in: the app menu (view + new + log-today), the command
  palette (every lifecycle verb), the quick switcher, and the tray (current session +
  freshness + log-today).
- FR-252 Session rows and detail expose reveal-in-Finder and copy-path for the session
  folder and any file in it.

### 3.2 Non-functional requirements — NFR-36 … NFR-39

- NFR-36 Session index cold scan of a zone with 200 sessions completes under 2 s; the
  list paints from cache first.
- NFR-37 Workspace freshness scanning is bounded: watch only list-visible sessions'
  workspaces, coarse mtime elsewhere; no measurable idle CPU on a 400 GB drive.
- NFR-38 Archive and promote are crash-safe: a killed archive resumes or rolls back to
  a state the checklist can re-run; no half-moved session folders.
- NFR-39 Every sessions write (frontmatter, promote-table edit) is byte-preserving
  outside the targeted key/row, per the notes writer contract.

### 3.3 Architecture decisions — AD-107 … AD-115 (headlines; the amendment owns bodies)

- AD-107 A sessions root is a sessions-flagged `SyncProfile` + subfolder (AD-54
  repeated); `CapabilitiesVm.sessions`.
- AD-108 `keeper_core::sessions` is pure (model, index, promote-table parser, lineage,
  naming); IO/watching in the shell over `keeper-sync`'s watcher, mirroring notes.
- AD-109 One frontmatter writer, one query parser, one editor stack — sessions extend
  the notes crates' surfaces; forks are defects.
- AD-110 Freshness is derived (subtree mtime + git head), never stored; the index cache
  is disposable.
- AD-111 The promote copy path reuses the four-tier stability gate; archive is a
  planned, journaled multi-step fs operation with resume.
- AD-112 Lineage is frontmatter ULID refs (`continues`/`continued-by`), written into
  both READMEs including archived ones — files are truth.
- AD-113 Workspace visibility is a read-only fs projection; nothing under `workspace/`
  enters the index's searchable text by default.
- AD-114 Session IPC mirrors notes IPC: ts-rs `Session*Vm/Req/Batch/Op`,
  `sessions_<verb>` commands, channel subscriptions, `keeper://sessions-*` events.
- AD-115 Sessions surfaces are a primary view + panel targets in the existing shell;
  capture windows host session files through the existing capture machinery.

### 3.4 Experience decisions — UX-DR85 … UX-DR92

- UX-DR85 The sessions list is a status board first: freshness-sorted, status glyphs,
  the last log line as the row's subtitle — "what is being worked on" at a glance.
- UX-DR86 Workspace and artifacts freshness are two visibly distinct signals (iterating
  vs promoted), never merged into one dot.
- UX-DR87 The archive flow is a visible checklist, each step named in the zone's own
  words (promote, empty, file); warnings inline, never a modal wall.
- UX-DR88 "New like this" lives on every session (row + detail + archive browse); the
  template preview shows the actual tree and README skeleton it will create.
- UX-DR89 Lineage renders as a breadcrumb chain on the session header; both directions
  navigable.
- UX-DR90 The promote panel is a two-column workspace→artifacts view with per-row
  actions and staleness badges; the README table is its rendered source of truth.
- UX-DR91 Editing anywhere in a session is the notes editor, unchanged — one editor,
  every surface; capture included.
- UX-DR92 Search, chips, tags, pins, unread and origin glyphs look and behave exactly
  as in notes; a user who knows notes already knows sessions.

### 3.5 Epics — Epic 47 … Epic 49

| # | Epic | Binds |
|---|------|-------|
| 47 | The zone you already have (root flag, index, list+search+spaces+pins, capability) | FR-222–FR-232, NFR-36, AD-107, AD-108, AD-110, AD-113 (partial), UX-DR85, UX-DR86, UX-DR92 |
| 48 | Open it, log it, watch it (detail, editor reuse, live changes, unread/history, capture parity, current session) | FR-233–FR-242, FR-250–FR-252, NFR-37, NFR-39, AD-109, AD-113, AD-114, AD-115, UX-DR89, UX-DR91 |
| 49 | The promote airlock (promote, archive checklist, delete/unarchive, lineage writes, sync honesty) | FR-243–FR-249, NFR-38, AD-111, AD-112, UX-DR87, UX-DR88, UX-DR90 |

## 4. External research pointer

Web research on session/agent-workflow best practices lands as
`_bmad-output/planning-artifacts/research-sessions-2026-08-12.md` when complete; it
informs the PRD's rationale section and may add *later*-scoped items, never renumber
this spine.
