# Epic 51 — A session that behaves like the rest of keeper

created: '2026-08-16'
source: the owner's fifth field report, filed against the epic-50 build installed on hesperia —
fourteen items, each measured against their live zone before this spine was written
binds: FR-285–FR-299 (allocated here); AD-119/AD-120/AD-121 (the flat contract), AD-88 (one write
path per surface), AD-111 (a plan is replayable), AD-113 (`workspace/` is fenced)
pushes back on: item 8 (`about.md` → `README.md`) — **unsafe as asked**; see §Push-back
supersedes: nothing. Two items turn out to be already-shipped features the owner cannot reach.

## The fourteen, triaged

Five read-only scouts checked every item against the code. **Four are not what they look like.**

| # | the ask | verdict | the actual cause |
|---|---|---|---|
| 1 | a real note editor: preview, source, **note** | **absent**, refusal has partial teeth | The Preview tab *already mounts the note editor's live-preview layer* (`markdown-preview.ts:126`) — clamped read-only at `:118-123`. The third mode is two `EditorState` facets and a text-feedback path, not a new renderer |
| 2 | per-space default fold + how many rows before a scrollbar | **absent** | A `_spaces/*.md` carries six keys and no fold, no cap. **Trap:** `render_edit` replaces the whole `keeper:` map (`spaces.rs:600`), so a key not threaded through `SpaceEdit` is destroyed on first Save |
| 3 | cannot create a folder in a session | **absent + unreachable** | `files::compile_new` already mints a parent directory (`files.rs:334-339`); nothing can send it a nested path — the dialog's Folder field is a `<select>` over existing folders |
| 4 | a `spaces/` directory holding created `.md` files | **(a) already done at zone level; (b) would ship invisible files** | Definitions live at `_spaces/` (AD-121, which refused per-session spaces *by name*). And the flat pool walks **one level, files only** (`sessions_root.rs:1055-1072`) — a create into `spaces/` is in no pool, no space, not even *Unfiled* |
| 5 | About has no *New note* | **deliberate, three independent teeth** | Its query has two terms, so `creatable_kind` refuses first; then About is refused; then `kind_dir` refuses. A second record gives `shape()` two answers. The defect is that the UI says **nothing** instead of why |
| 6 | no right-click menu in spaces | **absent** | No `ContextMenu` anywhere in `src/components/sessions/`. **This app has no tabs** — "open in new tab" has no referent; the verb is *Open in a new panel* |
| 7 | References shows nothing from spaces | **broken, plus a correct-but-confusing count** | The panel scans link *syntax*; the space runs `tag:ref` over the pool. 0 is *correct* under AD-120 — those `refs/*.md` carry no tag. The real bug: the owner's root `references.md` is in **neither** reader |
| 8 | rename `about.md` → `README.md` | **unsafe as asked — see §Push-back** | `shape()` keys on the presence of `about.md`. Adding `README.md` to that predicate flips **every folder-shaped session to flat in one rescan** |
| 9 | default `artifacts/` + `workspace/` everywhere | **absent for templates** | `zone_skeleton` writes two files and zero directories; a create forces them only for `PatternKind::Session` |
| 10 | tasks: drag-and-drop instead of a dropdown | **already shipped; the BOARD is unreachable** | `task-board.tsx:191-210` has full DnD. `session-detail.tsx:526` renders the board **only for a flat session** — his are folder-shaped, so he has never seen it |
| 11 | changing `title` does not rename the file | **broken, and wider than reported** | Nothing anywhere renames on a title change — and `notesRename` (FR-97) is **shipped with zero call sites**. The recorded refusal's *pins* half has no teeth (a session file has no pins); its *link-rewriting* half does |
| 12 | a template does not define spaces | **absent, and permitted** | AD-121 refused *per-session* spaces; a template **seeding the zone's** `_spaces/` is not refused. `template.rs` never mentions `_spaces` |
| 13 | log files in a `log/` directory | **deliberate** | Same request as 4(b) with the same enabling change. `kind_dir(Flat) → root` is the flat contract's premise |
| 14 | document a template's placeholders | **absent — the feature, not just the docs** | keeper substitutes **nothing** into a copied template. A `{{token}}` engine exists but is notes-only (`notes/templates.rs`) |

## Push-back: item 8 is unsafe in the form asked

`shape()` is `has(AGENTS.md) || has(about.md) → Flat, else Folder` (`shape.rs:84-88`).

- **Add `README.md` to that predicate and every folder-shaped session in the zone flips to Flat on the
  next rescan.** Their `## Log` disappears from the log view, `refs/` and `prompts/` stop being read,
  every pointer in them goes invisible, and `sessions_log_today` keeps appending to a README nothing
  parses. That is the owner's live, unmigrated data.
- **Keep `README.md` out of the predicate** and detection rests on `AGENTS.md` alone. Survivable, but
  it deletes the fallback `shape.rs:78-80` bought deliberately ("a hand-built flat session may start
  with either"), promotes `AGENTS.md` to **mandatory**, and requires rewriting `migrate`, whose whole
  rule is *"It does not delete the README … it is rewritten into a three-line signpost instead"*
  (`migrate.rs:33-37`) — the signpost and the record would become the same file.
- It also reverses a decision the owner himself gave: *"**Why no README.** The operator's instruction
  was explicit: the navigation file is `AGENTS.md`, and a README beside it would be a second answer to
  the same question."* (`template.rs:29-32`, pinned by a test).

**So this epic does not build item 8.** Two options, both cheap to state:
1. **Uniformity the other way** — *Convert to flat* already makes every session agree, on `about.md`.
2. **Do it properly as its own epic** — the safe form, plus a rewritten `migrate`, plus a zone
   migration renaming `about.md` in each live session, plus the contract promotion written down.

Item 13 gets the same treatment in miniature: this epic makes a `log/` directory **work if the
operator makes one** (51.1), and does not make `kind_dir` file logs there — that would cost the
board-row log probe, the migration output, and the flat contract's own premise, to buy only that
keeper does the filing.

## Functional requirements

- **FR-285** A flat session's pool finds markdown in subdirectories, budgeted, skipping `artifacts/`,
  `workspace/` and dotted directories — so `spaces/`, `log/` and any folder the operator makes are
  real homes, with the tag still deciding the kind (AD-120).
- **FR-286** A folder-shaped session's pool also reads root markdown, so a file written there is
  visible to spaces and to *Unfiled* instead of invisible to both.
- **FR-287** A folder can be created inside a session, refusing `workspace/` and traversal.
- **FR-288** Every template keeper writes, and every session created from any template, has
  `artifacts/` and `workspace/`.
- **FR-289** A space declares whether it arrives folded, per space, in its own file.
- **FR-290** A space declares how many rows it shows before the rest fold behind a *Show more*.
- **FR-291** A template can carry space definitions that a create seeds into the zone's `_spaces/`.
- **FR-292** A template's markdown may use the notes template vocabulary, expanded on create.
- **FR-293** The Templates room states which placeholders exist and what they mean.
- **FR-294** A markdown file opens in three modes: Preview, Source and **Note** — the live-preview
  editor over the same buffer and the same explicit Save.
- **FR-295** A row in a space has the same menu a file row has in Files, worded for panels.
- **FR-296** Changing a file's `title` renames the file, rewriting the pointers that would break.
- **FR-297** `notesRename` becomes reachable for a vault note.
- **FR-298** The About space says why it offers no create, and offers to open the record instead.
- **FR-299** The task board is reachable from a folder-shaped session.

## Stories

- **51.1 — Markdown is found wherever it sits** (Rust). FR-285, FR-286. Enables 4(b), 7, 13, 10.
- **51.2 — A folder you can make, and the two every session gets** (crosses IPC). FR-287, FR-288.
- **51.3 — A space that says how it opens and how much it shows** (crosses IPC). FR-289, FR-290.
- **51.4 — A template that defines spaces, and placeholders that mean something** (crosses IPC).
  FR-291, FR-292, FR-293.
- **51.5 — Note mode** (frontend). FR-294.
- **51.6 — A row you can right-click, and a title that renames its file** (crosses IPC). FR-295,
  FR-296, FR-297.
- **51.7 — About explains itself, and the board comes to the folder shape** (crosses IPC). FR-298,
  FR-299.

Stack order 51.1 → 51.7. 51.7 needs 51.1's pool; everything else is independent but shares
`session-spaces.tsx`, so the branches stay linear.
