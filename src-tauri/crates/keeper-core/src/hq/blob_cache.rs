//! This device's cache of one database's blobs (SPEC §5.9 B1–B4; FR-859):
//! how many bytes it may keep, which bodies go first when it holds more, and
//! when each body was last used.
//!
//! The bodies themselves live in the replica file, where the client puts a
//! blob it uploads and one it downloads; the client's own cap
//! (`blob_cache_max_bytes`) is set from [`BLOB_CACHE_MAX_BYTES`] and drops
//! only bodies no row references. An attachment a person opened stays
//! referenced by its row for as long as the session is held, so that cap
//! alone would let the cache grow with every file opened. keeper therefore
//! evicts by its own rule: once the cached bodies pass the cap, the
//! least recently used body the outbox does not pin goes first, referenced
//! or not — the next open downloads it again. A body the outbox pins (staged
//! for upload, or referenced by a commit the server has not taken) is never
//! evicted, even when the pinned bodies alone pass the cap: it is the only
//! copy of something the person wrote. [`evictions`] is that rule; the
//! adapter (`hq::syncular`) reads the facts from the replica file and drops
//! what it names. When each body was last used is keeper's own record, in a
//! small SQLite file beside the replica ([`AccessLog`]) — and so is every
//! refusal of a blob's upload the server sent, tied to the commits that
//! referenced the blob, since resolving one takes the blob out of the
//! replica's queue and nothing there would say why afterwards, and the blobs
//! a commit is staging until its message is in the outbox, so a process that
//! dies between the two leaves nothing queued for upload that no message
//! names (R1071K-07).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::hq::port::{Rejection, UploadRefusal};

/// The most bytes of blob bodies one database's replica keeps beyond what its
/// outbox pins: ten of the largest attachments a message may carry
/// (`attachmentMaxBytes` in `syncular/limits.json`, 25 MiB). Ten keep a
/// session's recent files open without a download each time while costing a
/// phone at most a quarter of a gigabyte per database (`docs/agents.md`,
/// *Files in an agents session*).
pub const BLOB_CACHE_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// One body in the cache, as eviction weighs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedBlob {
    pub blob_id: String,
    pub bytes: u64,
    /// Staged for upload, or referenced by a commit still in the outbox.
    pub pinned: bool,
    /// When it was last opened or written here (ms); a body never opened
    /// counts from when it was cached.
    pub used_at_ms: i64,
}

/// The bodies to drop so what stays cached is at most `max` bytes: unpinned
/// ones only, least recently used first (ties by id, so the order is
/// stable). Empty while the cache is within `max`; never a pinned body, so
/// what stays may exceed `max` by what the outbox pins.
pub fn evictions(blobs: &[CachedBlob], max: u64) -> Vec<String> {
    let mut total: u64 = blobs.iter().map(|blob| blob.bytes).sum();
    if total <= max {
        return Vec::new();
    }
    let mut candidates: Vec<&CachedBlob> = blobs.iter().filter(|blob| !blob.pinned).collect();
    candidates.sort_by(|a, b| {
        a.used_at_ms
            .cmp(&b.used_at_ms)
            .then_with(|| a.blob_id.cmp(&b.blob_id))
    });
    let mut dropped = Vec::new();
    for blob in candidates {
        if total <= max {
            break;
        }
        total = total.saturating_sub(blob.bytes);
        dropped.push(blob.blob_id.clone());
    }
    dropped
}

/// A blob a commit stages, and how the replica held it before: whether its
/// body was cached and whether it was already queued for upload. What putting
/// it back as it was needs, after a commit that failed or never finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedBlob {
    pub blob_id: String,
    pub body: bool,
    pub queued: bool,
}

/// When each blob of one replica was last used on this device, the server's
/// refusals of blob uploads, and the waiting commits set aside in advance:
/// keeper's own file beside the replica (`<replica>.blob-access.sqlite`),
/// never inside the client's, so a schema change there cannot take it and
/// nothing here is mistaken for a synced table. Losing it costs the order (a
/// body with no record counts from when it was cached), the reason a refused
/// file shows, the record of a commit a crash cut short, and a set-aside
/// commit's refusal, which then stands until a later write sets it aside.
pub struct AccessLog {
    conn: Connection,
}

impl AccessLog {
    /// The log of the replica at `replica`.
    pub fn beside(replica: &Path) -> PathBuf {
        replica.with_extension("blob-access.sqlite")
    }

    pub fn open(path: &Path) -> Result<AccessLog, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS used (blob_id TEXT PRIMARY KEY, at_ms INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS upload_refusals (
               commit_id TEXT NOT NULL, blob_id TEXT NOT NULL,
               code TEXT NOT NULL, message TEXT NOT NULL,
               refused_rows TEXT NOT NULL, commit_rows TEXT NOT NULL,
               at_ms INTEGER NOT NULL,
               PRIMARY KEY (commit_id, blob_id));
             CREATE TABLE IF NOT EXISTS unfinished_commit (
               blob_id TEXT PRIMARY KEY, body INTEGER NOT NULL, queued INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS superseded_waiting (
               commit_id TEXT PRIMARY KEY, replacement TEXT NOT NULL);",
        )?;
        Ok(AccessLog { conn })
    }

    /// A commit is about to stage `blobs`: kept, all or none, before the
    /// first is staged, so a process that dies before the commit's message is
    /// in the outbox leaves the record its next open puts them back by. A
    /// blob named twice keeps its first state, the one before either staged.
    pub fn begin_commit(&self, blobs: &[StagedBlob]) -> Result<(), rusqlite::Error> {
        let tx = self.conn.unchecked_transaction()?;
        for blob in blobs {
            tx.execute(
                "INSERT OR IGNORE INTO unfinished_commit (blob_id, body, queued) VALUES (?1, ?2, ?3)",
                params![blob.blob_id, blob.body, blob.queued],
            )?;
        }
        tx.commit()
    }

    /// The blobs of a commit that began and never finished.
    pub fn unfinished_commit(&self) -> Result<Vec<StagedBlob>, rusqlite::Error> {
        let mut statement = self
            .conn
            .prepare("SELECT blob_id, body, queued FROM unfinished_commit ORDER BY blob_id")?;
        let rows = statement.query_map([], |row| {
            Ok(StagedBlob {
                blob_id: row.get(0)?,
                body: row.get(1)?,
                queued: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    /// The commit finished — its message is in the outbox, or what it staged
    /// is back as it was.
    pub fn finish_commit(&self) -> Result<(), rusqlite::Error> {
        self.conn.execute("DELETE FROM unfinished_commit", [])?;
        Ok(())
    }

    /// Keep `refusals` (a later refusal of the same blob for the same
    /// commit replaces the earlier), all or none.
    pub fn refuse(&self, refusals: &[UploadRefusal], at_ms: i64) -> Result<(), rusqlite::Error> {
        let tx = self.conn.unchecked_transaction()?;
        for refusal in refusals {
            tx.execute(
                "INSERT INTO upload_refusals \
                 (commit_id, blob_id, code, message, refused_rows, commit_rows, at_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
                 ON CONFLICT(commit_id, blob_id) DO UPDATE SET code = excluded.code, \
                 message = excluded.message, refused_rows = excluded.refused_rows, \
                 commit_rows = excluded.commit_rows, at_ms = excluded.at_ms",
                params![
                    refusal.commit_id,
                    refusal.blob_id,
                    refusal.rejection.code,
                    refusal.rejection.message,
                    serde_json::to_string(&refusal.refused_rows).unwrap_or_default(),
                    serde_json::to_string(&refusal.commit_rows).unwrap_or_default(),
                    at_ms,
                ],
            )?;
        }
        tx.commit()
    }

    /// Every refusal kept, oldest first.
    pub fn refusals(&self) -> Result<Vec<UploadRefusal>, rusqlite::Error> {
        let mut statement = self.conn.prepare(
            "SELECT commit_id, blob_id, code, message, refused_rows, commit_rows \
             FROM upload_refusals ORDER BY at_ms, commit_id, blob_id",
        )?;
        let rows = statement.query_map([], |row| {
            let ids = |raw: String| serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default();
            Ok(UploadRefusal {
                commit_id: row.get(0)?,
                blob_id: row.get(1)?,
                rejection: Rejection::new(row.get::<_, String>(2)?, row.get::<_, String>(3)?),
                refused_rows: ids(row.get(4)?),
                commit_rows: ids(row.get(5)?),
            })
        })?;
        rows.collect()
    }

    /// The refusals of `commit_ids` are moot: their commits are neither
    /// waiting nor standing refused any more.
    pub fn drop_refusals(&self, commit_ids: &[String]) -> Result<(), rusqlite::Error> {
        let mut statement = self
            .conn
            .prepare("DELETE FROM upload_refusals WHERE commit_id = ?1")?;
        for commit_id in commit_ids {
            statement.execute(params![commit_id])?;
        }
        Ok(())
    }

    /// `blob_id` was opened or written now (`at_ms`).
    pub fn touch(&self, blob_id: &str, at_ms: i64) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO used (blob_id, at_ms) VALUES (?1, ?2) \
             ON CONFLICT(blob_id) DO UPDATE SET at_ms = max(at_ms, excluded.at_ms)",
            params![blob_id, at_ms],
        )?;
        Ok(())
    }

    /// Every recorded last use.
    pub fn used(&self) -> Result<HashMap<String, i64>, rusqlite::Error> {
        let mut statement = self.conn.prepare("SELECT blob_id, at_ms FROM used")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    /// The bodies dropped: their records go with them.
    pub fn forget(&self, blob_ids: &[String]) -> Result<(), rusqlite::Error> {
        let mut statement = self.conn.prepare("DELETE FROM used WHERE blob_id = ?1")?;
        for blob_id in blob_ids {
            statement.execute(params![blob_id])?;
        }
        Ok(())
    }

    /// `commit_ids`, still waiting in the outbox, are set aside by
    /// `replacement` in advance: a refusal arriving for one never stands.
    /// A later replacement of the same commit replaces the earlier.
    pub fn supersede_waiting(
        &self,
        commit_ids: &[String],
        replacement: &str,
    ) -> Result<(), rusqlite::Error> {
        let tx = self.conn.unchecked_transaction()?;
        for commit_id in commit_ids {
            tx.execute(
                "INSERT INTO superseded_waiting (commit_id, replacement) VALUES (?1, ?2) \
                 ON CONFLICT(commit_id) DO UPDATE SET replacement = excluded.replacement",
                params![commit_id, replacement],
            )?;
        }
        tx.commit()
    }

    /// Every commit set aside in advance, with its replacement.
    pub fn superseded_waiting(&self) -> Result<HashMap<String, String>, rusqlite::Error> {
        let mut statement = self
            .conn
            .prepare("SELECT commit_id, replacement FROM superseded_waiting")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    /// The outcomes of `commit_ids` are settled: their records go.
    pub fn drop_superseded_waiting(&self, commit_ids: &[String]) -> Result<(), rusqlite::Error> {
        let mut statement = self
            .conn
            .prepare("DELETE FROM superseded_waiting WHERE commit_id = ?1")?;
        for commit_id in commit_ids {
            statement.execute(params![commit_id])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(id: &str, bytes: u64, pinned: bool, used_at_ms: i64) -> CachedBlob {
        CachedBlob {
            blob_id: id.to_owned(),
            bytes,
            pinned,
            used_at_ms,
        }
    }

    #[test]
    fn past_the_cap_the_least_recently_used_unpinned_bodies_go_first_and_never_a_pinned_one() {
        let cache = [
            blob("old-pinned", 40, true, 1),
            blob("opened-long-ago", 30, false, 2),
            blob("opened-just-now", 30, false, 9),
            blob("opened-a-while-ago", 30, false, 5),
        ];
        // 130 cached, cap 80: the two least recently used unpinned bodies go
        // (130 - 30 - 30 = 70), the one opened just now stays, the pinned
        // one stays though it is the oldest.
        assert_eq!(
            evictions(&cache, 80),
            ["opened-long-ago", "opened-a-while-ago"]
        );
        // Within the cap nothing goes.
        assert!(evictions(&cache, 130).is_empty());
        // Pinned bodies alone past the cap: every unpinned one goes, and
        // nothing pinned does.
        assert_eq!(
            evictions(&cache, 10),
            ["opened-long-ago", "opened-a-while-ago", "opened-just-now"]
        );
        assert!(evictions(&[blob("only-pinned", 500, true, 0)], 10).is_empty());
    }

    #[test]
    fn the_last_use_survives_a_reopen_and_only_moves_forward() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = AccessLog::beside(&dir.path().join("ana.sqlite"));
        assert!(path.ends_with("ana.blob-access.sqlite"));
        let log = AccessLog::open(&path).expect("opens");
        log.touch("a", 10).expect("touch");
        log.touch("b", 20).expect("touch");
        log.touch("a", 30).expect("touch");
        log.touch("b", 5)
            .expect("an older use does not move it back");
        drop(log);
        let log = AccessLog::open(&path).expect("reopens");
        let used = log.used().expect("reads");
        assert_eq!(used.get("a"), Some(&30));
        assert_eq!(used.get("b"), Some(&20));
        log.forget(&["a".to_owned()]).expect("forgets");
        assert_eq!(log.used().expect("reads").len(), 1);
    }
}
