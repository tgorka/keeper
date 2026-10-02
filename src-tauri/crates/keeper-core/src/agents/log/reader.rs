//! Reading a session's log: every chunk of every host, merged (AD-366).
//!
//! The reader never changes a byte. A torn last line — another host's, or a
//! host's own before it reopened — is skipped with a problem entry; a line it
//! cannot read is skipped the same way, never a panic. Lines merge by
//! (`ts`, `host`, `id`). The epoch fence then drops a superseded writer's late
//! lines: once a `claim acquired` at epoch E lands at time T, a line of an
//! epoch below E written after T is a stale host still appending (AD-378).
//! Two `acquired` claims at one epoch with different claim events mean two
//! hosts both believed they held the session: the log is marked conflicted
//! and replay refuses it.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{ChunkName, ClaimAction, LineBody, LogError, LogLine, BLOBS_DIR, LOG_DIR};

/// One chunk as the reader found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkInfo {
    /// Its name.
    pub name: ChunkName,
    /// Its size on disk.
    pub bytes: u64,
    /// The offset of its last complete line, or 0 when it has none.
    pub last_offset: u64,
}

/// Something the reader skipped or could not reconcile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogProblem {
    /// The chunk's file name, or `log/` for the folder itself.
    pub chunk: String,
    /// The 1-based line in that chunk, when it is about one line.
    pub line: Option<usize>,
    /// What happened, as a sentence.
    pub sentence: String,
}

/// A session's log, merged.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionLog {
    /// Every readable line that survived the fence, in (`ts`, `host`, `id`) order.
    pub lines: Vec<LogLine>,
    /// Every chunk, in name order.
    pub chunks: Vec<ChunkInfo>,
    /// What was skipped, and why.
    pub problems: Vec<LogProblem>,
    /// Two hosts acquired the same epoch with different claim events.
    pub conflicted: bool,
}

/// Read every chunk of `session_dir/log/`.
pub fn read_session(session_dir: &Path) -> SessionLog {
    let mut log = SessionLog::default();
    let log_dir = session_dir.join(LOG_DIR);
    match fs::symlink_metadata(&log_dir) {
        Ok(meta) if meta.file_type().is_symlink() => {
            log.problems.push(LogProblem {
                chunk: format!("{LOG_DIR}/"),
                line: None,
                sentence:
                    "log/ is a symbolic link; keeper does not read a session's log through one."
                        .to_owned(),
            });
            return log;
        }
        Ok(_) => {}
        Err(_) => return log,
    }
    let entries = match fs::read_dir(&log_dir) {
        Ok(entries) => entries,
        Err(error) => {
            log.problems.push(LogProblem {
                chunk: format!("{LOG_DIR}/"),
                line: None,
                sentence: format!("log/ could not be listed: {error}."),
            });
            return log;
        }
    };
    let mut names: Vec<ChunkName> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name: ChunkName = entry.file_name().to_str()?.parse().ok()?;
            if entry.file_type().is_ok_and(|t| t.is_file()) {
                Some(name)
            } else {
                // A link or a folder under a chunk's name is not read, and
                // says so rather than vanishing from the session.
                log.problems.push(LogProblem {
                    chunk: name.to_string(),
                    line: None,
                    sentence: "It is not a regular file, so it was not read.".to_owned(),
                });
                None
            }
        })
        .collect();
    names.sort();

    let mut lines = Vec::new();
    for name in names {
        let file = name.to_string();
        let content = match fs::read(log_dir.join(&file)) {
            Ok(content) => content,
            Err(error) => {
                log.problems.push(LogProblem {
                    chunk: file,
                    line: None,
                    sentence: format!("The chunk could not be read: {error}."),
                });
                continue;
            }
        };
        let mut offset = 0usize;
        let mut last_offset = 0u64;
        let mut number = 0usize;
        while offset < content.len() {
            number += 1;
            let Some(end) = content[offset..].iter().position(|b| *b == b'\n') else {
                log.problems.push(LogProblem {
                    chunk: file.clone(),
                    line: Some(number),
                    sentence: "The chunk ends in half a line; it was skipped and left as it is."
                        .to_owned(),
                });
                break;
            };
            let raw = &content[offset..offset + end];
            last_offset = offset as u64;
            match std::str::from_utf8(raw)
                .map_err(|_| "it is not UTF-8".to_owned())
                .and_then(|text| LogLine::parse(text).map_err(|p| p.to_string()))
            {
                Ok(line) => lines.push(Placed {
                    line,
                    chunk: file.clone(),
                    number,
                }),
                Err(why) => log.problems.push(LogProblem {
                    chunk: file.clone(),
                    line: Some(number),
                    sentence: format!("A line was skipped: {why}."),
                }),
            }
            offset += end + 1;
        }
        log.chunks.push(ChunkInfo {
            name,
            bytes: content.len() as u64,
            last_offset,
        });
    }

    lines.sort_by(|a, b| {
        (a.line.ts, &a.line.host, a.line.id).cmp(&(b.line.ts, &b.line.host, b.line.id))
    });
    fence(&mut log, lines);
    log
}

/// A line and where it was read.
struct Placed {
    line: LogLine,
    chunk: String,
    number: usize,
}

/// Drop superseded epochs' late lines and detect a double acquire.
fn fence(log: &mut SessionLog, lines: Vec<Placed>) {
    // epoch -> (when it was first acquired, by which claim event)
    let mut acquired: BTreeMap<u64, (DateTime<Utc>, String)> = BTreeMap::new();
    for Placed { line, .. } in &lines {
        let LineBody::Claim(claim) = &line.body else {
            continue;
        };
        if claim.action != ClaimAction::Acquired {
            continue;
        }
        match acquired.get(&claim.epoch) {
            None => {
                acquired.insert(claim.epoch, (line.ts, claim.claim_event.clone()));
            }
            Some((_, event)) if *event != claim.claim_event => {
                log.conflicted = true;
                log.problems.push(LogProblem {
                    chunk: format!("{LOG_DIR}/"),
                    line: None,
                    sentence: format!(
                        "Two hosts acquired epoch {} with different claim events ({event} and {}); the session is conflicted.",
                        claim.epoch, claim.claim_event
                    ),
                });
            }
            Some(_) => {}
        }
    }
    for Placed {
        line,
        chunk,
        number,
    } in lines
    {
        // A claim transition is never fenced: a loser's `lost` is written
        // after the winner's `acquired` by its nature, and it is the record
        // that the loser noticed.
        if matches!(line.body, LineBody::Claim(_)) {
            log.lines.push(line);
            continue;
        }
        let superseded_at = acquired
            .range(line.epoch.saturating_add(1)..)
            .map(|(_, (ts, _))| *ts)
            .min();
        match superseded_at {
            Some(at) if line.ts > at => log.problems.push(LogProblem {
                chunk,
                line: Some(number),
                sentence: format!(
                    "Line {} from {} at epoch {} was written after a newer epoch was acquired; it was dropped.",
                    line.id, line.host, line.epoch
                ),
            }),
            _ => log.lines.push(line),
        }
    }
}

/// Read and verify `log/blobs/<sha256>.json`.
pub fn hydrate_blob(session_dir: &Path, sha256: &str) -> Result<Value, LogError> {
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(LogError::BadBlobName {
            name: sha256.to_owned(),
        });
    }
    let path = session_dir
        .join(LOG_DIR)
        .join(BLOBS_DIR)
        .join(format!("{sha256}.json"));
    let bytes = fs::read(&path).map_err(|e| LogError::io(&path, e))?;
    if hex::encode(Sha256::digest(&bytes)) != sha256 {
        return Err(LogError::BlobMismatch {
            name: sha256.to_owned(),
        });
    }
    serde_json::from_slice(&bytes).map_err(|e| LogError::BlobNotJson {
        name: sha256.to_owned(),
        detail: e.to_string(),
    })
}
