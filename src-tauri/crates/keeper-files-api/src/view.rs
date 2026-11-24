//! What a client shows of a manifest, and the registered reads it names;
//! every one names its data by `(server, domain)` ([`DomainRef`]).

use serde::{Deserialize, Serialize};

use crate::names::FileMode;
use crate::rows::Kind;
use crate::DomainRef;

/// One listed entry of a folder (a listing is by `parent_id`; a drive's
/// root is never in its own listing).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryVm {
    pub at: DomainRef,
    pub drive: String,
    pub id: String,
    pub parent_id: String,
    pub name: String,
    pub kind: Kind,
    pub content_id: Option<String>,
    pub size: Option<u64>,
    pub media_type: Option<String>,
    pub mtime_ms: Option<i64>,
    pub mode: FileMode,
    pub revision: String,
}

/// One drive of a domain, from `files_head.drives`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveVm {
    pub at: DomainRef,
    pub name: String,
    pub root_id: String,
}

/// The answer of `files.freshness`: how current a shadowed manifest is
/// (118.4's server-only record answers it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Freshness {
    pub last_commit_time_ms: Option<i64>,
    pub last_run: Option<ImportRun>,
}

/// The last import run, as `files.freshness` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRun {
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub result: RunResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunResult {
    Running,
    Ok,
    Failed,
}

/// `files.freshness`'s params: `{version: 1, domain}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreshnessQuery {
    pub version: u32,
    pub domain: String,
}
