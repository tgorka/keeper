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

/// One file (or directory) inside a session, for the detail's mini-file
/// sections (FR-233). Session-relative path, `/`-joined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionFileVm {
    /// The file's name, for the row label.
    pub name: String,
    /// Session-relative path (`artifacts/report.md`).
    pub rel_path: String,
    /// Bytes; 0 for a directory.
    #[ts(type = "number")]
    pub size: u64,
    /// Modification time, ms since epoch.
    #[ts(type = "number")]
    pub mtime_ms: i64,
    pub is_dir: bool,
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

/// Everything the session detail renders (FR-233): the header facts, the
/// properties widget, the rendered log, and the file sections. Composed in
/// the shell from one directory walk plus the README parse; every field is
/// derivable from files alone (AD-110).
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
    /// Promoted output — versioned, click-to-open.
    pub artifacts: Vec<SessionFileVm>,
    /// Kept inputs — versioned, click-to-open.
    pub refs: Vec<SessionFileVm>,
    /// Reusable prompts — versioned, click-to-open.
    pub prompts: Vec<SessionFileVm>,
    /// Scratch, READ-ONLY (AD-113): listed with the zone's own caveat, never
    /// written, capped by the same walk budget as the freshness signal.
    pub workspace: Vec<SessionFileVm>,
    /// Loose files at the session root beside the README — a session that
    /// grew extra notes keeps them visible rather than orphaned.
    pub extras: Vec<SessionFileVm>,
}
