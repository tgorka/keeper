//! The device-local cache of file bodies, keyed by content id
//! (`<cache>/sha256/<hex>`), never inside the replica's SQLite file
//! (DF/AD-13). A key names bytes, never a place, so a rename or move of a
//! folder changes no cached file's key (G-11). A cached body is used only
//! after its digest is checked against its key; a download in progress sits
//! beside it as `<hex>.part` until the transfer verifies and renames it.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::FilesError;

/// Bytes read per step while hashing or copying: a file is never whole in
/// memory.
const STEP: usize = 1 << 20;

/// The content-addressed cache of one domain on this device.
#[derive(Debug, Clone)]
pub struct Cache {
    dir: PathBuf,
}

/// The SHA-256 hex digest of the file at `path` and its length, read in
/// steps.
pub fn digest_file(path: &Path) -> std::io::Result<(String, u64)> {
    let mut reader = BufReader::with_capacity(STEP, File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; STEP];
    let mut length = 0_u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        length += read as u64;
    }
    Ok((hex::encode(hasher.finalize()), length))
}

/// The hex of a content id (`sha256:<hex>`), if it is one.
fn hex_of(content_id: &str) -> Option<&str> {
    keeper_files_api::ids::is_content_id(content_id)
        .then(|| content_id.strip_prefix("sha256:"))
        .flatten()
}

impl Cache {
    /// The cache under `dir`, created if absent.
    pub fn open(dir: &Path) -> Result<Self, FilesError> {
        let dir = keeper_paths::lexical_join(dir, "sha256")
            .map_err(|refusal| FilesError::Storage(format!("{refusal:?}")))?;
        std::fs::create_dir_all(&dir).map_err(|error| FilesError::storage("cache", &error))?;
        Ok(Self { dir })
    }

    /// Where `content_id`'s body is kept. Refused for text that is not a
    /// content id, which could not name a file here.
    pub fn path(&self, content_id: &str) -> Result<PathBuf, FilesError> {
        let hex = hex_of(content_id)
            .ok_or_else(|| FilesError::Row(format!("not a content id: {content_id}")))?;
        keeper_paths::lexical_join(&self.dir, hex)
            .map_err(|refusal| FilesError::Storage(format!("{refusal:?}")))
    }

    /// The cached body of `content_id` (`size` bytes) when its bytes hash
    /// to it; a body whose bytes do not (changed on disk, cut short) is
    /// removed, so the next fetch writes it afresh.
    pub fn valid(&self, content_id: &str, size: u64) -> Result<Option<PathBuf>, FilesError> {
        let path = self.path(content_id)?;
        let Ok(metadata) = std::fs::metadata(&path) else {
            return Ok(None);
        };
        let matches = metadata.is_file()
            && metadata.len() == size
            && digest_file(&path).is_ok_and(|(hex, length)| {
                length == size && hex_of(content_id) == Some(hex.as_str())
            });
        if matches {
            return Ok(Some(path));
        }
        std::fs::remove_file(&path).map_err(|error| FilesError::storage("cache", &error))?;
        Ok(None)
    }

    /// Write the empty file's body (it needs no request).
    pub(crate) fn store_empty(&self) -> Result<PathBuf, FilesError> {
        let path = self.path(keeper_files_api::ids::EMPTY_CONTENT_ID)?;
        File::create(&path).map_err(|error| FilesError::storage("cache", &error))?;
        Ok(path)
    }
}
