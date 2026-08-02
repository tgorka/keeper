---
topic: keeper note taker — Obsidian-vault markdown notes inside keeper's synced folders
source: brainstorm-keeper-notes-2026-08-02/.memlog.md
date: 2026-08-02
purpose: clean input for bmad-prd and bmad-architecture
---

# Keeper Notes — Brainstorm Intent

## Problem & jobs

- **Catch a thought in under two seconds** without leaving the context I am in — outranks every other job.
- **Give me and my agent one shared brain we both write to, in files**, so neither needs the other's tool.
- **Keep what I write in files I own that are already synced** — no vendor, no export, no lock-in.
- **Find the thing I wrote three months ago in five seconds** from a half-remembered fragment.
- **Turn a message I just received into a note without leaving the messenger** — keeper is the only note app already looking at my conversations.
- **Keep a daily journal going** without ever deciding where today's entry lives.
- **See what changed while I was away** — by whom, on which machine, and whether it was the agent.
- **Park a half-finished thought** in an inbox that is honest about being unfiled.

The keeper-native pair nobody else can offer is *notes beside a Matrix timeline* + *notes on top of a git sync engine that already stamps provenance*.

## Product intent

Keeper gains a markdown note system whose vaults are ordinary Obsidian-shaped directories inside folders keeper already syncs — the feature adds a lens, not a store. **Capture is tray-first**: a global-hotkey panel and a tray menu are the primary surface, the main window is optional for a whole day of use, and nothing is ever filed at creation. **Files are the product**: everything is vanilla frontmatter and standard markdown that Obsidian reads unchanged, and keeper's index is a disposable cache that rebuilds from disk in seconds. **The agent is a co-author, not a background job**: it writes most of the words, so the primary surface is a *review* surface — unread marks, per-note history and inline diffs, never a silent edit. **Organisation is virtual**: tags, frontmatter properties, links and query-defined spaces are the filing system; the physical tree is hidden by default but always one click away. Because nothing is filed at creation, the virtual lens is not a nicety — it is the only organisation that exists.

## Non-negotiable decisions

- **Architecture** — the notes domain (frontmatter, tags, links, templates, naming, index) is pure and lands in `keeper-core`, which stays tauri-free and sync-free. The filesystem watcher, vault IO and git cadence live in the keeper shell over `keeper-sync`. The webview is a pure renderer of Rust-composed note view models; no large payloads over IPC (bodies stream per open note, images go through the existing custom URI scheme).
- **Storage** — a vault is an ordinary Obsidian-shaped directory inside a synced element: `attachments/`, `journal/`, `templates/`, `spaces/`, flat notes at the root. Keeper's index cache lives in `.keeper/` and is never committed. `.obsidian/` is strictly untouched. Keeper never moves a file the user did not ask it to move.
- **Identity** — every note carries a ULID id in frontmatter so links, pins and history survive a rename; the filename stays human and Obsidian-native (first line is the title, slug with date prefix, collisions append a counter).
- **Concurrency** — never lock a note; watch it. A clean buffer applies the external write live with a fading highlight; a dirty buffer merges non-overlapping hunks and raises an inline diff bar. Never a modal. Conflicts reuse keeper's existing surviving-copies model and appear as first-class rows in the note list, not litter on disk.
- **Linux parity** — the tray is the primary surface, so the macOS-template tray glyphs and the swap-the-whole-menu tray strategy must be fixed for Linux **before** notes can rely on them. No macOS-only affordance may be load-bearing.

Licensing is already cleared: mermaid (MIT) and CodeMirror 6 (MIT) pass cargo-deny; no AGPL/GPL.

## Scope

### Must (this phase)

- Notes flag on a sync element + vault model in Rust.
- Vault writer with journal / dated-inbox naming rules.
- Global-hotkey quick-capture panel (plain textarea, paints instantly; buffer survives dismissal and process restart).
- Tray menu: New Note, Today's Journal, last five touched notes.
- Note list with tag and space filtering.
- Live-preview markdown editor with frontmatter properties (source revealed only on the active line).
- External-write live refresh.
- Auto-sync cadence for notes elements (on by default).
- Multi-vault switcher (switching is a filter change, not a reload).
- Wikilink + tag autocomplete; templates + journal; mermaid; attachment paste + local-file links.
- Agent-change marks with per-note history diff.

### Should

- Table and board lenses over frontmatter; saved views.
- Backlinks pane; attachments browser.
- Sticky torn-off note windows.
- Capture-from-chat-message.

### Could

- Calendar lens; transclusion; graph view; publish-note-to-room.

### Won't (this phase)

- Vault encryption; a real full-text engine (tantivy); plugin API; mobile notes surface.

## Key insights that must survive into the PRD

- **Agent cohabitation is nearly free.** The most differentiating cluster — unread agent marks, per-note history, blame, "what did the agent change" — is a projection of commit provenance the sync engine already writes. Build it as a view over existing data, not as new machinery.
- **Capture-with-no-filing and virtual-organisation are one idea.** Retro-filing from the same panel that captured the thought is the bridge; the two must be specced together or both degrade.
- **Vault-as-sync-element deletes an entire settings surface.** No vault picker, no path validator, no migration story, zero new configuration. Do not reintroduce them.
- **The breakthrough wildcard: the tray glyph gains a subtle dot when the agent has touched notes you have not read.** It makes an invisible collaborator visible, one click shows the diff, Accept clears the dot. No existing note app does this — treat it as a headline requirement, not polish.
- **One Feature Only fixes epic ordering.** The first user-visible story is capture-to-file; the vault model exists only to make that story honest. Backcast: mark a sync element as notes → model the vault in Rust → hotkey + always-on-top webview → vault writer → sync cadence → per-file commit log → dot and diff.
- **Search is not an engine.** A bounded parallel scan over a personal vault answers in tens of milliseconds and is never stale; the index is a cache you can delete.
- **Reuse, don't reinvent.** One Rust-composed note view model feeds list, palette and tray. Every note action goes in the existing palette-and-native-menu registry. Attachments use the existing custom URI scheme. A notes capability gate lets shells omit the surface without dead code.
- **Failure modes to design against:** lossy export (there is nothing to export), unreconciled conflict duplicates, capture buffers lost on accidental dismissal, tags that cost a dialog, virtual rows that hide the real path, watchers that miss editor-atomic renames (treat rename-into-place as a modification; reconcile on window focus).

## Open questions for planning

- What is the exact Linux tray remediation scope, and is it a blocking predecessor epic or a parallel track?
- What is the debounce/coalesce/push cadence contract (idle commit at ~2s, push timer, force-flush on hide and quit) and is it per-vault configurable?
- What frontmatter key namespace does keeper claim, and how do we guarantee Obsidian tolerates it across versions?
- How is a space query expressed in a plain note such that it is both agent-writable and safely evaluable?
- What is the merge rule when the agent and the user touch *overlapping* hunks — does it fall through to keeper's conflict rows?
- At what vault size does the bounded parallel scan stop meeting the five-second retrieval job, and what is the measured ceiling?
- Does capture-from-chat-message need a Matrix-side source-link format now (forward-compatible) even though the feature is Should?
- What is the review UX when the agent has touched many notes at once — per-note diffs, or a batched activity feed?
