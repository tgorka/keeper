---
title: "Product inputs — Notes phase (Phase 5)"
status: final
created: 2026-08-02
sources:
  - _bmad-output/brainstorming/brainstorm-keeper-notes-2026-08-02/.memlog.md
  - _bmad-output/brainstorming/brainstorm-keeper-notes-2026-08-02/brainstorm-intent.md
---

# Product inputs — Notes (Phase 5)

Stakeholder ask, verbatim intent, and the numbering spine every Phase 5 document binds to.
This file is the contract between the PRD, the architecture amendment, the experience
extension and the epics: **numbers are allocated here and nowhere else.**

## 1. Stakeholder ask (owner, 2026-08-02)

> Note taker — take examples from Obsidian, Apple Notes and Raycast Notes.
> Store notes as `.md` files in Obsidian vault format inside the synced stores; the main
> notes go in a `notes` subfolder of the synced place, and the sync gains an option marking
> that element as *notes*. keeper works with multiple note stores with an easy switch.
> Organisation similar to the `make.md` Obsidian plugin. Journal and templates like Obsidian.
> Easy to create a new note at any time, and a handy note taker while working on the desktop
> (Raycast-notes sticky, or Apple Notes but cleverer). Organisation over tags (Apple-Notes
> style tag filtering) and note metadata fields (Obsidian style). Physically it may be stored
> differently — even flat — but there must be a user interface to see it. Ability to link
> notes, or files from the same sync folder. Sync auto-syncs note changes (may be
> time-based). Focus on usability; it must be handy. Reuse the menu icon. Support mermaid.
> An agent must be able to write in these notes (over `.md` files) and the user sees the
> changes.

## 2. Upstream synthesis

The full divergent session (163 logged entries, 15 techniques) is in
`_bmad-output/brainstorming/brainstorm-keeper-notes-2026-08-02/.memlog.md`; the distilled
input is `brainstorm-intent.md` in the same folder. Everything below derives from those.

Five verdicts carry the phase:

1. **Architecture** — the notes *domain* is pure and lands in `keeper-core` (tauri-free,
   sync-free). Vault IO, the filesystem watcher and the git cadence live in the `keeper`
   shell over `keeper-sync`. The webview stays a pure renderer of Rust-composed view models.
2. **Storage** — a vault is an ordinary Obsidian-shaped directory inside a folder keeper
   already syncs. `.obsidian/` is untouched, keeper never moves a file unasked, and keeper's
   index cache lives in `.keeper/` and is never committed.
3. **Identity** — a ULID `id` in frontmatter, so links, pins and history survive a rename.
   Filenames stay human and Obsidian-native.
4. **Concurrency** — never lock a note; watch it. Clean buffer applies an external write
   live; dirty buffer merges non-overlapping hunks and raises an inline diff bar. Never a
   modal.
5. **Linux parity** — the tray is the primary surface. The macOS-template tray glyphs and
   the replace-the-whole-menu tray strategy are Linux defects and are fixed *before* notes
   rely on them.

## 3. Numbering spine (allocated here; do not renumber downstream)

Prior phases end at FR-93, NFR-26, AD-53, UX-DR34, Epic 34.

### 3.1 Functional requirements — FR-94 … FR-124

| id | requirement |
|---|---|
| FR-94  | A sync profile can be marked as a **notes vault**: a `notes` option on the profile naming the vault subfolder (default `notes/`). Flagging is the only configuration a vault needs. |
| FR-95  | **Multi-vault.** Every notes-flagged profile is a vault. keeper lists them and switches the active vault as a filter change, not a reload. |
| FR-96  | **Vault index.** keeper scans the vault and builds a disposable index of notes, frontmatter, tags, links and attachments. Deleting the cache rebuilds it from disk. |
| FR-97  | **Note identity.** Every note keeper writes carries a ULID `id` in frontmatter; links, pins, unread marks and history survive a rename. |
| FR-98  | **Create a note.** The first line is the title; the filename is `YYYY-MM-DD-<slug>.md`; a collision appends a counter. No dialog, ever. |
| FR-99  | **Journal.** `journal/<YYYY>/<YYYY-MM-DD>.md` created on demand from the journal template; a "Today" action opens or creates it. Path template configurable per vault. |
| FR-100 | **Templates.** `templates/*.md` applied at creation, with placeholder expansion (date, time, title, cursor position). |
| FR-101 | **Quick capture.** A global hotkey raises an always-on-top capture panel focused in the text area. Escape saves and hides. The buffer survives dismissal and process restart. |
| FR-102 | **Tray.** The menu-bar icon offers New Note, Today's Journal and the last five touched notes, and shows an indicator when notes changed by another origin are unread. |
| FR-103 | **Filtered list.** The note list filters by text query, tag chips (intersection), space, date range, origin ("changed by agent") and pinned. |
| FR-104 | **Tags.** Tags come from frontmatter `tags` and from inline hierarchical `#a/b` tags in the body; a tag tree with counts drives the filter. |
| FR-105 | **Spaces.** A space is a virtual folder defined by a saved query, stored as a note under `spaces/` so it syncs, diffs and is agent-editable. |
| FR-106 | **Physical tree.** The real folder structure is always available as an alternative lens, and any row can reveal its real path. |
| FR-107 | **Editor.** Live-preview markdown: source is revealed only on the line under the cursor. Frontmatter renders as a typed properties panel. |
| FR-108 | **Wikilinks.** `[[note]]` with autocomplete and create-on-Enter; a backlinks list at the foot of the editor. |
| FR-109 | **File links.** A note can link to a file elsewhere in the same synced folder; keeper opens or reveals it. |
| FR-110 | **Attachments.** Pasting or dropping an image writes it into `attachments/` and embeds it; assets are served over a `keeper-note://` scheme, never base64 over IPC. |
| FR-111 | **Mermaid.** ` ```mermaid ` blocks render inline; a broken diagram renders its error inline and never blanks. |
| FR-112 | **External writes.** A write by an agent, another editor or another machine is detected and applied live. A dirty buffer merges non-overlapping hunks and raises an inline diff bar. |
| FR-113 | **Agent-change review.** Notes changed by a non-local origin since last read are marked unread; the editor offers a diff of that change; Accept clears the mark. |
| FR-114 | **Note history.** Per-note commit history projected from the sync engine, carrying its provenance (device, origin, source), with a diff per revision. |
| FR-115 | **Auto-sync cadence.** Notes vaults sync themselves: idle-debounced local commit, interval or on-blur push, force-flush on window hide and on quit. On by default for notes profiles. |
| FR-116 | **Conflicts.** A conflict is a first-class row in the note list and resolves inside the editor, never as litter the user must find on disk. |
| FR-117 | **Actions.** New Note, Quick Capture, Today's Journal, Open Note…, Search Notes and Switch Vault are declared once in the action registry, so palette, cheat sheet and native menu cannot drift. |
| FR-118 | **Search.** A bounded parallel content scan over the active vault with match highlighting; never stale, no separate index to invalidate. |
| FR-119 | **Pin / archive.** `pinned` in frontmatter floats a note to the top of the list; archive moves it out of the default lens without deleting it. |
| FR-120 | **Vault settings.** Journal path template, default template, capture destination and sync cadence are per-vault settings. |
| FR-121 | **Obsidian coexistence.** `.obsidian/` is never read or written; keeper never moves a file the user did not ask it to move; `.keeper/` is added to the vault's ignore rules so the index never syncs. |
| FR-122 | **Capability gate.** A `notes` capability flag; shells without folder sync (iOS) omit the surface entirely rather than rendering it dead. |
| FR-123 | **Table / board lens.** The same note set rendered as a table whose columns are frontmatter fields, or grouped into a board by one field. |
| FR-124 | **Sticky note.** A note can be torn off into a small always-on-top window; several may live at once. |

FR-123 and FR-124 are the phase's *Should* tier — planned and specified, scheduled last.

### 3.2 Non-functional requirements — NFR-27 … NFR-30

| id | requirement |
|---|---|
| NFR-27 | Quick capture is visible and focused within **300 ms** of the hotkey on a warm process, and the first keystroke is never dropped. |
| NFR-28 | A **10 000-note** vault indexes cold in under 5 s, the list paints in under 100 ms, and steady-state watch cost is one `lstat` per changed path — not per note. |
| NFR-29 | An external write to a note is reflected in the UI within **1 s**. |
| NFR-30 | No keeper code path deletes or overwrites a note body without leaving a recoverable copy (a commit or a conflict copy). Data loss is the one unacceptable failure. |

### 3.3 Architecture decisions — AD-54 … AD-63

| id | decision |
|---|---|
| AD-54 | **Vault = notes-flagged `SyncProfile` + a subfolder.** No second configuration store, no vault picker, no path validator, no migration. |
| AD-55 | **`keeper_core::notes` is pure.** Frontmatter parse/serialise, filename/slug rules, tag tree, link graph, template expansion, the space query language and the index model live there. No filesystem, no `gix`, no `tauri` — the `check:core-tauri-free` / `check:core-sync-free` gates stay green. |
| AD-56 | **Vault IO and watching live in the shell.** A `notes_vault` module in the `keeper` crate performs reads/writes and hosts the watcher, reusing `keeper-sync`'s watcher and stability primitives, and feeds the pure core plain inputs. |
| AD-57 | **The index is a cache, not a database.** In-memory, rebuilt from disk, persisted only as an advisory `<vault>/.keeper/index.json` for fast cold start. A corrupt or absent cache is a rescan, never an error. |
| AD-58 | **Bodies stream, lists project.** The note list carries view models only; a note body streams over a `Channel` when opened. Nothing large crosses IPC as JSON. |
| AD-59 | **`keeper-note://` serves vault assets**, cloned from `media_protocol.rs`, with a mandatory canonicalise-and-contain check against the vault root before any read. |
| AD-60 | **Quick capture is a second window** declared statically in `tauri.conf.json` (`label: "quick-capture"`, hidden, always-on-top, undecorated, skip-taskbar) with its own least-privilege capability file. Rust drives show/hide/position; the webview asks for nothing. |
| AD-61 | **Tray items live in the first-built menu.** A Linux tray menu cannot be replaced once set, so every notes affordance is present in the menu built at tray creation, and the glyph set gains a Linux-visible variant selected by target. |
| AD-62 | **Cadence is a profile knob.** The existing 1 Hz supervisor consumes a per-profile cadence; notes profiles default to a short one. No second scheduler. |
| AD-63 | **Provenance is the audit log.** "Who changed this note" is read from the sync engine's existing commit trailers. keeper adds no parallel history store. |

### 3.4 Experience decisions — UX-DR35 … UX-DR44

| id | decision |
|---|---|
| UX-DR35 | Capture never blocks the first keystroke: no title prompt, no folder prompt, no save button, anywhere in the feature. |
| UX-DR36 | Notes is a top-level view inside the existing three-pane frame; the vault switcher occupies the position and affordance of the account switcher. |
| UX-DR37 | The filtered list is the primary surface and the editor is secondary; filters are chips, and any filter is one keystroke from becoming a space. |
| UX-DR38 | Virtual organisation is the default lens; the physical tree is always one click away and any row can reveal its real path. |
| UX-DR39 | An agent change is never silent: a dot on the tray glyph, an unread mark on the row, a diff in the editor. |
| UX-DR40 | Live preview is the only editing mode; source appears on the active line. There is no preview toggle as the primary affordance. |
| UX-DR41 | A lens or filter change is a filter, never a navigation: the note under the cursor survives it. |
| UX-DR42 | Notes actions are declared once in the action registry so the palette, the ⌘? cheat sheet and the native menu cannot drift (the UX-DR15 rule, applied to notes). |
| UX-DR43 | Linux parity is a first-class acceptance criterion: every tray-reachable notes affordance exists in the first tray menu and is legible on a dark panel. |
| UX-DR44 | Mermaid diagrams and images degrade to their source text, never to an empty box. |

### 3.5 Epics — 35 … 39

| epic | title | binds |
|---|---|---|
| 35 | The vault is a folder you already sync | FR-94–97, FR-121–122, NFR-28, AD-54–57 |
| 36 | Capture in two seconds | FR-98–102, FR-117, FR-120, NFR-27, AD-60–61 |
| 37 | A place to read and write | FR-103–111, FR-118–119, FR-123, AD-58–59 |
| 38 | Your agent writes here too | FR-112–114, FR-116, NFR-29–30, AD-63 |
| 39 | Notes that sync themselves | FR-115, FR-124, AD-62, plus docs and phase acceptance |

## 4. Out of scope this phase

Vault encryption; a real full-text engine (tantivy) — revisit past 10 000 notes; a plugin
API; notes on the phone shell; publishing a note into a Matrix room; transclusion; a graph
view; calendar lens.

## 5. Licensing pre-clearance

`mermaid` (MIT) and CodeMirror 6 (MIT) both pass the cargo-deny / JS licence firewall. No
AGPL or GPL dependency is introduced. `ulid` is already a `keeper-core` dependency.
