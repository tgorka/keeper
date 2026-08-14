---
topic: keeper sessions — LLM work sessions (manual/hybrid) inside synced 60-sessions zones
source: brainstorm-keeper-sessions-2026-08-12/.memlog.md
date: 2026-08-12
purpose: clean input for bmad-prd and bmad-architecture
---

# Keeper Sessions — Brainstorm Intent

## Problem & jobs

- **See what is being worked on** — active sessions across both drives, with honest
  freshness (what moved, where, by whom), without opening a terminal.
- **Review what the agent did while I was away** — unread marks, per-file history,
  diffs since I last looked; the session surface is a *review* surface first.
- **Edit the record where the work is** — README, artifacts, prompts, refs, with the
  full notes editing experience including quick capture into the session log.
- **Start a session in zero decisions** — from `_template/` or "like session X",
  pre-staged folder, date, slug; a continuation carries a reference both ways.
- **Close a session honestly** — the archive flow runs the zone's own checklist:
  promote what the Promote table names, empty the workspace, file under
  `archive/YYYY/`; a session with nothing worth keeping is offered deletion instead.
- **Find the session from a half-remembered fragment a year later** — search over
  README + artifacts + prompts with the notes query grammar; tags, properties and
  pins for browsing.

## Product intent

Keeper gains a **sessions** surface over the `60-sessions/` zone of folders it already
syncs — the same one-idea as notes: *a sessions root is a folder you already sync, plus a
flag*. A session is a **directory** (`active/YYYY-MM-DD-<slug>/` with `README.md`,
`workspace/`, `artifacts/`, `refs/`, `prompts/`); its README frontmatter carries the
session's identity, tags, properties and pins. Keeper adds a lens and lifecycle verbs —
it never runs the agent, never invents state files cannot express, and never moves a
folder unasked. Manual or hybrid means: the human edits in keeper, the agent edits on
disk, and both see each other's changes live through the existing watcher channels.

## Non-negotiable decisions (from the divergent session)

1. **Reuse the notes machinery wholesale.** The editor (CodeMirror 6 stack, properties
   panel, format toolbar, slash menu, mermaid, viewer registry), quick capture, the live
   change pipeline (list + body channels, diff bar, unread against `headRev`), frontmatter
   tiers with byte-preserving writes, ULID identity, the query grammar/parser, trash-based
   delete, and conflict rows. Sessions add a folder-level domain and lifecycle verbs —
   nothing that duplicates a notes subsystem.
2. **Adopt, never scaffold.** The flag points at an existing subfolder (default
   `60-sessions`); keeper indexes `active/` + `archive/`, skips `_template/`, `_*`, `.*`.
   Creating the zone from nothing is an explicit, confirmed action.
3. **Files are the only truth.** Every visible fact is derivable from files + git:
   freshness = newest mtime split by subtree (workspace vs artifacts — two signals),
   status = folder location (active/archive), lineage = `continues`/`continued-by` id
   references in frontmatter, promotion = the README's `## Promote` table. Any state a
   Finder edit could desync is forbidden.
4. **The promote airlock is the archive flow.** Archive walks a visible checklist:
   parse the Promote table, copy each named `workspace/` source to its `artifacts/`
   target under the sync engine's stability gate, flag staleness (source newer than
   target), warn on hot workspace (changes in the last minutes), empty `workspace/`
   (leave `.gitkeep`), then move to `archive/<year>/`. Nothing worth keeping → offer
   delete instead (zone rule). Unarchive exists as an escape hatch; continuation is the
   preferred reopen.
5. **Identity survives the move.** Session id is a ULID in README frontmatter; links
   within a session are session-relative, cross-zone links repo-root-relative — both
   survive `active/ → archive/YYYY/` untouched.
6. **Workspace is visible but untouchable.** It is gitignored at drive level; keeper
   lists it read-only (fs walk, coarse mtime) so "what the agent is doing right now" is
   on screen — but never commits it, never syncs it, and search excludes it by default.
7. **Capability-gated like notes.** `CapabilitiesVm.sessions`, true only where sync is;
   surfaces are absent rather than broken. Multi-root (tgdrive + neuradrive) scoped as
   `(rootId, sessionId)` exactly like `(vaultId, noteId)`.

## Scope

### Must (this phase)

- Sessions flag on a sync profile (+ subfolder name); sessions index with `.keeper`
  cache; capability plumbing.
- Sessions primary view: list with search (notes grammar + `is:active/archived`,
  freshness sort), tags/properties/pins from README frontmatter, unread + origin glyphs,
  session detail with subtree tree (README/artifacts/refs/prompts + read-only workspace),
  full notes editor on any text file inside the session, live external changes.
- Lifecycle verbs: create from `_template/` or from a previous session (structure-only
  copy + two-way lineage refs), log-today capture into a chosen session, promote a
  workspace file, archive (the airlock checklist), delete (trash), unarchive.
- Menu entries, command palette verbs, quick-capture parity (open session README as
  capture window; capture into session log), default session spaces (active / archived /
  stale / by-origin), pins.
- Linking parity: wikilink a session from a note by id; session README links files by
  the same rules as notes; backlinks across both indexes.

### Later (explicitly out)

- Board/table lenses over session properties (follows notes FR-123/124 when those land).
- Cross-session compare (MLflow-style property tables).
- Any agent execution/orchestration from keeper; transcript ingestion.
- Sessions on mobile; publishing a session into a Matrix room.

## Open questions for the PRD

- Default capture target when multiple sessions are active: most-recently-touched, or an
  explicit "current session" the user sets (sticky per drive)?
- Does the archive flow commit the workspace emptying as one visible operation with a
  summary line, and how loudly does it report parked LFS uploads (existing journal
  surface vs a session-detail banner)?
- `_template/` editing UI: is editing the template just "open the folder's files in the
  editor" (files-are-truth) or does it deserve a guided form? (Lean: files-are-truth.)
- Does `continued-by` get written into an *archived* session's README (a write into
  archive/) or only rendered from the index? (Lean: write it — files are truth.)
