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
    pub log: Vec<SessionLogEntryVm>,
}
