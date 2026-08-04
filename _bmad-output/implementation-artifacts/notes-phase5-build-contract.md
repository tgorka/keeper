# Phase 5 build contract

The exact seam signatures every Phase 5 worker codes against. The architecture companion
(`_bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-NOTES-PHASE5.md`)
is the design; this file is the *linker*. Where the two disagree, this file wins, because it
is what the compiler will check.

## Scope actually being built in this pass

Epics 35, 36, 37, 38, 39 **minus** the two Should-tier requirements the PRD already defers:
FR-123 (table/board lens) and FR-124 (sticky windows). Everything else in the phase is in.

## 1. `keeper_core::notes` — the pure domain (crate `keeper-core`)

`keeper-core/src/lib.rs` gains `pub mod notes;` in alphabetical position.

### `notes/mod.rs`

```rust
pub mod frontmatter;
pub mod index;
pub mod links;
pub mod naming;
pub mod query;
pub mod search;
pub mod tags;
pub mod templates;
pub mod vm;

/// Everything the notes domain can refuse to do.
#[derive(Debug, thiserror::Error)]
pub enum NotesError {
    #[error("note frontmatter is malformed at line {line}: {reason}")]
    Frontmatter { line: usize, reason: String },
    #[error("space query: {message}")]
    Query { message: String, token_index: usize },
    #[error("template: {0}")]
    Template(String),
    #[error("invalid note name: {0}")]
    Name(String),
    #[error("no such note: {0}")]
    NotFound(String),
    #[error("vault {0} is not indexed")]
    VaultUnknown(String),
}
```

`keeper-core/src/error.rs`'s `CoreError` gains exactly one variant:

```rust
#[error(transparent)]
Notes(#[from] crate::notes::NotesError),
```

and `to_ipc_error` in `keeper/src/ipc.rs` maps it: `Frontmatter | Name | Query | Template`
=> `IpcErrorCode::InvalidInput`, `NotFound | VaultUnknown` => `IpcErrorCode::Internal`.

### `notes/frontmatter.rs`

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue { Str(String), Num(f64), Bool(bool), List(Vec<FieldValue>), Map(Vec<(String, FieldValue)>) }

/// Parsed frontmatter that REMEMBERS its source bytes, so a write that touches
/// one key leaves every other byte identical (the FR-121 guarantee).
#[derive(Debug, Clone, Default)]
pub struct Frontmatter { /* private */ }

impl Frontmatter {
    /// `(frontmatter, body_offset)`. No leading `---` block => empty frontmatter, offset 0.
    pub fn parse(source: &str) -> (Frontmatter, usize);
    pub fn get(&self, key: &str) -> Option<&FieldValue>;
    pub fn keys(&self) -> impl Iterator<Item = &str>;
    /// Splice a key into the ORIGINAL source, preserving every other byte.
    /// Returns the whole new document.
    pub fn set_in(source: &str, key: &str, value: FieldValue) -> String;
    pub fn remove_in(source: &str, key: &str) -> String;
    /// Render a fresh block for a note keeper authors.
    pub fn serialise_new(pairs: &[(String, FieldValue)]) -> String;
    pub fn as_string(&self, key: &str) -> Option<&str>;
    pub fn as_bool(&self, key: &str) -> Option<bool>;
    pub fn as_list(&self, key: &str) -> Option<Vec<String>>;
}
```

### `notes/naming.rs`

```rust
pub fn slug(title: &str) -> String;                       // NFC, folded, 60-char cap
pub fn note_filename(title: &str, date: &str, taken: &[String]) -> String; // YYYY-MM-DD-slug[-2].md
pub fn title_from_body(body: &str) -> String;             // first non-empty line, # stripped
pub fn journal_path(template: &str, y: i32, m: u32, d: u32) -> String;
pub const DEFAULT_JOURNAL_TEMPLATE: &str = "journal/{yyyy}/{yyyy}-{mm}-{dd}.md";
```

### `notes/tags.rs`

```rust
pub fn normalise(tag: &str) -> Option<String>;
pub fn inline_tags(body: &str) -> Vec<String>;   // skips fences, inline code, URLs, \#
pub fn note_tags(fm: &Frontmatter, body: &str) -> Vec<String>;  // union, normalised, sorted
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TagNode { pub name: String, pub path: String, pub count: u32, pub children: Vec<TagNode> }
pub fn tag_tree<'a>(all: impl Iterator<Item = &'a [String]>) -> Vec<TagNode>;
```

### `notes/links.rs`

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct RawLink { pub target: String, pub alias: Option<String>, pub embed: bool, pub span: (usize, usize) }
pub fn extract(body: &str) -> Vec<RawLink>;   // [[x]], [[x|y]], ![[x]], [t](rel/path)
```

### `notes/templates.rs`

```rust
/// `(expanded_text, cursor_byte_offset)`. Closed placeholder set:
/// {{date:FMT}} {{time:FMT}} {{title}} {{cursor}} {{id}}. Unknown => left literal.
pub fn expand(template: &str, ctx: &TemplateCtx) -> (String, Option<usize>);
pub struct TemplateCtx { pub title: String, pub id: String, pub now_local: String /* RFC3339 */ }
```

### `notes/index.rs`

```rust
pub const INDEX_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    pub id: String, pub path: String, pub title: String,
    pub size: u64, pub mtime_ns: i128, pub ino: u64,
    pub created_ms: i64, pub updated_ms: i64,
    pub tags: Vec<String>,
    pub fields: BTreeMap<String, String>,   // stringified FieldValue for querying
    pub links: Vec<String>,
    pub flags: Vec<String>,                 // pinned|archived|conflict|template|space|capture|unstable_identity|unparsed
    pub snippet: String,
}

#[derive(Debug, Clone, Default)]
pub struct IndexSnapshot { /* private */ }
impl IndexSnapshot {
    pub fn entries(&self) -> &[IndexEntry];
    pub fn by_id(&self, id: &str) -> Option<&IndexEntry>;
    pub fn by_path(&self, path: &str) -> Option<&IndexEntry>;
    pub fn tag_tree(&self) -> Vec<TagNode>;
    pub fn backlinks(&self, id: &str) -> Vec<&IndexEntry>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}

/// Additive, approved 2026-08-02 (Main): the frozen `IndexEntry` has no slot for
/// provenance or a per-device touch time, but `origin:` and `date:touched` are both in
/// the predicate table, so three reserved `fields` keys carry them. Namespaced under
/// `keeper.` so they can never collide with a user's own frontmatter. Absent
/// `keeper.origin` reads as `local`; absent `keeper.touched` degrades `date:touched` to
/// `modified`. The reconciler writes them through these consts, never as literals.
pub const FIELD_ORIGIN: &str = "keeper.origin";
pub const FIELD_DEVICE: &str = "keeper.device";
pub const FIELD_TOUCHED: &str = "keeper.touched";
/// The separator `FieldValue::index_string` uses to flatten a list field. A newline,
/// because a frontmatter scalar is single-line by construction and so can never contain
/// one — whereas a comma appears inside real values ("Doe, Jane").
pub const FIELD_LIST_SEPARATOR: char = '\n';

impl IndexEntry {
    pub fn has_flag(&self, flag: &str) -> bool;
    /// Every key something can link to this note by: id, path, path sans `.md`, filename
    /// stem, title — all folded. One definition, shared by the alias map, the backlink
    /// lookup and `query::bind_index`.
    pub fn link_keys(&self) -> std::collections::BTreeSet<String>;
}

impl IndexSnapshot {
    /// Additive: resolve a raw link target (title, alias or vault-relative path) to the
    /// note it names. Ambiguity resolves by path order rather than reporting an error.
    pub fn resolve_link(&self, target: &str) -> Option<&IndexEntry>;
}

#[derive(Debug, Clone)]
pub enum NoteDelta { Upsert(IndexEntry), Remove { path: String }, Rescan }

#[derive(Debug, Default)]
pub struct IndexBuilder { /* private */ }
impl IndexBuilder {
    pub fn new() -> Self;
    pub fn from_entries(entries: Vec<IndexEntry>) -> Self;
    pub fn apply(&mut self, delta: NoteDelta);
    pub fn snapshot(&self) -> std::sync::Arc<IndexSnapshot>;
}

/// The `.keeper/index.json` document. Serde only — the shell does the IO.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexCache { pub schema: u32, pub vault_id: String, pub built_ms: i64, pub entries: Vec<IndexEntry> }
```

### `notes/query.rs`

```rust
#[derive(Debug, Clone, PartialEq)] pub struct Query { /* private AST root */ }
#[derive(Debug, Clone, PartialEq)] pub struct QueryError { pub message: String, pub token_index: usize, pub byte_span: (usize, usize) }
pub fn parse(input: &str) -> Result<Query, QueryError>;
/// `body` is fetched only if the query actually contains a `text:` term.
pub fn eval(q: &Query, e: &IndexEntry, body: &mut dyn FnMut() -> String, now_ms: i64) -> bool;
pub fn needs_body(q: &Query) -> bool;
/// Additive, approved 2026-08-02 (Main). `eval` sees one entry, but "who links to me" is
/// a whole-index fact, so an UNBOUND `backlink:` matches nothing — the same degradation a
/// broken query already takes. Binding also upgrades `link:` from a literal string match
/// to real resolution. Call once per snapshot revision before running a space.
pub fn bind_index(query: &mut Query, index: &IndexSnapshot);
pub const MAX_DEPTH: usize = 8;
pub const MAX_TOKENS: usize = 256;
```

Grammar and predicate semantics: exactly as the architecture companion's "Space notes and the
query DSL" section. Depth cap 8, token cap 256, unknown key/flag = located error, broken query
matches nothing.

### `notes/search.rs`

```rust
pub struct Hit { pub line: u32, pub span: (usize, usize), pub snippet: String }
pub fn find(haystack: &str, needle: &str, max_hits: usize) -> Vec<Hit>;  // case + diacritic folded
```

### `notes/vm.rs`

Every type below derives `Debug, Clone, Serialize, Deserialize, TS` with
`#[ts(export)] #[serde(rename_all = "camelCase")]`, and every `u64`/`i64` field carries
`#[ts(type = "number")]` (the `copy_ipc.rs` precedent):

`NoteVaultVm { id, profile_id, name, subfolder, root, indexed, note_count, unread_count, cadence: NoteCadenceVm }`
`NoteCadenceVm { commit_idle_ms, push_interval_ms, push_on_blur }`
`NoteRowVm { id, path, title, snippet, tags, updated_ms, pinned, archived, unread, conflict, origin, head_rev }`
  — **amended 2026-08-02 (Main).** `head_rev: String` is the head revision `unread` was computed
  against; empty string when the note has no commit yet, the same "absent" spelling `origin` uses.
  Without it, clearing an unread mark from the list has nothing but `updated_ms` to acknowledge, and
  a mark cleared against a timestamp instead of a revision silently loses an agent's edit — the one
  failure NFR-30 exists to forbid. The shell fills it from the same commit lookup that produces
  `unread` and `origin`, so it costs nothing to carry.
`NoteListVm { rows: Vec<NoteRowVm>, total, offset }`
`NoteRefVm { vault_id, id, path, title }`
`NoteTagTreeVm { nodes: Vec<NoteTagNodeVm> }` / `NoteTagNodeVm { name, path, count, children }`
`NoteFolderVm { rel_dir, dirs: Vec<String>, notes: Vec<NoteRowVm> }`
`NoteSpaceVm { id, name, query, sort, limit, error: Option<String> }`
`NoteQueryCheckVm { ok, message: Option<String>, token_index: Option<u32>, span: Option<(u32,u32)> }`
`NoteTemplateVm { name, path }`
`NoteBodyBatch` — `#[serde(tag = "kind", rename_all = "camelCase")]` enum. Every variant carrying
  text carries the **body** (no `---`), with its block beside it, because the block is not in the
  editor buffer (FR-107, and the fix for DW-N1):
  `Reset { rev, frontmatter, text, cursor: Option<u32> } | External { rev, frontmatter, text } |
  Diverged { rev, frontmatter, theirs } | Renamed { path } | Gone`.
  `cursor` is a byte offset into `text`, set only when the note's template declared a `{{cursor}}`.
`NoteChangeBatch { vault_id, ops: Vec<NoteListOp> }`, `NoteListOp` — `#[serde(tag = "op",
  rename_all = "camelCase", rename_all_fields = "camelCase")]`, matching `RoomListOp`/`TimelineOp`/
  `InboxOp`, which are the same index-diff shape:
  `Reset { rows, total } | Upsert { index, row } | Remove { id }`
`NoteWriteVm { rev, path, frontmatter, conflict_copy: Option<String> }` — `frontmatter` is the block
  as it landed, `updated` already stamped.
`NoteRevisionVm { rev, when_ms, device, origin, source, subject }`
`NoteDiffVm { from_rev, to_rev: Option<String>, hunks: Vec<NoteHunkVm> }` / `NoteHunkVm { old_start, old_lines, new_start, new_lines, text }`
`NoteConflictVm { id, path, mine_rev, theirs_path }`
`NoteAttachmentVm { rel_path, url, markdown }`
`NoteLinkTargetVm { id, title, path }`
`NoteSearchBatch { done, hits: Vec<NoteSearchHitVm> }` / `NoteSearchHitVm { id, path, title, line, snippet }`
`NoteIndexProgressVm { vault_id, scanned, total_estimate, phase }`

Request types (also `TS`, also camelCase): `NoteQueryReq { text, tags, space_id, origin, flags, offset, limit }`,
`NoteCreateReq { title, body, template, dest, tags }`, `NoteSearchReq { text, limit }`,
`NoteSpaceReq { id: Option<String>, name, query, sort, limit }`,
`NoteVaultSettingsReq { subfolder: Option<String>, journal_template: Option<String>,
  default_template: Option<String>, cadence: Option<NoteCadenceVm> }` — every field optional, because
  a form that does not show a knob must never reset it (AD-34-9); absent means "keep what is in force",
`NoteConflictChoiceReq` tagged enum `TakeMine | TakeTheirs | Merged { text }`, `NoteFlag` enum `Pinned | Archived`.

### `keeper-core` other edits

- `vm.rs`: `CapabilitiesVm` gains `pub notes: bool` (beside `sync`).
- `palette.rs`: a `Notes` category with six actions, ids exactly
  `notes-new`, `notes-capture`, `notes-journal-today`, `notes-open`, `notes-search`,
  `notes-switch-vault`; shortcut labels `⌘⌥N`, `⌘⌥K`, `⌘⌥J`, `⌘P`, `⌘⇧F`, `⌘⌥V`.
  Gate the whole section on a new `notes: bool` parameter of `registry_sections`, mirroring
  how `recording` is already gated — every caller must pass it.
- `registry.rs`: `hotkey.capture` (`get_capture_hotkey` / `set_capture_hotkey`, absent = `""`),
  `notes.active_vault` (`get/set_active_vault`), and
  `notes_read_mark_get/set(data_dir, note_id) -> Option<String>` storing the acknowledged rev
  in the existing `settings` table under `notes.read.<note_id>`.
  Also `notes.capture_buffer` (`get/set_capture_buffer`).

## 2. `keeper-sync` — three additions

```rust
// profile.rs
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesConfig {
    #[serde(default = "default_subfolder")] pub subfolder: String,       // "notes"
    #[serde(default = "default_journal_template")] pub journal_template: String,
    #[serde(default)] pub default_template: Option<String>,
    #[serde(default)] pub cadence: NotesCadence,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesCadence { pub commit_idle_ms: u64, pub push_interval_ms: u64, pub push_on_blur: bool }
// Default: 2000 / 30000 / true. Clamp on validate: commit_idle_ms >= 500, push_interval_ms >= 5000.

// on SyncProfile:
#[serde(default)] pub notes: Option<NotesConfig>,

impl SyncProfile {
    /// `local_path.join(subfolder)`, or None when not a vault.
    pub fn vault_root(&self) -> Option<PathBuf>;
}
```

`validate()` rejects a subfolder that is empty, absolute, contains `..`, or equals
`.obsidian`, with the crate's existing `SyncError::Config`.

- `exclude.rs`: `BUILTIN_EXCLUDES` gains `.keeper` (name rule) and `**/.keeper/**` (subtree
  rule), following that module's own documented pairing.
- `engine.rs`: `Engine::watch_tap(&self) -> tokio::sync::broadcast::Receiver<(String, PathBuf)>`
  — profile id plus the changed absolute path, fanned out where the engine already drains its
  watch events. Capacity 1024; send failures ignored; the tap must never affect sync.
- `provenance.rs`: `SUBJECT_PLACEHOLDERS` gains `"notes"`.

## 3. `keeper` shell

New modules, all `#[cfg(desktop)]`: `notes_vault.rs`, `notes_ipc.rs`, `note_protocol.rs`,
`notes_window.rs`. Commands exactly as the architecture companion's IPC table, minus the
sticky ones. Every command is registered inside the single `keeper_with_commands!` literal in
`lib.rs` (desktop `$extra` list), and every one has a `#[cfg(not(desktop))]` `Unsupported`
twin so the handler list is identical on every target.

`tauri.conf.json` gains the second window:

```json
{ "label": "quick-capture", "url": "capture.html", "width": 560, "height": 340,
  "visible": false, "decorations": false, "alwaysOnTop": true, "skipTaskbar": true,
  "resizable": false, "center": true, "focus": false, "title": "keeper — quick note" }
```

and `capabilities/quick-capture.json` scopes `["quick-capture"]` to `core:default` +
`core:window:allow-hide` + `core:window:allow-set-focus`.

`vite.config.ts` gains the second rollup input (`capture: capture.html`).

## 4. Frontend

- `src/lib/ipc/client.ts` gains one thin wrapper per command, re-exporting the generated types
  exactly like the existing sync wrappers, and `subscribe*` helpers for the four channels.
- `PrimaryView` gains `"notes"`; `app-shell.tsx` gains its branch; `sidebar-pane.tsx` gains a
  capability-gated `NOTES_VIEW` entry placed before Settings.
- New deps (all MIT, all pre-cleared): `codemirror`, `@codemirror/state`, `@codemirror/view`,
  `@codemirror/language`, `@codemirror/commands`, `@codemirror/lang-markdown`,
  `@codemirror/autocomplete`, `@lezer/markdown`, `mermaid`, `@tanstack/react-virtual`.
  Mermaid and the editor are BOTH lazy `import()`s — the capture entry point must not pull
  either.

## 5. Rules every worker follows

- Do NOT run `cargo build`, `cargo test`, `cargo clippy`, `bun run build`, `bun test`,
  `bun run typecheck` or any formatter. The integrator runs them once, at the end.
- No `.unwrap()` / bare `.expect()` in production paths. `thiserror` errors, `tracing` logs.
- No `any` in TypeScript; `import type` for type-only imports; `@/*` alias.
- Rust unit tests in `#[cfg(test)]` modules beside the code; frontend tests colocated
  `*.test.tsx`. Test behaviour, not plumbing.
- English everywhere. Comments explain WHY, and are worth reading.
