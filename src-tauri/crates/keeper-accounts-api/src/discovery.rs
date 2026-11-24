//! `GET /databases`: the domains a credential opens, and nothing about any
//! other (story 114.2). The body keeps its camelCase envelope; each entry's
//! keys are snake_case, the names of the map's columns.
//!
//! A parser ignores an entry field it does not know, so the server can add
//! one without breaking a shipped client; the vectors hold such an entry.
//! It skips an entry whose `kind` or `files_writes` is a value it does not
//! know, and reads the rest of the body: a server that adds a fourth kind
//! hides that domain from an older client and nothing else. `kind` and
//! `display_name` stay required, so a body from a server older than story
//! 114.2 does not parse; that pairing is not supported.

use serde::de::value::Error as ValueError;
use serde::de::IntoDeserializer;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{DomainKind, FilesWrites};

/// The body of `GET /databases` for one credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    pub actor_id: String,
    pub client_id: String,
    /// Every domain this credential opens, and only those, less any entry
    /// whose `kind` or `files_writes` this client does not know.
    #[serde(deserialize_with = "known_entries")]
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
    /// The object store the server announced for this domain, as the server
    /// sent it (story 115.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
}

/// A `databases[]` entry as the server sends it, before an entry with a
/// value this client does not know is skipped.
#[derive(Deserialize)]
struct WireEntry {
    name: String,
    mount: String,
    kind: Lenient<DomainKind>,
    display_name: String,
    #[serde(default)]
    replaced_repository: Option<String>,
    #[serde(default)]
    graph_root: Option<String>,
    #[serde(default)]
    files_writes: Option<Lenient<FilesWrites>>,
    #[serde(default)]
    store: Option<String>,
}

impl WireEntry {
    fn known(self) -> Option<DomainEntry> {
        let Lenient::Known(kind) = self.kind else {
            return None;
        };
        let files_writes = match self.files_writes {
            None => None,
            Some(Lenient::Known(value)) => Some(value),
            Some(Lenient::Unknown) => return None,
        };
        Some(DomainEntry {
            name: self.name,
            mount: self.mount,
            kind,
            display_name: self.display_name,
            replaced_repository: self.replaced_repository,
            graph_root: self.graph_root,
            files_writes,
            store: self.store,
        })
    }
}

/// A string from a closed set, or a value of it this client does not know.
enum Lenient<T> {
    Known(T),
    Unknown,
}

impl<'de, T: for<'a> Deserialize<'a>> Deserialize<'de> for Lenient<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let word = String::deserialize(deserializer)?;
        let known: Result<T, ValueError> = T::deserialize(word.into_deserializer());
        Ok(known.map_or(Self::Unknown, Self::Known))
    }
}

fn known_entries<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<DomainEntry>, D::Error> {
    let wire = Vec::<WireEntry>::deserialize(deserializer)?;
    Ok(wire.into_iter().filter_map(WireEntry::known).collect())
}
