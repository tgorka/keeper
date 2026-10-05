//! The file-level writer of a session's log (AD-366; choice C1).
//!
//! One host writes its own chunks and nobody else's: the writer composes
//! every chunk name from its own slug, opens it `O_APPEND | O_CREAT`, writes
//! each line with one `write` ending in `\n`, and `fsync`s at [`ChunkWriter::sync`]
//! (the end of a turn, and before an approval is consumed). On opening its own
//! current chunk it truncates a torn last line back to the last `\n`. It
//! rotates before a chunk would reach [`rotate_at`] and at a UTC date change,
//! so a chunk never becomes an LFS object, and it stores a body over 16 KiB as
//! an immutable blob written and `fsync`ed before its line. It refuses a
//! symlinked `log/` (or `blobs/`, or chunk): the caller resolved the session
//! folder through `browse::resolve`, and a link inside it could point anywhere.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use sha2::{Digest, Sha256};

use super::{
    BlobRef, ChunkName, HostSlug, LineBody, LogError, LogLine, BLOBS_DIR, BLOB_OVER_BYTES, LOG_DIR,
    MAX_LINE_BYTES,
};
use crate::agents::redact::redact_secrets;

/// The size a chunk never reaches: `min(192 KiB, 3/4 × lfs_threshold_bytes)`.
pub fn rotate_at(lfs_threshold_bytes: u64) -> u64 {
    (192 * 1024).min(lfs_threshold_bytes.saturating_mul(3) / 4)
}

/// Where a line landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinePlace {
    /// The chunk it is in.
    pub chunk: ChunkName,
    /// The byte offset of its first byte in that chunk.
    pub offset: u64,
    /// Its length, newline included.
    pub bytes: u64,
}

/// What [`ChunkWriter::append`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendReceipt {
    /// The chunk the line is in.
    pub chunk: ChunkName,
    /// The byte offset of the line in that chunk.
    pub offset: u64,
    /// The line's length, newline included.
    pub bytes: u64,
    /// The line as the log holds it, after redaction and with its body
    /// inline (never a blob reference), so a caller can keep an in-memory
    /// history equal to what a later read and hydrate would give.
    pub line: LogLine,
    /// The blob the body went to, when it went to one.
    pub blob: Option<String>,
}

struct Current {
    name: ChunkName,
    file: File,
    bytes: u64,
    dirty: bool,
}

/// The one writer of one host's chunks of one session.
pub struct ChunkWriter {
    log_dir: PathBuf,
    host: HostSlug,
    rotate_at: u64,
    current: Option<Current>,
}

/// Whether `path` is a real directory (`Ok(false)`: absent); a link or
/// anything else there is refused. Every folder of a session keeper writes
/// in — `log/`, `approvals/` — is checked this way, never followed.
pub fn refuse_symlink(path: &Path) -> Result<bool, LogError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(LogError::Symlink {
            path: path.display().to_string(),
        }),
        Ok(meta) if !meta.is_dir() => Err(LogError::NotADirectory {
            path: path.display().to_string(),
        }),
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(LogError::io(path, error)),
    }
}

/// A real directory at `path`, made if absent; a link or a file is refused,
/// one that appears while it is made too.
pub fn real_dir(path: &Path) -> Result<(), LogError> {
    if !refuse_symlink(path)? {
        match fs::create_dir(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                refuse_symlink(path)?;
            }
            Err(error) => return Err(LogError::io(path, error)),
        }
    }
    Ok(())
}

impl ChunkWriter {
    /// Open `host`'s writer in `session_dir`. The host's newest chunk,
    /// whatever its date, has its torn tail truncated (it is this host's own,
    /// and a tail torn yesterday would otherwise stay torn on every host); it
    /// is resumed when it is today's.
    pub fn open(
        session_dir: &Path,
        host: &HostSlug,
        rotate_at: u64,
        today: NaiveDate,
    ) -> Result<ChunkWriter, LogError> {
        // Below this a body small enough to stay inline could make a line no
        // chunk can hold.
        let least = 2 * BLOB_OVER_BYTES as u64;
        if rotate_at < least {
            return Err(LogError::RotateTooSmall { rotate_at, least });
        }
        let log_dir = session_dir.join(LOG_DIR);
        real_dir(&log_dir)?;
        let mut writer = ChunkWriter {
            log_dir,
            host: host.clone(),
            rotate_at,
            current: None,
        };
        if let Some(name) = writer.newest_own()? {
            let current = writer.open_chunk(name, true)?;
            if current.name.date == today {
                writer.current = Some(current);
            }
        }
        Ok(writer)
    }

    /// The chunk the next line would go to, if one is open.
    pub fn current_chunk(&self) -> Option<&ChunkName> {
        self.current.as_ref().map(|current| &current.name)
    }

    /// This host's chunks, by name.
    fn own_chunks(&self) -> Result<Vec<ChunkName>, LogError> {
        let entries = fs::read_dir(&self.log_dir).map_err(|e| LogError::io(&self.log_dir, e))?;
        let mut own = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| LogError::io(&self.log_dir, e))?;
            let Some(name) = entry
                .file_name()
                .to_str()
                .and_then(|n| n.parse::<ChunkName>().ok())
            else {
                continue;
            };
            if name.host == self.host {
                own.push(name);
            }
        }
        Ok(own)
    }

    /// This host's newest chunk of any date.
    fn newest_own(&self) -> Result<Option<ChunkName>, LogError> {
        Ok(self.own_chunks()?.into_iter().max())
    }

    /// The highest `n` of this host's chunks dated `date`.
    fn newest(&self, date: NaiveDate) -> Result<Option<u32>, LogError> {
        Ok(self
            .own_chunks()?
            .into_iter()
            .filter(|name| name.date == date)
            .map(|name| name.n)
            .max())
    }

    fn open_chunk(&self, name: ChunkName, existing: bool) -> Result<Current, LogError> {
        if name.host != self.host {
            return Err(LogError::ForeignHost {
                line: name.host.to_string(),
                writer: self.host.to_string(),
            });
        }
        let path = self.log_dir.join(name.to_string());
        if let Ok(meta) = fs::symlink_metadata(&path) {
            if !meta.file_type().is_file() {
                return Err(LogError::Symlink {
                    path: path.display().to_string(),
                });
            }
        }
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|e| LogError::io(&path, e))?;
        // The file held open must be the regular file at `path`: a link
        // swapped in between the check above and the open is refused too.
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let held = file.metadata().map_err(|e| LogError::io(&path, e))?;
            let at = fs::symlink_metadata(&path).map_err(|e| LogError::io(&path, e))?;
            if !at.file_type().is_file() || (held.dev(), held.ino()) != (at.dev(), at.ino()) {
                return Err(LogError::Symlink {
                    path: path.display().to_string(),
                });
            }
        }
        if !existing {
            // A new chunk's directory entry is made durable with it, so a
            // turn synced into it cannot vanish with the entry on a crash.
            File::open(&self.log_dir)
                .and_then(|dir| dir.sync_all())
                .map_err(|e| LogError::io(&self.log_dir, e))?;
        }
        let mut bytes = 0;
        if existing {
            let mut content = Vec::new();
            file.read_to_end(&mut content)
                .map_err(|e| LogError::io(&path, e))?;
            let keep = content
                .iter()
                .rposition(|b| *b == b'\n')
                .map_or(0, |at| at + 1);
            if keep < content.len() {
                // A torn tail: the host died mid-write. The line was never
                // complete, so no reader ever took it.
                file.set_len(keep as u64)
                    .map_err(|e| LogError::io(&path, e))?;
                file.sync_all().map_err(|e| LogError::io(&path, e))?;
            }
            bytes = keep as u64;
        }
        Ok(Current {
            name,
            file,
            bytes,
            dirty: false,
        })
    }

    /// The chunk a `len`-byte line dated `date` goes to, rotating first if
    /// the current one is of another date or would reach `rotate_at`.
    fn chunk_for(&mut self, date: NaiveDate, len: u64) -> Result<&mut Current, LogError> {
        let rotate = match &self.current {
            None => true,
            Some(current) => current.name.date != date || current.bytes + len >= self.rotate_at,
        };
        if rotate {
            let n = match &self.current {
                Some(current) if current.name.date == date => current.name.n + 1,
                _ => self.newest(date)?.map_or(1, |n| n + 1),
            };
            self.sync()?;
            let name = ChunkName {
                date,
                host: self.host.clone(),
                n,
            };
            let fresh = self.open_chunk(name, false)?;
            self.current = Some(fresh);
        }
        match self.current.as_mut() {
            Some(current) => Ok(current),
            None => Err(LogError::NotADirectory {
                path: self.log_dir.display().to_string(),
            }),
        }
    }

    fn write_at(&mut self, date: NaiveDate, raw: &str) -> Result<LinePlace, LogError> {
        if raw.contains('\n') {
            return Err(LogError::Newline);
        }
        let mut buffer = Vec::with_capacity(raw.len() + 1);
        buffer.extend_from_slice(raw.as_bytes());
        buffer.push(b'\n');
        let len = buffer.len() as u64;
        let limit = MAX_LINE_BYTES.min(self.rotate_at.saturating_sub(1) as usize);
        if buffer.len() > limit {
            return Err(LogError::LineTooLong {
                bytes: buffer.len(),
                limit,
            });
        }
        let log_dir = self.log_dir.clone();
        let current = self.chunk_for(date, len)?;
        let offset = current.bytes;
        current
            .file
            .write_all(&buffer)
            .map_err(|e| LogError::io(&log_dir.join(current.name.to_string()), e))?;
        current.bytes += len;
        current.dirty = true;
        Ok(LinePlace {
            chunk: current.name.clone(),
            offset,
            bytes: len,
        })
    }

    /// Append one raw line (no newline in it) dated `date` (UTC), with the
    /// same rotation and bounds as [`ChunkWriter::append`]: a date change
    /// starts a new chunk. The primitive Epic 95's journal writer reuses.
    pub fn append_line(&mut self, date: NaiveDate, raw: &str) -> Result<LinePlace, LogError> {
        self.write_at(date, raw)
    }

    /// Append `line`: redact every string in its body, move a large body to
    /// a blob, and write it as one line in the chunk of its UTC date.
    pub fn append(&mut self, line: &LogLine) -> Result<AppendReceipt, LogError> {
        if line.host != self.host {
            return Err(LogError::ForeignHost {
                line: line.host.to_string(),
                writer: self.host.to_string(),
            });
        }
        let mut written = line.clone();
        written.body = redacted(&line.body)?;

        let body_json = serde_json::to_string(&written.body)?;
        let blob =
            if body_json.len() > BLOB_OVER_BYTES && !matches!(written.body, LineBody::Blob(_)) {
                Some(self.write_blob(body_json.as_bytes())?)
            } else {
                None
            };
        let raw = match &blob {
            Some(sha256) => {
                let mut stored = written.clone();
                stored.body = LineBody::Blob(BlobRef {
                    kind: written.kind(),
                    sha256: sha256.clone(),
                    bytes: body_json.len() as u64,
                });
                stored.to_json()?
            }
            None => written.to_json()?,
        };
        let place = self.write_at(written.ts.date_naive(), &raw)?;
        Ok(AppendReceipt {
            chunk: place.chunk,
            offset: place.offset,
            bytes: place.bytes,
            line: written,
            blob,
        })
    }

    /// Store `bytes` as `log/blobs/<sha256>.json`, durably, before the line
    /// that names it. An existing blob of that name is the same bytes.
    fn write_blob(&self, bytes: &[u8]) -> Result<String, LogError> {
        let sha256 = hex::encode(Sha256::digest(bytes));
        let dir = self.log_dir.join(BLOBS_DIR);
        real_dir(&dir)?;
        let path = dir.join(format!("{sha256}.json"));
        if let Ok(meta) = fs::symlink_metadata(&path) {
            if meta.file_type().is_file() {
                return Ok(sha256);
            }
            return Err(LogError::Symlink {
                path: path.display().to_string(),
            });
        }
        let temp = dir.join(format!(".{sha256}.{}.tmp", ulid::Ulid::new()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| LogError::io(&temp, e))?;
        file.write_all(bytes).map_err(|e| LogError::io(&temp, e))?;
        file.sync_all().map_err(|e| LogError::io(&temp, e))?;
        fs::rename(&temp, &path).map_err(|e| LogError::io(&path, e))?;
        File::open(&dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| LogError::io(&dir, e))?;
        Ok(sha256)
    }

    /// `fsync` what this writer has written since the last sync.
    pub fn sync(&mut self) -> Result<(), LogError> {
        if let Some(current) = self.current.as_mut() {
            if current.dirty {
                current
                    .file
                    .sync_data()
                    .map_err(|e| LogError::io(&self.log_dir.join(current.name.to_string()), e))?;
                current.dirty = false;
            }
        }
        Ok(())
    }
}

/// The body with every string in it passed through [`redact_secrets`]:
/// whatever field a model, a person or a tool put a secret into — a message,
/// a tool call's arguments, a summary, an error — it does not reach the chunk.
/// A blob reference is the writer's own and is left alone.
fn redacted(body: &LineBody) -> Result<LineBody, LogError> {
    if matches!(body, LineBody::Blob(_)) {
        return Ok(body.clone());
    }
    let mut value = serde_json::to_value(body)?;
    if !redact_strings(&mut value) {
        return Ok(body.clone());
    }
    Ok(LineBody::decode(body.kind(), value)?)
}

/// Redact every string leaf in place; whether anything was replaced.
fn redact_strings(value: &mut serde_json::Value) -> bool {
    use serde_json::Value;
    match value {
        Value::String(text) => {
            let found = redact_secrets(text);
            if found.found.is_empty() {
                false
            } else {
                *text = found.text;
                true
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .fold(false, |any, item| redact_strings(item) | any),
        Value::Object(map) => map
            .values_mut()
            .fold(false, |any, item| redact_strings(item) | any),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}
