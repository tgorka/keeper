//! Who sends a files command (decision 3) and the file part of an agent's,
//! a tool's or a job's capability.

use keeper_accounts_api::grant::{FileItem, GrantMode};
use keeper_accounts_api::Role;
use serde::{Deserialize, Serialize};

use crate::refusal::{Code, Refusal};

/// What a capability's file part may hold over its roots. Read carries no
/// mode; `reverse_own` decides whose effects a caller may reverse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileClass {
    Read,
    Write,
    ReverseOwn,
}

impl FileClass {
    pub const ALL: [Self; 3] = [Self::Read, Self::Write, Self::ReverseOwn];

    /// The class a word names; any other word is `invalid_change`.
    pub fn parse(word: &str) -> Result<Self, Refusal> {
        match word {
            "read" => Ok(Self::Read),
            "write" => Ok(Self::Write),
            "reverse_own" => Ok(Self::ReverseOwn),
            _ => Err(Refusal::new(Code::InvalidChange)),
        }
    }
}

/// How strict a mode is: `ask` (2) before `propose` (1) before `apply` (0).
#[must_use]
pub const fn strictness(mode: GrantMode) -> u8 {
    match mode {
        GrantMode::Ask => 2,
        GrantMode::Propose => 1,
        GrantMode::Apply => 0,
    }
}

/// The mode a word names; any other word is `unknown_mode`, refused when
/// the grant is read.
pub fn parse_mode(word: &str) -> Result<GrantMode, Refusal> {
    match word {
        "ask" => Ok(GrantMode::Ask),
        "propose" => Ok(GrantMode::Propose),
        "apply" => Ok(GrantMode::Apply),
        _ => Err(Refusal::new(Code::UnknownMode)),
    }
}

/// One parsed item of a file part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePartItem {
    pub roots: Vec<String>,
    pub classes: Vec<FileClass>,
    pub mode: GrantMode,
}

/// Parses a capability's file part as 114.6 stores it (words carried as
/// given): an unknown mode is `unknown_mode`, an unknown class
/// `invalid_change`.
pub fn parse_file_part(items: &[FileItem]) -> Result<Vec<FilePartItem>, Refusal> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let mode = parse_mode(&item.mode)?;
        let classes = item
            .classes
            .iter()
            .map(|word| FileClass::parse(word))
            .collect::<Result<Vec<_>, _>>()?;
        out.push(FilePartItem {
            roots: item.roots.clone(),
            classes,
            mode,
        });
    }
    Ok(out)
}

/// The grant a rooted caller's write cites in `{basis: "grant", …}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantRef {
    pub grant_id: String,
    pub grant_revision: String,
}

/// The sender of a command, as the server derives it from the credential
/// (never from the payload).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Caller {
    /// A person's device: its domain role governs.
    Person { actor_id: String, role: Role },
    /// An agent instance, under its grant's file part.
    Agent {
        actor_id: String,
        grant: GrantRef,
        files: Vec<FileItem>,
        #[serde(default)]
        verified_uploads: Vec<String>,
    },
    /// A tool identity (114.6).
    Tool {
        actor_id: String,
        grant: GrantRef,
        files: Vec<FileItem>,
        #[serde(default)]
        verified_uploads: Vec<String>,
    },
    /// A job (epic 122): reserved; an envelope with `job` set is refused.
    Job {
        actor_id: String,
        grant: GrantRef,
        files: Vec<FileItem>,
        #[serde(default)]
        verified_uploads: Vec<String>,
    },
    /// The import identity (114.6): `files.import` only, no roots.
    Import { actor_id: String },
}

/// The kinds of caller a grant's default mode is given for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantHolder {
    Agent,
    Tool,
    Job,
}

/// The mode a write class gets when its grant names none: a tool and a job
/// `apply`, an agent instance `ask` (which may always propose instead).
#[must_use]
pub const fn default_mode(holder: GrantHolder) -> GrantMode {
    match holder {
        GrantHolder::Agent => GrantMode::Ask,
        GrantHolder::Tool | GrantHolder::Job => GrantMode::Apply,
    }
}

impl Caller {
    #[must_use]
    pub fn actor_id(&self) -> &str {
        match self {
            Self::Person { actor_id, .. }
            | Self::Agent { actor_id, .. }
            | Self::Tool { actor_id, .. }
            | Self::Job { actor_id, .. }
            | Self::Import { actor_id } => actor_id,
        }
    }

    /// `origin_class` of the events this caller's commands write.
    #[must_use]
    pub const fn origin_class(&self) -> crate::rows::OriginClass {
        use crate::rows::OriginClass;
        match self {
            Self::Person { .. } => OriginClass::Human,
            Self::Agent { .. } => OriginClass::Agent,
            Self::Tool { .. } => OriginClass::Tool,
            Self::Job { .. } => OriginClass::Job,
            Self::Import { .. } => OriginClass::Import,
        }
    }
}
