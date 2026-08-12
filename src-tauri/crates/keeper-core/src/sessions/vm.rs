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
