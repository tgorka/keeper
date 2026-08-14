//! Session view models — ordinary serde + `ts_rs` DTOs, exactly as every
//! other VM in the product (AD-7, AD-114). The shell composes them; the
//! webview renders them; nothing here decides anything.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One sessions root: a sessions-flagged sync profile, projected for the
/// board's root switcher (FR-224).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionRootVm {
    /// The profile id — a root is a filter over the profile list (AD-107).
    pub id: String,
    /// The profile's human name ("tgdrive").
    pub name: String,
    /// The zone subfolder in force ("60-sessions").
    pub subfolder: String,
    /// Absolute path of the zone root, for reveal and display.
    pub root: String,
    /// Whether the index has completed a scan of this root yet.
    pub indexed: bool,
    /// Sessions currently in `active/`.
    pub active_count: u32,
    /// Sessions with unseen changes (FR-235).
    pub unread_count: u32,
}

/// One row of the sessions board (FR-228, UX-DR85).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionRowVm {
    /// The session's ULID from its README frontmatter (FR-226).
    pub id: String,
    /// Zone-relative folder path — presentation, not identity.
    pub path: String,
    /// README H1, falling back to the folder name.
    pub title: String,
    /// `"active"` or `"archived"` — folder location, never a stored flag.
    pub status: String,
    /// The close year for an archived session, `null` for active.
    #[ts(type = "number | null")]
    pub archived_year: Option<i32>,
    /// Newest workspace mtime, ms since epoch — "the agent is iterating"
    /// (UX-DR86). `null` when the workspace is empty.
    #[ts(type = "number | null")]
    pub workspace_ms: Option<i64>,
    /// Newest record-side change (README/artifacts/refs/prompts), ms.
    #[ts(type = "number | null")]
    pub record_ms: Option<i64>,
    /// Date of the newest `## Log` entry, `YYYY-MM-DD`, empty when none.
    pub last_log_date: String,
    /// First line of the newest log entry — the row's subtitle (UX-DR85).
    pub last_log_line: String,
    /// First non-empty line under `## Summary`, for the wider row.
    pub snippet: String,
    /// Frontmatter tags, notes rules (FR-227).
    pub tags: Vec<String>,
    pub pinned: bool,
    /// Versioned content changed under another origin since last looked
    /// (FR-235). Cleared against `head_rev`, the notes contract.
    pub unread: bool,
    /// Provenance class of the last versioned change: `local`, `agent`,
    /// `remote` — the notes origin vocabulary (AD-63).
    pub origin: String,
    /// Revision the unread mark is held against; `""` for uncommitted.
    pub head_rev: String,
    /// A conflict copy exists inside the session.
    pub conflict: bool,
    /// This session continues another, or is continued (renders the lineage
    /// glyph; the chain itself is on the detail — UX-DR89).
    pub lineage: bool,
}

/// A stable pointer to one session, returned by every mutating command —
/// the `NoteRefVm` shape, session-sized.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionRefVm {
    pub root_id: String,
    pub id: String,
    pub path: String,
    pub title: String,
}

/// One thing a new session can be shaped from (FR-253): the zone's own
/// `_template/`, or a session that already exists. The picker lists these;
/// creating with one is `sessions_create` naming its id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionPatternVm {
    /// `"_template"` for the zone template, else the source session's ULID —
    /// the value the create command takes back.
    pub id: String,
    /// `"template"` or `"session"` — what kind of thing this is.
    pub kind: String,
    /// The label: the template's own name, or the session's title.
    pub label: String,
    /// One line of orientation: what the pattern is, in the zone's terms.
    pub detail: String,
    /// Newest change under the pattern, ms since epoch; `null` when unknown.
    /// Orders the list — a pattern you used yesterday beats one from March.
    #[ts(type = "number | null")]
    pub mtime_ms: Option<i64>,
    /// What creating from this pattern copies, and what it deliberately does
    /// not — the SAME decision the plan runs on, projected (AD-116). Empty
    /// `copies` is honest: some sessions carry nothing reusable.
    pub copies: Vec<SessionPatternFileVm>,
    pub skips: Vec<SessionPatternSkipVm>,
}

/// One file a pattern copies, for the preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionPatternFileVm {
    /// Source-relative path (`prompts/01-scope.md`).
    pub rel_path: String,
    pub is_dir: bool,
}

/// One file a pattern deliberately leaves behind, with the rule's own
/// sentence — so the preview answers "where did my report go" before it is
/// asked, rather than being silently short.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionPatternSkipVm {
    pub rel_path: String,
    /// The reason, spelled for a person by the domain, rendered verbatim.
    pub reason: String,
}

/// One dated entry of the session's `## Log`, parsed for the detail's
/// rendered timeline (FR-233). The zone writes newest-last; the detail
/// REVERSES for display, because the detail is a review surface and the
/// question it answers is "what happened most recently".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionLogEntryVm {
    /// `YYYY-MM-DD` from the entry's heading.
    pub date: String,
    /// What follows the dash on the heading — the sitting's own summary line.
    pub title: String,
    /// The entry's prose, markdown verbatim, trimmed. Empty is ordinary.
    pub body: String,
}

/// One entry of a session's own file tree (FR-254, AD-117).
///
/// A session folder is a small workspace, so the detail browses it the way the
/// Files tab browses a synced folder: real nesting, one sync mark per entry,
/// and the same words for the same state. What made this a new type rather
/// than a wider `SessionFileVm` is the two facts a flat list had nowhere to
/// put — whether the entry syncs, and whether keeper may write to it.
///
/// **Flat on the wire, nested on screen.** Every entry carries `parent` and
/// `depth`, and the frontend assembles the rows from them. That is rendering,
/// not deciding: the shell already knows the shape (it walked the directory),
/// and a nested payload would make the tree's own order a thing React could
/// disagree with the shell about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntryVm {
    /// The entry's own name — the row's label, and the only thing a person
    /// navigating by first letter is matching against.
    pub name: String,
    /// Session-relative path (`artifacts/report.md`), `/`-joined. The key.
    pub rel_path: String,
    /// Session-relative parent, `""` for a top-level entry. What the frontend
    /// nests on.
    pub parent: String,
    /// 1 for a top-level entry, growing with each level. Rendered as
    /// `aria-level`, so it starts at 1 like the ARIA tree it feeds.
    pub depth: u32,
    pub is_dir: bool,
    /// **Profile-relative**, composed in Rust (AD-65): the zone subfolder, the
    /// session folder and `rel_path`. The frontend hands this straight to a
    /// file target or to `sync_open_entry` and never joins a path itself.
    pub subpath: String,
    /// The same entry against the profile's local path, composed in Rust —
    /// [`crate::vm::FilesEntryVm::absolute_path`]'s rule, and its restriction:
    /// only ever an action's argument (reveal), never something rendered.
    pub absolute_path: String,
    /// `None` for a directory — the [`crate::vm::FilesEntryVm::size`] rule: a
    /// folder's byte count is a number keeper would have to invent.
    pub size: Option<crate::vm::FileSizeVm>,
    /// Modification time, ms since epoch; 0 when the OS would not say.
    #[ts(type = "number")]
    pub mtime_ms: i64,
    /// What sync says about this entry — the SAME mark and the SAME sentence
    /// the Files tab renders, from the same `Engine::pending` answer. A
    /// session that lives in a synced folder has a sync story, and hiding it
    /// here would make the Files tab and this tree disagree about one file.
    pub sync: crate::vm::FilesEntrySyncVm,
    /// Why keeper will not write here, when it will not (AD-113): the
    /// workspace fence's own refusal sentence, verbatim. `None` everywhere
    /// else. A lock with no reason is a lock people file bugs about.
    pub locked: Option<String>,
    /// Why this row has no Delete, when it has none (FR-262):
    /// [`super::files::check_deletable`]'s own sentence, verbatim.
    ///
    /// The same shape as [`Self::locked`] and for the same reason, but a
    /// strictly wider question — scratch is one of four ways a file can be
    /// undeletable, beside a path that leaves the session, an extension keeper
    /// does not author, and the two files that decide the session's shape. The
    /// row renders its Delete exactly when this is `None`, so the button and
    /// the command agree by construction rather than by both being kept in step
    /// (AD-108): a rule the frontend re-derives is a rule that drifts the first
    /// time a fifth refusal is added here.
    ///
    /// Always `Some` for a directory, because deleting a folder is a different
    /// verb — recursive, and irreversible in a way one file is not — and this
    /// module does not offer it.
    pub undeletable: Option<String>,
}

/// One session's file tree (FR-254): the entries, and whether the walk was cut
/// short.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionTreeVm {
    /// Every entry, shell-ordered: the zone's own sections first, in the
    /// zone's own order, each followed by its subtree.
    pub entries: Vec<SessionEntryVm>,
    /// The walk hit its budget and stopped. A session's `workspace/` can hold
    /// a `node_modules`, and a tree that silently showed a prefix of one would
    /// be a tree that lies about being complete.
    pub truncated: bool,
}

/// One thing a session points at (FR-255, AD-118).
///
/// The tree says what a session *holds*; this says what it *names*. The zone's
/// own rule is that the two differ on purpose — big files live in their zone
/// and a session references them by repo-root-relative path — so the pointer is
/// what breaks, and this is the row that says so.
///
/// **Not [`SessionRefVm`]**, which is a handle *to* a session (the `NoteRefVm`
/// shape, returned by the mutating commands). This is a reference *from* one,
/// and spelling both `Ref` would make two unrelated things one grep.
///
/// Every field was decided in Rust. `kind` is six resolvers' answers, not a
/// guess from a file extension; `panelTarget` is the one file target (AD-109)
/// already composed, so a click is `setActiveTarget(row.panelTarget)` and the
/// frontend joins nothing (AD-65).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionReferenceVm {
    /// `"note"`, `"recording"`, `"file"`, `"session"`, `"external"` or
    /// `"missing"` — [`crate::sessions::refs::RefKind::as_str`]. A string
    /// rather than a union because the set grows the way the index's `flags`
    /// grow, and `missing` is the product's existing word for the last one.
    pub kind: String,
    /// The target **as the author spelled it**, which is what makes a missing
    /// row findable in the file it was written in — the export receipt's rule.
    pub target: String,
    /// What to call it: the resolved title, else the link's own words, else
    /// the target.
    pub label: String,
    /// Session-relative path of the file this was written in (`README.md`,
    /// `refs/inputs.md`) — where to go to fix it.
    pub source: String,
    /// What clicking opens, or `None` for a missing row and nothing to open.
    /// External URLs are not panel targets: they open in the system browser,
    /// so [`Self::url`] carries them instead.
    pub panel_target: Option<crate::panels::PanelTargetVm>,
    /// The `http(s)` address of an external row, `None` for every other kind.
    pub url: Option<String>,
    /// Why a missing row is missing, naming the paths keeper looked in —
    /// [`crate::notes::embed::not_found_notice`]'s acceptance criterion, in a
    /// session's frame. `None` for everything that resolved.
    pub notice: Option<String>,
}

/// Everything one session points at (FR-255), missing first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionReferencesVm {
    /// The rows, shell-ordered: broken pointers first, then document order.
    pub refs: Vec<SessionReferenceVm>,
    /// How many of them are missing — the number the widget's heading states,
    /// counted in Rust so two surfaces cannot count it differently.
    pub missing: u32,
    /// The scan hit its budget. A session whose `refs/` somebody filled with a
    /// crawl is the one way here, and a list that silently showed a prefix
    /// would be a list that lies about being complete — the tree's own rule.
    pub truncated: bool,
}

/// One user-tier frontmatter field of the session README, for the detail's
/// properties widget (FR-227): keeper-owned keys and `tags` are projected
/// elsewhere on the header, so what remains here is exactly "yours".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionPropertyVm {
    pub key: String,
    /// The flattened index form — a list joins on newline, the notes rule.
    pub value: String,
}

/// Everything the session detail renders about the session's *record*
/// (FR-233): the header facts, the properties widget and the rendered log.
/// Composed in the shell from one README parse; every field is derivable from
/// files alone (AD-110).
///
/// The files themselves are [`SessionTreeVm`], read separately (FR-254): the
/// tree costs a walk and one `Engine::pending` query, and the log does not,
/// so binding them into one payload would make every log re-read pay for the
/// tree.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetailVm {
    pub id: String,
    /// Zone-relative folder path.
    pub path: String,
    pub title: String,
    /// `"active"` or `"archived"` — location, never a stored flag.
    pub status: String,
    #[ts(type = "number | null")]
    pub archived_year: Option<i32>,
    pub pinned: bool,
    pub tags: Vec<String>,
    /// User-tier frontmatter, the properties widget.
    pub properties: Vec<SessionPropertyVm>,
    /// Lineage ids, both directions (AD-112). Dangling ids render inert.
    pub continues: Vec<String>,
    pub continued_by: Vec<String>,
    /// First prose under `## Summary`, the header's one-liner.
    pub summary: String,
    /// The `## Log`, parsed, NEWEST FIRST (review order — the zone's file
    /// stays newest-last; only this projection reverses).
    ///
    /// Same shape and same order under both contracts: a flat session's log is
    /// its `log`-tagged files, and the detail cannot tell which it is holding.
    /// That is the point — the reader changed, the rendering did not.
    pub log: Vec<SessionLogEntryVm>,
    /// Which on-disk contract this session follows: `"flat"` or `"folder"`
    /// ([`crate::sessions::shape::Shape`]).
    ///
    /// A string rather than a bool because the set may grow and a field named
    /// `flat` cannot carry a third answer. The frontend reads it to decide what
    /// to *offer* — migration, a new-log button — never to decide what a file
    /// means; that decision was already made in Rust (AD-7).
    pub shape: String,
    /// Root markdown declaring no kind: a leftover `README.md`, a file someone
    /// dropped in, anything mid-migration. Session-relative paths.
    ///
    /// Surfaced rather than swallowed. The flat contract's whole premise is
    /// that a file says what it is, so a file that says nothing is exactly the
    /// case the operator needs to see — and it is what makes a half-finished
    /// migration visible instead of merely survivable. Empty for a clean
    /// session, in both shapes.
    pub unfiled: Vec<String>,
    /// The work items, ready for the board. Empty under the folder contract,
    /// which has no such thing.
    pub tasks: Vec<SessionTaskVm>,
}

/// One work item of a flat session — a `task`-tagged markdown file, projected
/// as a board card (FR-259).
///
/// A card is a *file*, not a row in a database keeper keeps beside the files:
/// its column is its `status:` and its position is its `order:`, both ordinary
/// frontmatter that Obsidian shows and an agent can write. Moving a card writes
/// one key (FR-121); nothing else in the file changes, and nothing outside the
/// file has to be told.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionTaskVm {
    /// ULID when the file has one, else `path:<rel>` — keeper never stamps an
    /// id into a file it did not author (FR-121).
    pub id: String,
    /// Session-relative path: what opens the card.
    pub rel_path: String,
    pub title: String,
    /// The column, as [`crate::sessions::shape::TaskStatus::as_str`] spells it.
    /// `null` when the file states a status nothing can read — the card renders
    /// as unplaced rather than silently landing in "to do".
    #[ts(type = "string | null")]
    pub status: Option<String>,
    /// Position within the column. Fractional by design: dropping a card
    /// between two others writes one number rather than renumbering the rest.
    pub order: f64,
    /// The order is the file's own rather than a default keeper supplied —
    /// [`crate::notes::order::NoteOrderSource::Own`].
    pub order_is_own: bool,
    /// Tags beyond the kind, for filtering and for the card's own chips.
    pub tags: Vec<String>,
    /// The id is path-derived, so pins and lineage will not survive a rename.
    pub unstable_identity: bool,
}

/// What migrating one session would do, shown before anything is done (FR-257).
///
/// Migration rewrites the shape of a folder on a live, synced drive, and the
/// two removals at the end are not undoable from inside keeper. So it is a verb
/// the operator triggers after reading what it will do — never something a scan
/// performs on their behalf, and never a dialog that only says "are you sure".
/// This is the *what*: every path that will appear, the one that will be
/// rewritten, and the two that will go to the trash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionMigrationVm {
    /// False when this session already follows the flat contract. Everything
    /// below is then empty, and the UI offers nothing rather than an inert
    /// button.
    pub needed: bool,
    /// Session-relative paths that will be created, in write order.
    pub creates: Vec<String>,
    /// Paths that will be rewritten in place — today only `README.md`, which
    /// becomes a signpost pointing at `about.md`.
    pub rewrites: Vec<String>,
    /// Directories that will be moved to the trash, last and irreversibly.
    pub trashes: Vec<String>,
}

/// One space definition in a zone's `_spaces/`, projected for the rail and the
/// editor (FR-261, AD-121).
///
/// Deliberately field-for-field [`crate::notes::vm::NoteSpaceVm`] minus the two
/// things a session space does not have — a template (a session's files are made
/// by the file verbs, not by a space) and a limit (a session holds tens of files,
/// not thousands, so a cap would only ever hide one). Same names, same meanings,
/// same `error`/`warnings` split, because the chip editor that opens one opens
/// the other and a field that meant something else here would be a second
/// contract wearing the first one's clothes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionSpaceVm {
    /// Zone-relative path (`_spaces/tasks.md`) — the id, and the thing every
    /// write and delete names.
    ///
    /// A path rather than a ULID, unlike a note space, and that is the flat
    /// contract's own rule showing through: `_spaces/` is a directory keeper
    /// introduces, it is not indexed as a vault, and there is no snapshot to ask
    /// "which file has this id". The file's location IS its identity here, which
    /// is also why renaming a space rewrites `title` and never the filename.
    pub id: String,
    pub name: String,
    /// `keeper.space`, verbatim — never the canonical re-emission of it. What
    /// the editor shows is what the file says (FR-121).
    pub query: String,
    /// `keeper.sort`, verbatim, including a word keeper could not read.
    pub sort: String,
    /// What the sort actually resolves to, canonical — what the list is doing,
    /// which is what the form seeds from.
    pub sort_effective: String,
    pub icon: Option<String>,
    /// Which seeded default this is, or `None` for one the operator wrote.
    /// Read from `keeper.default`, which only keeper writes.
    pub default_key: Option<String>,
    /// Rail position; zero means unpositioned, and ties break by name.
    pub order: f64,
    /// Presentation keys keeper could not read, each a finished sentence. The
    /// space still works — this is the "not obeying one line of its own file"
    /// severity, distinct from `error`.
    pub warnings: Vec<String>,
    /// The query does not parse. The space then selects **nothing**; it never
    /// falls back to selecting the whole session.
    pub error: Option<String>,
}

/// What one space selected out of one session's pool (FR-261).
///
/// The definitions and the selections are two payloads on purpose. A zone's
/// definitions are the same for every session and change when someone edits a
/// space; a selection changes whenever any file in the session does. Binding
/// them would make every file write re-read five `_spaces/*.md` off the drive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionSpaceFilesVm {
    /// The space's id, matching one [`SessionSpaceVm::id`].
    pub space_id: String,
    /// What it selected, in the space's own order.
    pub files: Vec<SessionSpaceFileVm>,
    /// Its query would not parse, already worded. `files` is then empty, and the
    /// section renders the sentence rather than a suspiciously complete list.
    pub error: Option<String>,
}

/// One file a space selected — the card a space's section draws.
///
/// A thinner projection than [`SessionTaskVm`]: a space section lists files, and
/// the one thing every kind of file has that a name does not convey is when it
/// last changed. The board needs a status and a position because it *arranges*
/// cards; a list does not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionSpaceFileVm {
    /// ULID when the file carries one, else `path:<rel>` — keeper never stamps
    /// an id into a file it did not author.
    pub id: String,
    /// Session-relative path — what the row is keyed by, and what it shows when
    /// a file is somewhere other than the session root.
    pub rel_path: String,
    /// **Profile-relative**, composed in Rust (AD-65) exactly as
    /// [`SessionEntryVm::subpath`] is: the zone subfolder, the session folder
    /// and `rel_path`. The frontend hands this straight to a file target and
    /// never joins a path itself.
    pub subpath: String,
    pub title: String,
    /// Every tag the file carries, kind included, for the row's own chips.
    ///
    /// Not "tags beyond the kind": the kind is a tag like any other here, and
    /// which tag a space selected on is a fact about the *query*, not about the
    /// file. Subtracting it would need this projection to re-read the query it
    /// was already evaluated against, and would make a file's chips depend on
    /// which section is drawing it — the same file listing two different tag
    /// sets in two spaces. A row says what its file says.
    pub tags: Vec<String>,
    /// Last modified, from the shell's stat — the one fact a filename hides.
    ///
    /// `number`, like [`SessionEntryVm::mtime_ms`]: ts-rs maps a bare `i64` to
    /// `bigint`, which does not survive `JSON.parse` and would make this the one
    /// timestamp in the pane a date formatter refuses.
    #[ts(type = "number")]
    pub mtime_ms: i64,
    /// The id is path-derived, so it will not survive a rename.
    pub unstable_identity: bool,
}

/// What the editor sends to save one session space (FR-261).
///
/// [`crate::notes::vm::NoteSpaceReq`]'s twin, minus `limit` and `template` for
/// [`SessionSpaceVm`]'s reasons, and with `id` meaning a path rather than a
/// ULID: `None` creates a file whose name is derived from the name, `Some(rel)`
/// rewrites that exact file and never moves it.
///
/// There is no `defaultKey`. `keeper.default` is keeper's own marker, read off
/// the file being edited and written back unchanged, so a request cannot promote
/// a hand-written space into a seeded one — which would leave Restore offering
/// something that is already there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionSpaceReq {
    /// Zone-relative path of the space to rewrite; `None` creates one.
    pub id: Option<String>,
    pub name: String,
    pub query: String,
    /// The canonical `<key> <dir>` the form had selected.
    pub sort: String,
    pub icon: Option<String>,
    /// Rail position; zero writes no key, because zero means unpositioned.
    pub order: f64,
}

/// What a restore actually wrote (FR-261).
///
/// Names rather than a count, because "3 spaces restored" and "About, Log and
/// Prompts restored" cost the same to send and only one of them tells the
/// operator whether keeper agreed with them about what was missing. Empty when
/// nothing was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionSpacesRestoredVm {
    pub names: Vec<String>,
}
