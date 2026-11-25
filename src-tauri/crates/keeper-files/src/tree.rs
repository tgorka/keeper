//! The tree, read from the replica's rows alone: drives from `files_head`,
//! a folder's entries by `parent_id`, an entry's path derived by walking
//! `parent_id` to its drive's root, a typed path resolved segment by
//! segment under the stored fold. Nothing here makes a request.

use std::path::PathBuf;

use keeper_files_api::rows::{Drive, Entry, Kind, Lifecycle};
use keeper_files_api::view::{DriveVm, EntryVm};
use keeper_files_api::{DomainRef, MAX_DEPTH};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::{Files, FilesError, Target};

/// One listed entry: the contract's view model, its derived drive-relative
/// path, and its `facts`, absent until 122.6 fills them from `files_facts`
/// (G-26) — the one listing view model 119.1 also extends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listed {
    #[serde(flatten)]
    pub entry: EntryVm,
    /// The drive-relative path, derived anew on every read (`""` for a
    /// root).
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facts: Option<Value>,
}

/// A folder and its live entries, root excluded, ordered by fold then id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    pub at: DomainRef,
    pub folder: Listed,
    pub entries: Vec<Listed>,
}

/// The fields of `files_entries` an [`Entry`] holds; a replica row carries
/// more (its authorship), which a listing never shows.
const ENTRY_FIELDS: [&str; 17] = [
    "id",
    "domain",
    "drive_key",
    "parent_id",
    "name",
    "name_fold",
    "kind",
    "content_id",
    "size",
    "media_type",
    "mtime_ms",
    "mode",
    "revision",
    "lifecycle",
    "created_event_id",
    "updated_event_id",
    "updated_seq",
];

/// A `files_entries` row as the contract's [`Entry`].
pub(crate) fn entry_of(row: &Map<String, Value>) -> Result<Entry, FilesError> {
    let mut fields = Map::new();
    for name in ENTRY_FIELDS {
        fields.insert(
            name.to_owned(),
            row.get(name).cloned().unwrap_or(Value::Null),
        );
    }
    Entry::deserialize(Value::Object(fields)).map_err(|error| FilesError::Row(error.to_string()))
}

/// A JSON column, whether the replica answers its text or its value.
fn json_column(value: Option<&Value>) -> Result<Value, FilesError> {
    match value {
        Some(Value::String(text)) => {
            serde_json::from_str(text).map_err(|error| FilesError::Row(error.to_string()))
        }
        Some(value) => Ok(value.clone()),
        None => Ok(Value::Null),
    }
}

impl Files {
    /// The domain's drives, from `files_head.drives`, sorted by name.
    pub fn drives(&self) -> Result<Vec<DriveVm>, FilesError> {
        let rows = self.source.replica.query(
            "SELECT drives FROM files_head WHERE id = ?",
            &[json!(self.at.domain)],
        )?;
        let Some(row) = rows.first() else {
            return Ok(Vec::new());
        };
        let drives = match json_column(row.get("drives"))? {
            Value::Null => Vec::new(),
            value => Vec::<Drive>::deserialize(value)
                .map_err(|error| FilesError::Row(error.to_string()))?,
        };
        let mut drives: Vec<DriveVm> = drives
            .into_iter()
            .map(|drive| DriveVm {
                at: self.at.clone(),
                name: drive.name,
                root_id: drive.root_id,
            })
            .collect();
        drives.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(drives)
    }

    /// The entry `id`, whatever its lifecycle.
    pub fn entry(&self, id: &str) -> Result<Option<Entry>, FilesError> {
        let rows = self
            .source
            .replica
            .query("SELECT * FROM files_entries WHERE id = ?", &[json!(id)])?;
        rows.first().map(entry_of).transpose()
    }

    /// The live entries of folder `parent_id`, its own row excluded (a
    /// root's `parent_id` is itself), ordered by fold, then id.
    pub(crate) fn children(&self, parent_id: &str) -> Result<Vec<Entry>, FilesError> {
        self.source
            .replica
            .query(
                "SELECT * FROM files_entries WHERE parent_id = ? AND id <> parent_id \
                 AND lifecycle = 'live' ORDER BY name_fold, id",
                &[json!(parent_id)],
            )?
            .iter()
            .map(entry_of)
            .collect()
    }

    /// The live child of `parent_id` whose stored fold is `fold`.
    pub(crate) fn child_by_fold(
        &self,
        parent_id: &str,
        fold: &str,
    ) -> Result<Option<Entry>, FilesError> {
        let rows = self.source.replica.query(
            "SELECT * FROM files_entries WHERE parent_id = ? AND id <> parent_id \
             AND lifecycle = 'live' AND name_fold = ? ORDER BY id",
            &[json!(parent_id), json!(fold)],
        )?;
        rows.first().map(entry_of).transpose()
    }

    /// The drive named `drive` (exactly, or under the fold).
    fn drive(&self, drive: &str) -> Result<DriveVm, FilesError> {
        let drives = self.drives()?;
        let fold = keeper_paths::names::name_fold(drive);
        drives
            .iter()
            .find(|candidate| candidate.name == drive)
            .or_else(|| {
                drives
                    .iter()
                    .find(|candidate| keeper_paths::names::name_fold(&candidate.name) == fold)
            })
            .cloned()
            .ok_or_else(|| FilesError::NotFound(format!("drive {drive}")))
    }

    /// The live entry a typed path names in `drive`: each segment is
    /// matched under the fold, so `notes/Plan.MD` finds `Notes/plan.md`. A
    /// path `keeper-paths` refuses is refused before anything is read.
    pub fn resolve_path(&self, drive: &str, path: &str) -> Result<Entry, FilesError> {
        let segments = keeper_paths::plain_segments(path)
            .map_err(|refusal| FilesError::PathRefused(format!("{refusal:?}")))?;
        let names = segments
            .iter()
            .map(|segment| {
                segment
                    .to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| FilesError::PathRefused(path.to_owned()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let drive = self.drive(drive)?;
        let mut current = self
            .entry(&drive.root_id)?
            .filter(|root| root.lifecycle == Lifecycle::Live)
            .ok_or_else(|| FilesError::NotFound(format!("drive {}", drive.name)))?;
        for name in names {
            if current.kind != Kind::Dir {
                return Err(FilesError::NotFound(path.to_owned()));
            }
            current = self
                .child_by_fold(&current.id, &keeper_paths::names::name_fold(&name))?
                .ok_or_else(|| FilesError::NotFound(path.to_owned()))?;
        }
        Ok(current)
    }

    /// The live entry `target` names.
    pub fn resolve(&self, target: &Target) -> Result<Entry, FilesError> {
        match target {
            Target::Path { drive, path } => self.resolve_path(drive, path),
            Target::Entry(id) => self
                .entry(id)?
                .filter(|entry| entry.lifecycle == Lifecycle::Live)
                .ok_or_else(|| FilesError::NotFound(format!("entry {id}"))),
        }
    }

    /// The drive name and drive-relative path of `entry`, derived by
    /// walking `parent_id` to its drive's root and joining the names
    /// through `keeper-paths`.
    pub fn path_of(&self, entry: &Entry) -> Result<(String, String), FilesError> {
        let mut names: Vec<String> = Vec::new();
        let mut current = entry.clone();
        for _ in 0..=MAX_DEPTH {
            if current.is_root() {
                names.reverse();
                let mut joined = PathBuf::new();
                for name in &names {
                    joined = keeper_paths::lexical_join(&joined, name)
                        .map_err(|refusal| FilesError::Row(format!("{refusal:?}")))?;
                }
                let path = joined
                    .to_str()
                    .ok_or_else(|| FilesError::Row(format!("entry {}", entry.id)))?
                    .to_owned();
                return Ok((current.name, path));
            }
            names.push(current.name.clone());
            current = self
                .entry(&current.parent_id)?
                .ok_or_else(|| FilesError::Row(format!("entry {} has no root", entry.id)))?;
        }
        Err(FilesError::Row(format!(
            "entry {} is deeper than {MAX_DEPTH}",
            entry.id
        )))
    }

    /// `entry` as a listing shows it.
    pub fn listed(&self, entry: &Entry) -> Result<Listed, FilesError> {
        let (drive, path) = self.path_of(entry)?;
        Ok(self.listed_at(entry, drive, path))
    }

    fn listed_at(&self, entry: &Entry, drive: String, path: String) -> Listed {
        Listed {
            entry: EntryVm {
                at: self.at.clone(),
                drive,
                id: entry.id.clone(),
                parent_id: entry.parent_id.clone(),
                name: entry.name.clone(),
                kind: entry.kind,
                content_id: entry.content_id.clone(),
                size: entry.size,
                media_type: entry.media_type.clone(),
                mtime_ms: entry.mtime_ms,
                mode: entry.mode,
                revision: entry.revision.clone(),
            },
            path,
            facts: None,
        }
    }

    /// The folder `target` names and its live entries, from the replica
    /// alone: offline as online. The folder's path is derived once; each
    /// entry's is the folder's joined with its name through `keeper-paths`.
    pub fn list(&self, target: &Target) -> Result<Listing, FilesError> {
        let folder = self.resolve(target)?;
        if folder.kind != Kind::Dir {
            return Err(FilesError::NotAFolder(folder.id));
        }
        let (drive, folder_path) = self.path_of(&folder)?;
        let base = PathBuf::from(&folder_path);
        let entries = self
            .children(&folder.id)?
            .iter()
            .map(|entry| {
                let path = keeper_paths::lexical_join(&base, &entry.name)
                    .ok()
                    .and_then(|joined| joined.to_str().map(str::to_owned))
                    .ok_or_else(|| FilesError::Row(format!("entry {}", entry.id)))?;
                Ok(self.listed_at(entry, drive.clone(), path))
            })
            .collect::<Result<Vec<_>, FilesError>>()?;
        Ok(Listing {
            at: self.at.clone(),
            folder: self.listed_at(&folder, drive, folder_path),
            entries,
        })
    }
}
