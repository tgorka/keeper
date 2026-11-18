//! `GET /databases`: the domains a credential opens, and nothing about any
//! other (DF/OQ-16). The body keeps its camelCase envelope; each entry's
//! keys are snake_case, the names of the map's columns.
//!
//! A parser ignores an entry field it does not know, so the server can add
//! one without breaking a shipped client; the vectors hold such an entry.

use serde::{Deserialize, Serialize};

use crate::{DomainKind, FilesWrites};

/// The body of `GET /databases` for one credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    pub actor_id: String,
    pub client_id: String,
    /// Every domain this credential opens, and only those.
    pub databases: Vec<DomainEntry>,
}

/// One domain the credential opens: what a person's device is told about
/// it, and the input of the view of "my domains" (114.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainEntry {
    /// The domain's name: its database, its schema and its drive.
    pub name: String,
    /// The path its replicas sync under, `/<name>`.
    pub mount: String,
    pub kind: DomainKind,
    /// What a person is shown for it.
    pub display_name: String,
    /// The git repository this domain replaced — its remote, as the operator
    /// set it — or absent. Set only on the admin path when the domain is an
    /// import's destination, and told only to the domain's members. On its
    /// own it never means the drive is cut over: an offer to stop using the
    /// git drive needs `files_writes` to be `open` as well (120.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_repository: Option<String>,
    /// The id of the domain's root `Collection` — the one node of its graph
    /// that carries `okf:profile_version` — absent until that node exists.
    /// A walk of the domain's graph starts from it (117.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_root: Option<String>,
    /// The domain's `shadow`/`open` record; absent while the domain has no
    /// manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_writes: Option<FilesWrites>,
}
