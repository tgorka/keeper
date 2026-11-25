//! Fetch on demand: a file's bytes come into the cache only when asked
//! for, through the files download grant and 115.3's transfer (presigned,
//! ranged, resumed from the bytes already on disk, its SHA-256 checked
//! before it is renamed onto its key). A cached body whose digest still
//! matches is used with no request.

use std::path::PathBuf;

use keeper_files_api::ids::EMPTY_CONTENT_ID;
use keeper_files_api::rows::Kind;
use keeper_ports::transfer::Progress;

use crate::tree::Listed;
use crate::{Files, FilesError, Target};

/// What a `get` did.
#[derive(Debug, Clone, PartialEq)]
pub struct Fetched {
    pub entry: Listed,
    /// The body in the cache.
    pub file: PathBuf,
    /// Whether a transfer ran (`false`: the cached body was valid).
    pub fetched: bool,
    /// Body bytes received by this call.
    pub bytes_fetched: u64,
    /// Download URLs asked for by this call.
    pub grants: u32,
}

impl Files {
    /// The file `target` names, in the cache: the cached body when its
    /// digest matches, else fetched (or continued from its `.part`).
    pub fn get(
        &self,
        target: &Target,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<Fetched, FilesError> {
        let entry = self.resolve(target)?;
        if entry.kind != Kind::File {
            return Err(FilesError::NotAFile(entry.id));
        }
        let listed = self.listed(&entry)?;
        let (Some(content_id), Some(size)) = (entry.content_id.as_deref(), entry.size) else {
            return Err(FilesError::Row(format!("file {} has no content", entry.id)));
        };
        if let Some(file) = self.cache.valid(content_id, size)? {
            return Ok(Fetched {
                entry: listed,
                file,
                fetched: false,
                bytes_fetched: 0,
                grants: 0,
            });
        }
        if content_id == EMPTY_CONTENT_ID {
            return Ok(Fetched {
                entry: listed,
                file: self.cache.store_empty()?,
                fetched: false,
                bytes_fetched: 0,
                grants: 0,
            });
        }
        let transfer = self
            .transfer
            .as_ref()
            .ok_or_else(|| FilesError::Offline(format!("{content_id} is not cached")))?;
        let file = self.cache.path(content_id)?;
        let downloaded = transfer.download(content_id, size, &file, progress)?;
        Ok(Fetched {
            entry: listed,
            file,
            fetched: true,
            bytes_fetched: downloaded.bytes_fetched,
            grants: downloaded.grants,
        })
    }
}
