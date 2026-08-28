# Epic 50 — A session note behaves like a note

created: '2026-08-16'
source: the owner's report against the epic-49 build installed on hesperia — three
observations, each measured against their live zone before this spine was written
binds: FR-277–FR-284 (allocated here); FR-233 (whose "full notes editor" claim this epic makes
true), AD-65, AD-113, AD-120 (a file's kind is a tag, never its folder), AD-121
supersedes: story 49.2's note arm and its acceptance sentence — see §2, it describes a
configuration keeper's own validator refuses

## The ask, verbatim

> *w session spaces nie widze przycisku nowych notes (tylko ilosc) - jestes pewien ze kazdy space
> przyjmuje wiecej niz 2 notes?*

> *nie widze tez otwierania w okienku notes*

> *w session templates nie widze mozliwosci dodawania/usuwania/zmiany nazwy plikow/folderow*

## What the machine actually says

Measured on hesperia before planning, not inferred:

- The zone is `/Volumes/merope/tgdrive/60-sessions`, flagged on profile `tgdrive`.
- **The notes vault on that same profile is `10-notes` — a sibling of the zone, not its parent.**
- **The live sessions are FOLDER-shaped**: `active/2026-08-10-keeper/` holds `README.md`,
  `references.md`, `artifacts/`, `prompts/`, `refs/`, `workspace/`. `shape()` returns `Folder`
  because neither `AGENTS.md` nor `about.md` is present (`shape.rs:89-96`).
- The zone's `about` space asks `tag:about tag:recordings` — **two terms**.
- `_template/` holds `README.md`, four `.gitkeep` folders, a stray `.DS_Store`, and one named
  template `test1/`.

### Report 1 — no create button, and "does a space take more than 2 notes?"

| cause | verdict | evidence |
|---|---|---|
| every space's create control is suppressed by the `shape === "flat"` gate story 49.2 added | **deliberate, and treating a symptom** | `session-spaces.tsx:599`; the reason is that `sessions_file_new_kind` writes into the session ROOT (`sessions_ipc.rs:2478-2480`) while a folder-shaped pool reads `README.md` + `refs/` + `prompts/` only (`sessions_root.rs:1068-1089`) |
| `about` is *additionally* suppressed | correct | `creatable_kind` takes one term or nothing (`spaces.rs:511-514`) |
| the counts read 0 | **broken, and not what 49.2 addressed** | the live `README.md` carries no frontmatter at all, so it has no tags, so no space selects it (`pool.rs:253` — kind comes from tags only). Root-level `references.md` is not even in a folder-shaped pool |
| *Add reference* writes `references.md` into the session root **on a folder-shaped session too**, ungated | **broken** | `sessions_ipc.rs:2866-2871` — that file is on the owner's disk now and is invisible to every space and to Unfiled |
| the control is hover-only (`opacity-0 group-hover:opacity-100`) | **broken as discoverability** | it is the house pattern for a *row* (`space-list.tsx:295`), but session create verbs the owner knows are always-visible labelled buttons (`session-file-actions.tsx`) — so "I don't see the button" is literally true |

**The gate was the wrong fix.** "We would write somewhere the pool cannot see" is answered by writing
somewhere it *can* see, not by removing the verb. And the answer is two-part, because **AD-120 says a
file's kind is its tag and never its folder** (`pool.rs:253`): a folder-shaped create must write the
right directory *and* the tag.

### Report 2 — "no opening in a notes window"

**The note arm shipped in story 49.2 can never execute. Not on this machine — on any machine.**

`notePathForFile` resolves only when the vault's subfolder is a component-prefix of the file's path,
i.e. only when the vault *contains* the zone (`rule.ts:145-184`). And `SessionsConfig::validate`
refuses exactly that containment, in either direction, including equality:

> *"sessions subfolder {subfolder} overlaps notes subfolder {vault}: one folder cannot be both a
> vault and a sessions zone"* — `keeper-sync/src/profile/mod.rs:648-654`, reason at `:599-604`:
> *"the notes indexer and the sessions indexer would each claim the same markdown"*

The resolving condition and the refusal are exact complements, pinned by
`sessions_may_not_overlap_notes_or_recordings_in_either_direction` (`mod.rs:1707-1729`). So story
49.2's `openNoteForFile` call, its three matrix rows and its `stillWanted` plumbing exist for a branch
no configuration can reach, and its acceptance sentence — *"open any row in the full note editor when
the zone lives inside a vault"* — **describes a state the product forbids**.

**That refusal has teeth and this epic does not touch it.** Two indexers over one tree would make
every session log a note row, a tag-index member and a backlink source, and would apply vault commit
and trash semantics to `workspace/` scratch that AD-113 fences off from writes entirely.

So what the owner is missing is not a vault membership and not, primarily, a window: it is the
**editor chrome**. Measured against a `kind: "note"` target, a session `.md` on a `kind: "file"`
target has mermaid and a read-only rendered tab, and lacks: the properties panel, the format toolbar,
the slash menu, tag/wikilink/emoji completion, attachments, backlinks, history, the diff bar, and
autosave (`note-editor.tsx:1039,1061,1076,1084,1102,971,951` vs `text-file-frame.tsx:225-240`). FR-233
promises exactly that list by name (`phase-7-sessions.md:190-193`) and it has never been true.

### Report 3 — no add/delete/rename of a template's files and folders

**Absent**, with no recorded refusal to relax: story 49.1's acceptance was list, open, create-named,
rename-the-template. The session file verbs cannot be pointed at a template because all three resolve
through `sessions_root::row_of(root_id, session_id)` and `_template/` is never scanned as a session
(FR-225) — an id-lookup wall, not a path guard. The plan vocabulary already has `MkDir`, `WriteFile`,
`MoveDir`, `TrashDir`, `TrashFile`, `EmptyDirKeep`; **the one missing primitive is `MoveFile`** —
nothing in the crate renames a file. `docs/sessions.md`'s refusal to rename session files is about
link identity (a path *is* the id, so a rename breaks the pins pointing at it) and has **no teeth
inside a template**, which nothing points at and which a create copies rather than references.

## Decisions this epic takes

1. **The overlap validator stays.** Its reason survives scrutiny. Instead the unreachable note arm is
   deleted and the specs that claimed it are corrected — code that claims a behaviour the product
   forbids is worse than code that does not have it.
2. **A folder-shaped create writes the folder AND the tag.** The kind→directory mapping becomes a
   public function in the domain, beside `carried_kind` which already knows the inverse.
3. **`task` has no home in the folder contract**, so a folder-shaped session offers no create on
   Tasks — absent, with the reason in the UI's own words rather than a silent nothing.
4. **The create control becomes always visible**, like the session file verbs the owner already
   knows. Hover-reveal is right for a row's edit/delete and wrong for the one verb a section exists
   to offer.
5. **The chrome comes to the file, not the file to the vault** — and it comes by *moving* the
   extensions into the shared text host, never by copying them, because
   `text-editor-host.ts:8-13` is right that two editor configurations drift.
6. **A capture-style window for a session file is NOT in this epic.** It needs a third
   `CaptureTargetVm` variant and, first, a file autosave the product deliberately does not have
   (`capture-document.tsx:26-30`). Written down so the owner can ask for it knowingly.

## Functional requirements

- **FR-277** A session space offers *New note* on a folder-shaped session, writing into the directory
  that shape's pool reads and stamping the kind tag that makes the space list it.
- **FR-278** A kind with no home in a session's shape offers no create, and says why where the person
  is looking.
- **FR-279** *Add reference* writes where the session's own shape can see it.
- **FR-280** A space's create control is visible without hovering.
- **FR-281** A row in a session space opens the file, and keeps no code path for a vault membership
  the product refuses.
- **FR-282** A markdown file open from a session has the format toolbar, the slash menu and emoji
  completion — one editor host, shared with the note editor.
- **FR-283** A markdown file open from a session has a properties panel over its own frontmatter,
  written byte-preserving and addressed by `(profileId, relativePath)`.
- **FR-284** A template's files and folders can be created, renamed and deleted from the Templates
  room, journaled like every other zone write, with a recoverable trash.

## Stories

- **50.1 — A space you can write into, wherever the session keeps it** (crosses IPC). The public
  kind→directory mapping, a shape-aware `sessions_file_new_kind`, the `Add reference` destination
  fix, the flat-gate removal, the always-visible control, and the honest refusal for `task`. Deletes
  story 49.2's unreachable note arm and corrects its spec. Binds FR-277–FR-281.
- **50.2 — A template's files and folders** (crosses IPC). `PlanStep::MoveFile`, template-scoped
  create/rename/delete for files and folders, and a tree in the Templates room. Binds FR-284.
- **50.3 — One editor host, one set of writing tools** (frontend only). The format toolbar, slash
  menu and emoji completion move from the note editor into the shared text host and appear on any
  markdown file. Binds FR-282.
- **50.4 — A file's own properties** (crosses IPC). A byte-preserving frontmatter write addressed by
  `(profileId, relativePath)`, and the properties panel on a file target. Binds FR-283.

Stack order: 50.1 → 50.2 → 50.3 → 50.4, on top of the epic-49 stack. 50.4 sits on 50.3 because both
change the file panel's frame.
