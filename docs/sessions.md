# Sessions

keeper's board over LLM work sessions. One folder per session, inside a folder keeper
already synchronises — the `60-sessions/` zone layout the drives themselves define.

This is the operator document: what is on disk, what keeper reads and writes, what it will
never touch, and what to do when something is wrong. The design reasoning lives in
`_bmad-output/planning-artifacts/` (PRD phase 7, `ARCHITECTURE-SESSIONS-PHASE7.md`,
`EXPERIENCE-SESSIONS.md`); the requirement numbers below point there.

## The one idea

**A sessions root is a folder you already sync, plus a flag.**

There is no picker, no import, no session store and no second place to configure anything.
Settings → Sync → a folder → *This folder has sessions*. keeper lists sessions from a
subfolder of that folder (`60-sessions` by default) and syncs them with everything else
there. keeper **adopts** the layout that is already on disk; it does not create one.

Everything else follows from that: sessions sync because the folder syncs, they have
history because the folder is a git repository, and every visible fact — status,
freshness, tags, lineage — is derivable from the files alone. Anything a Finder edit
could desync, keeper does not store.

## On disk

```text
<your synced folder>/
  60-sessions/                         # the zone — the subfolder you named
    README.md                          # the zone's own contract (the drives wrote it)
    AGENTS.md                          # agent rules for the zone
    _template/                         # the skeleton a new session copies. Yours; keeper
                                       # copies it and never edits it unasked.
    active/
      2026-08-10-keeper/               # one session: YYYY-MM-DD-<slug>
        README.md                      # THE record: summary, decisions, log, promote table
        workspace/                     # scratch — unversioned, unsynced, READ-ONLY to keeper
        artifacts/                     # promoted output — versioned and synced
        refs/                          # inputs worth keeping
        prompts/                       # reusable prompts, numbered
    archive/
      2025/2025-03-01-taxes/           # finished sessions, filed by close year
    .keeper/                           # keeper's own cache + journal + trash. NEVER synced.
```

### What keeper promises

- **`workspace/` is read-only.** keeper lists it for the freshness signal and never
  writes, commits, syncs or searches it. An edit aimed there is refused with the zone's
  own words and a pointer at promotion.
- **`_template/` and every `_*`/dotted name is never a session.** The scan skips them.
- **keeper never moves a session folder unasked.** Archive, delete and unarchive are
  explicit actions; each is a journaled plan whose irreversible step runs last, so a
  crash mid-verb resumes or rolls back and never leaves a half-moved folder.
- **A write that changes one frontmatter key leaves every other byte identical** — the
  notes writer's promise, kept here for pins, lineage and the promote table.

## The session README

The README's frontmatter is the session's identity, tags, properties and pins, under the
notes three-tier contract: keeper-owned (`id` — a ULID minted on first index; `pinned`;
the `keeper:` namespace), Obsidian-native (`tags`), yours (anything else). Lineage lives
one level under `keeper:` as flow lists:

```yaml
keeper:
  session-continues: [01J4…]
  session-continued-by: [01J6…]
```

Status is the folder's location — `active/` or `archive/<year>/` — never a stored flag.
The board's two freshness signals are derived mtimes: **workspace** (the agent is
iterating) and **record** (README/artifacts/refs/prompts changed). They are never merged;
the distinction is the review loop.

## The verbs

- **New session** — one question (the title). The template is copied verbatim, the README
  stamped from the template's own headings with today's date and a minted id, the folder
  named `YYYY-MM-DD-<slug>` with a collision counter.
- **New like this** — a continuation (the zone's preferred reopen). Structure only:
  `prompts/` and `refs/` travel, artifacts and workspace contents never do, and the new
  README is the source's headings, empty. `continues`/`continued-by` are written into
  BOTH READMEs — including an archived source — so the lineage is visible to `cat`,
  Obsidian and the agent, not only to keeper.
- **Log today** — appends `### YYYY-MM-DD — ` under `## Log`, newest last (the zone's
  convention), once per day, and opens the README. `⌘⌥L`, the palette, or the row menu.
- **Pin** — `pinned: true` in frontmatter; pinned sessions sort first in their group.
- **Archive** — per the zone's checklist: workspace emptied (`.gitkeep` stays), folder
  filed under `archive/<current year>/`. Promote what matters first — workspace contents
  do not survive. The confirm dialog says exactly this.
- **Delete** — the folder moves into the zone's `.keeper/trash/<id>/`, workspace and all,
  recoverable. Never an unlink.
- **Unarchive** — one move back to `active/`. Lineage untouched. Prefer a continuation.

## Editing

Opening a session opens its README in the same editor every other keeper surface uses —
markdown with live preview, the raw/rendered toggle, mermaid, tables, embeds. Any text
file in the session opens the same way; `workspace/` files open read-only. An agent
editing a file on disk shows up live: the row updates, and an open buffer takes clean
external writes silently or raises the diff bar when dirty — the notes pipeline,
unchanged.

## When something is wrong

**"No sessions folder yet."** No folder is flagged. Settings → Sync → your folder →
*This folder has sessions*. If the subfolder name differs from `60-sessions`, set it in
the same place.

**The board is missing a session that is on disk.** The scan is a cache and is allowed to
be wrong. Press *Rescan* — a full pass over the zone costs well under the 2 s budget.

**A lifecycle verb refused with "changed while this was being planned".** An agent wrote
the README between planning and writing. Nothing was written; run the verb again — the
refusal is the fence working, not a fault.

**A crash mid-archive.** Relaunch. The journal in the zone's `.keeper/` resumes the
remaining steps; every step is idempotent, and the folder move is always last.

**A deleted session is gone.** It is not: `<zone>/.keeper/trash/<session id>/`.

## What is not here yet

The promote panel (per-row promote review with staleness badges), the full notes query
grammar over sessions (`is:`, `origin:`, `field:` — the board's free-text filter covers
title/path/tags/snippet/log today), unread marks and per-file history projections on
rows, capture-into-session-log, the sticky current session in the tray, and wikilinking a
session from a note. All specified (FR-229/235/236/241/242/250) and scheduled; none
implemented. Sessions on iOS are out of scope with the rest of the sync surface.
