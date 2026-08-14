---
name: 'keeper'
type: architecture-spine-companion
purpose: build-substrate
altitude: initiative
paradigm: 'hexagonal Rust core + unidirectional view-model projection — unchanged; the flat session is a second on-disk contract read by the same domain, not a second domain'
scope: 'keeper Phase 7 round two — the flat session: one markdown pool per session with kinds declared in frontmatter, spaces as saved queries, a task board, three markdown widgets reusable in notes, file verbs, a reference picker, named templates, search everywhere, and the default template keeper ships'
status: final
created: '2026-08-14'
binds: [FR-253..FR-268]
sources:
  - _bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SESSIONS-PHASE7.md
  - docs/sessions.md
parent: ARCHITECTURE-SESSIONS-PHASE7.md
---

# Architecture Companion — The flat session (Phase 7, round two)

Extends AD-107..AD-115 with **AD-116..AD-121**. Nothing here renegotiates them: core is
still pure and the shell still executes (AD-108), there is still one frontmatter writer
and one query grammar (AD-109), lifecycle verbs are still journaled plans whose
irreversible step is last (AD-111), `workspace/` is still a read-only projection (AD-113),
and session IPC still mirrors notes IPC (AD-114).

**What changed is the contract on disk, and only that.** The first round shipped a session
as a folder of folders — `refs/`, `prompts/`, `artifacts/`, `workspace/` — with the record
in `README.md`. Using it surfaced the structural complaint: a file's kind was decided by
*where it sat*, so moving a file changed what it was, and the operator could not add a
kind without adding a directory. Round two makes a session **one markdown pool** whose
files declare their own kind, and turns the old directories into *queries*.

The one-sentence shape: **a session is a pool of markdown that says what each file is;
keeper reads that pool once and projects it through saved queries — a list, a board, a log
— and the same projections work in any note, because nothing about them is
session-specific.**

## Architecture decisions AD-116 … AD-121

### AD-116 — One create, one picker, and the preview *is* the plan

- **Binds:** FR-253, FR-266; Epic 47
- **Decision.** *New session* and *New like this* collapse into one command over one list
  of **patterns**: the zone's `_template/`, then its named templates (`_template/<name>/`,
  id `_template/<name>`), then every session in the zone newest-change-first. A pattern id
  is resolved once, in `pattern::resolve`, so the "is this a session path?" test is a
  prefix test rather than a `!= "_template"` equality — a named-template id slipping past
  that filter and being opened as a session path is the defect this shape prevents.
- **The preview is not a description of the copy; it is the copy.** `pattern::apply`
  returns both what travels and what stays with a reason for each, and the plan compiler
  consumes the same value. A preview computed by a second rule would eventually disagree
  with the write, and a disagreement here is silent — the operator sees the promise, not
  the result.
- **Consequence.** Anything the copy decision needs must be available to a pure function.
  Kinds are not (see AD-120), so `apply_with_kinds` takes a `kind_of` closure the shell
  fills from the pool it has already parsed, and the default closure answers `None` —
  which leaves loose files behind. That is the conservative direction on purpose: a
  prompt that did not travel is one copy-paste, a log that did travel is a false record.

### AD-117 — A session folder is a small workspace, browsed whole

- **Binds:** FR-254, FR-262; Epic 48
- **Decision.** The detail browses a session with a real tree, ordered by **the zone's own
  contract** (`artifacts/`, `refs/`, `prompts/`, `workspace/`, then everything else) rather
  than alphabetically, and it arrives **fully expanded**. This is the deliberate inverse of
  the Files pane, which loads lazily per expand because one of its folders may be a pendrive
  with a hundred thousand files on it. A session is bounded by its own contract, so
  preloading costs one walk and saves every click; the walk is bounded and reports
  `truncated` rather than showing a prefix of itself.
- Rows carry the sync mark, size and age, and the write fence — read from the same engine
  answers the Files tab reads, because two surfaces disagreeing about one file is worse
  than either being sparse.
- **File verbs are create and delete only.** *New file* (`.md`/`.csv`/`.json`), *New log*
  and *New prompt* write through the plan/exec path; delete moves into `.keeper/trash/`
  and is never an unlink. Rename and move stay unbuilt and stay on
  `FILES_UNBUILT_CONTROL_LABELS`: a file whose identity is its path (AD-120) loses its
  pins on a rename, so renaming is a link-rewriting problem, and half of one is worse than
  none.

### AD-118 — What a session points at is classified in Rust, missing first

- **Binds:** FR-255, FR-265; Epic 48
- **Decision.** References are parsed out of the session's markdown and resolved into six
  kinds — note, recording, file, session, link, missing — each with a real test behind it
  (a recording is a note carrying a `session:` key, not a file with an audio extension;
  a link is reported without being fetched, because a red row that only means "no internet"
  is worse than no row). Missing sorts first and states **both paths keeper tried**.
- `artifacts/` and `workspace/` are not scanned: the first is a deliverable whose internals
  are its own business, the second is scratch that dies with the session. The scan is
  bounded by total text, not file count, and says so when it stops.
- **Adding a reference is a picker over disk, notes and recordings**, searchable by tag,
  writing the pointer in the session's own convention with the kind already decided. A
  target inside `workspace/` triggers an *offer* to copy into `artifacts/` and point there
  instead — an offer and not a rule, because a deliberately temporary pointer is a thing a
  person is allowed to want.

### AD-119 — Shape is decided in Rust, by presence, and migration is a verb

- **Binds:** FR-256, FR-257; Epic 47
- **Decision.** `Shape::{Flat, Folder}` is computed from the session's top-level listing
  alone — **presence of `AGENTS.md` or `about.md`**, never absence of `refs/` and never a
  parse of the record. Absence cannot distinguish "migrated" from "brand new"; a parse
  would let a session flip shape on a prose edit. A folder holding both `README.md` and
  `AGENTS.md` reads **Flat** — the safe direction, because the residual README then shows
  up as an *unfiled* file, making a half-migration visible rather than merely survivable.
  The frontend reads `shape` off the VM and never probes the disk for which record exists
  (AD-65).
- **Migration is `sessions_migrate`, never automatic**, compiled by
  `compile_migrate(&MigrateInput) -> Option<Plan>` — `None` for an already-flat session, so
  idempotence is stated in the type rather than promised in prose. ULIDs and timestamps
  come in from the shell, so a resumed journal replays the ids and filenames it recorded
  (AD-111, AD-56).
- **Step order is the load-bearing part.** Every file the flat shape needs is written
  first; `AGENTS.md` — the shape flip — is written after them; the README becomes a
  three-line signpost via `GuardedWrite` (every link and agent instruction in the
  operator's world points at that filename); the two directory removals run last. There is
  no window in which the session reads as flat but its logs are missing. `TrashDir` is
  emitted only for directories actually present, because it is idempotent on replay but
  errors on a never-existed source — so the guard belongs at compile time.
- `artifacts/` and `workspace/` survive both contracts: they are the two subtrees that are
  not markdown, and the difference between them is versioning, not kind.

### AD-120 — Every file declares its own kind, as a tag

- **Binds:** FR-256, FR-259, FR-268; Epic 47, Epic 48
- **Decision.** In the flat contract a file's kind is a tag in its own frontmatter —
  `about`, `task`, `log`, `prompt`, `ref` — read by `pool.rs` through the **notes** readers
  (`frontmatter::parse`, `tags::note_tags`, `order::read_order`), so hierarchical tags and
  inline `#a/b` come for free and there is still one parser (AD-109). A file declaring none
  of the five is **unfiled**: not an error, and not styled as one, because a hand-dropped
  note is an ordinary thing to do — but it is surfaced, because nothing else would list it.
  `TaskStatus` is a closed set of four, like the `is:` flag set; a fifth value is
  *unreadable*, shown as such rather than quietly filed under "to do".
- **keeper does not stamp.** A file keeper did not author keeps its bytes: no `id` means
  `path:<rel>` identity plus `unstable_identity`, exactly as notes already do. Stamping at
  index time would dirty a git tree the operator did not touch — and it is the reason
  rename stays unbuilt (AD-117).
- **The known cost is stated, not hidden.** A flat session opened in Finder, or handed to
  an agent, is an undifferentiated pile of markdown until something reads the tags. The
  mitigation is `AGENTS.md` at the session root: the navigation contract, written for a
  reader with no other context, shipped as part of the default template (FR-268) and
  **carried by every create** — a template's `README.md`/`about.md` is restamped (copying
  it would name every new session after the template) but its `AGENTS.md` travels
  untouched, because a zone that edited its own contract meant it. Seeds are the opposite:
  a continuation gets the contract and not the seed log, since "Nothing has happened yet"
  atop a session continuing months of work is a false record.

### AD-121 — A space is a file; the board and the log are views of a query

- **Binds:** FR-261, FR-263, FR-264, FR-267; Epic 48
- **Decision.** Space definitions live at **zone** level, one markdown file each under
  `60-sessions/_spaces/` — not per-session (the five are identical for every session;
  per-session means editing one query N times and reintroduces a folder into a shape whose
  point is that there are none) and not built-in-only (a space in keeper is a file you can
  rename, reorder and delete — AD-79). `model::skipped` already hides `_`-prefixed names,
  so `_spaces/` needs no new rule. A zone with **no** `_spaces/` is seeded with the five
  defaults on first read; a zone that *has* the directory owns it, and an empty one stays
  empty — which is what makes a deleted space stay deleted without a ledger file.
- **One evaluator.** `as_index_entry` projects a `PoolEntry` into what `query::eval`
  already takes, and `notes_space_validate`/`notes_space_terms` are pure and vault-free, so
  sessions call them as-is. A `tag:` that meant one thing in notes and another in sessions
  would be a trap. Extending the chip union with an `=`/`!=` field chip repaired a live gap
  in **notes**, where `field:status=open` parsed but could not be edited — one fix, two
  panes. Ordered comparisons stay unrepresentable: a chip that silently widened `>=` to `=`
  would be worse than no chip.
- **The board composes its column queries in Rust** (`<space query> field:status=<v>`,
  AD-65) and a card's position is `order: f64`, whose module header already described
  fractional values as what a future drag-to-reorder would write so that moving one card
  does not rewrite every card after it. This is that future; no new mechanism, and no DnD
  dependency — the hand-rolled HTML5 precedent in `pins-strip.tsx` is followed instead.
- **All three views are also markdown widgets** — `> [!board]`, `> [!log]`, `> [!refs]` —
  usable in any note. **Callouts, not fences**, per the `gallery-block.ts` argument: a note
  carrying one degrades to a labelled quote block in Obsidian or on GitHub rather than a
  wall of grey source. Block decorations come from a StateField, never a ViewPlugin
  (DW-165), and the lazy chunk stays React-free until it is mounted (NFR-27). The board is
  a widget that happens to be useful in a session, not a session feature that leaked.
- **Search is one shortcut pair, not a second search.** `⌘F` finds inside the focused
  document; `⌘⇧F` — already bound to chat search — widens to messages, notes and session
  files, reusing `notes/search.rs::find` over the existing bounded parallel walk streamed
  on a channel, and each hit opens at the same path a file row would (AD-109's one
  file-open path).

## Deferred

Unchanged from AD-107..AD-115's list, minus what round two shipped. Still deferred:
the promote panel with staleness badges, unread marks and per-file history on rows,
capture-into-session-log, the sticky current session in the tray, and wikilinking a session
from a note. Newly deferred by this round: **rename and move** inside the session tree (the
link-rewriting problem above), and any migration in the other direction — flat back to
folder-shaped — for which there is no demand and which would need the same journal built
twice.

## Consistency check

FR-253–FR-268 are implementable within AD-116..AD-121 plus AD-107..AD-115 and the frozen
spine; no PRD amendment required. The two purity gates stay green: the new domain modules
(`shape`, `pool`, `spaces`, `tasks`, `migrate`, `refs`, `search`, `template`, `pattern`) are
tauri-free, clock-free and IO-free, with the one place that needed disk knowledge —
kind-aware copying — expressed as a closure the shell fills (AD-116). No new crates. The
riskiest seams are the shape flip's step order (AD-119) and the preview/plan single-value
rule (AD-116); both are unit-tested against the live zone's real bytes rather than against
a fixture written to agree with them.
