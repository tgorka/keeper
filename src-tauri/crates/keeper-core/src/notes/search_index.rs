//! Disposable retrieval beside the model, never its replacement (AD-261…265).
//!
//! The content-owning FTS and explicit integer identities follow recordings_fts
//! (research §4.8); keeper folds both sides and owns raw-text marks (§4.3).
//! Vectors remain f32 LE SQLite blobs: the bounded exact scan buys atomic updates
//! without an ANN dependency (§7.1). Fusion follows pi-knowledge §4 / research §8:
//! pool-relative scores, a lexical anchor, and no weak semantic padding.

use super::{
    chunk::{self, Chunk},
    search,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BinaryHeap, HashMap, HashSet},
    path::Path,
    time::Duration,
};

pub const SEARCH_DB_FILE: &str = "search.db";
pub const SEARCH_SCHEMA: u32 = 2;
pub const LEXICAL_POOL: usize = 1000;
pub const VECTOR_POOL: usize = 50;
pub const TITLE_WEIGHT: f64 = 4.0;
pub const LEX_WEIGHT: f32 = 0.45;
pub const VEC_WEIGHT: f32 = 0.55;
pub const OVERLAP_BONUS: f32 = 0.15;
pub const MIN_HYBRID_SCORE: f32 = 0.18;
pub const MIN_MEANING_COSINE: f32 = 0.5;

#[derive(Debug, thiserror::Error)]
pub enum SearchIndexError {
    #[error("notes search SQLite: {0}")]
    Sqlite(String),
    #[error("notes search IO: {0}")]
    Io(String),
    #[error("notes search vector: {0}")]
    Vector(String),
}
impl From<rusqlite::Error> for SearchIndexError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e.to_string())
    }
}
impl From<std::io::Error> for SearchIndexError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

pub struct NoteDoc<'a> {
    pub id: &'a str,
    pub path: &'a str,
    pub title: &'a str,
    pub tags: &'a [String],
    pub fields: &'a BTreeMap<String, String>,
    pub body: &'a str,
    pub stat: Option<&'a str>,
}
pub struct SearchIndex {
    conn: Connection,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChunk {
    pub rowid: i64,
    pub text_hash: String,
    pub embedding_text: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexStats {
    pub notes: u32,
    pub chunks: u32,
    pub vectors: u32,
    pub model: Option<String>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkHit {
    pub note_id: String,
    pub ordinal: u32,
    pub chunk_rowid: i64,
    pub score: f32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchWhy {
    Words,
    Meaning,
    Both,
}
impl MatchWhy {
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Words => "words",
            Self::Meaning => "meaning",
            Self::Both => "both",
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct NoteScore {
    pub note_id: String,
    pub ordinal: u32,
    pub chunk_rowid: i64,
    pub score: f32,
    pub why: MatchWhy,
}

const DDL: &str = "
CREATE TABLE IF NOT EXISTS meta(schema INTEGER, vault_id TEXT);
CREATE TABLE IF NOT EXISTS notes(id TEXT PRIMARY KEY, path TEXT, title TEXT, text_hash TEXT, stat TEXT);
CREATE TABLE IF NOT EXISTS chunks(rowid INTEGER PRIMARY KEY, note_id TEXT REFERENCES notes(id) ON DELETE CASCADE,
 ordinal INTEGER, heading TEXT, byte_start INTEGER, byte_end INTEGER, text TEXT, text_hash TEXT, UNIQUE(note_id, ordinal));
CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(title, text, tokenize='unicode61');
CREATE TABLE IF NOT EXISTS vectors(chunk_rowid INTEGER PRIMARY KEY REFERENCES chunks(rowid) ON DELETE CASCADE, model TEXT, dim INTEGER, vec BLOB);
CREATE TRIGGER IF NOT EXISTS chunks_delete_fts AFTER DELETE ON chunks BEGIN
 DELETE FROM chunks_fts WHERE rowid=old.rowid; END;";

impl SearchIndex {
    pub fn open(path: &Path, vault_id: &str) -> Result<Self, SearchIndexError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let existed = path.exists();
        let opened = (|| -> rusqlite::Result<Connection> {
            let conn = writer(path)?;
            if existed {
                let meta: Option<(u32, String)> = conn
                    .query_row("SELECT schema, vault_id FROM meta LIMIT 1", [], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })
                    .optional()?;
                if meta
                    .as_ref()
                    .is_none_or(|(schema, id)| *schema != SEARCH_SCHEMA || id != vault_id)
                {
                    return Err(rusqlite::Error::InvalidQuery);
                }
            }
            Ok(conn)
        })();
        let mut conn = match opened {
            Ok(conn) => conn,
            Err(error) if existed && disposable_error(&error) => {
                tracing::warn!("discarding corrupt or incompatible notes search index");
                std::fs::remove_file(path)?;
                // SQLite normally removes these on last close. Remove leftovers too,
                // never an adjacent user file or the enclosing .keeper directory.
                for suffix in ["-wal", "-shm"] {
                    let mut name = path.as_os_str().to_os_string();
                    name.push(suffix);
                    match std::fs::remove_file(std::path::PathBuf::from(name)) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e.into()),
                    }
                }
                writer(path)?
            }
            Err(error) => return Err(error.into()),
        };
        let tx = conn.transaction()?;
        tx.execute_batch(DDL)?;
        tx.execute(
            "INSERT INTO meta(schema,vault_id) SELECT ?1,?2 WHERE NOT EXISTS(SELECT 1 FROM meta)",
            params![SEARCH_SCHEMA, vault_id],
        )?;
        tx.commit()?;
        Ok(Self { conn })
    }

    pub fn open_read_only(path: &Path) -> Result<Self, SearchIndexError> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        // Ranking and snippet reads share one revision even if the reconciler
        // replaces a note between them. Dropping this fresh reader ends the snapshot.
        conn.execute_batch("BEGIN DEFERRED")?;
        Ok(Self { conn })
    }

    pub fn replace_note(&mut self, doc: &NoteDoc<'_>) -> Result<bool, SearchIndexError> {
        let head = chunk::head_text(doc.title, doc.path, doc.tags, doc.fields);
        let hash = hash_parts(&[doc.title, doc.path, &head, doc.body]);
        let old: Option<String> = self
            .conn
            .query_row("SELECT text_hash FROM notes WHERE id=?1", [doc.id], |r| {
                r.get(0)
            })
            .optional()?;
        if old.as_deref() == Some(&hash) {
            self.conn.execute(
                "UPDATE notes SET stat=?2 WHERE id=?1 AND stat IS NOT ?2",
                params![doc.id, doc.stat],
            )?;
            return Ok(false);
        }
        let mut chunks = chunk::chunk_body(doc.body);
        chunks.insert(
            0,
            Chunk {
                ordinal: 0,
                heading: String::new(),
                byte_start: 0,
                byte_end: 0,
                text: head,
            },
        );
        let tx = self.conn.transaction()?;
        // Reuse by embedding-text identity, even when an earlier section moved.
        let saved = {
            let mut stmt = tx.prepare("SELECT c.text_hash,v.model,v.dim,v.vec FROM chunks c JOIN vectors v ON v.chunk_rowid=c.rowid WHERE c.note_id=?1")?;
            let rows = stmt.query_map([doc.id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    (
                        r.get::<_, String>(1)?,
                        r.get::<_, usize>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                    ),
                ))
            })?;
            rows.collect::<Result<HashMap<_, _>, _>>()?
        };
        tx.execute("INSERT INTO notes(id,path,title,text_hash,stat) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET path=excluded.path,title=excluded.title,text_hash=excluded.text_hash,stat=excluded.stat", params![doc.id,doc.path,doc.title,hash,doc.stat])?;
        tx.execute("DELETE FROM chunks WHERE note_id=?1", [doc.id])?;
        for c in chunks {
            let text_hash = hash_parts(&[doc.title, &c.heading, &c.text]);
            tx.execute("INSERT INTO chunks(note_id,ordinal,heading,byte_start,byte_end,text,text_hash) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![doc.id,c.ordinal,c.heading,c.byte_start,c.byte_end,c.text,text_hash])?;
            let rowid = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO chunks_fts(rowid,title,text) VALUES(?1,?2,?3)",
                params![
                    rowid,
                    search::fold_str(&format!("{} {}", doc.title, c.heading)),
                    search::fold_str(&c.text)
                ],
            )?;
            if let Some((model, dim, bytes)) = saved.get(&text_hash) {
                tx.execute(
                    "INSERT INTO vectors(chunk_rowid,model,dim,vec) VALUES(?1,?2,?3,?4)",
                    params![rowid, model, dim, bytes],
                )?;
            }
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn remove_note(&mut self, note_id: &str) -> Result<(), SearchIndexError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM notes WHERE id=?1", [note_id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn retain_notes(&mut self, ids: &HashSet<String>) -> Result<usize, SearchIndexError> {
        let removed = {
            let mut stmt = self.conn.prepare("SELECT id FROM notes")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|id| !ids.contains(id))
                .collect::<Vec<_>>()
        };
        let tx = self.conn.transaction()?;
        for id in &removed {
            tx.execute("DELETE FROM notes WHERE id=?1", [id])?;
        }
        tx.commit()?;
        Ok(removed.len())
    }
    pub fn note_stats(&self) -> Result<HashMap<String, String>, SearchIndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id,stat FROM notes WHERE stat IS NOT NULL")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn stats(&self) -> Result<IndexStats, SearchIndexError> {
        Ok(self.conn.query_row("SELECT (SELECT count(*) FROM notes),(SELECT count(*) FROM chunks),(SELECT count(*) FROM vectors),(SELECT model FROM vectors LIMIT 1)", [], |r| Ok(IndexStats { notes:r.get(0)?,chunks:r.get(1)?,vectors:r.get(2)?,model:r.get(3)? }))?)
    }

    pub fn chunk_text(&self, rowid: i64) -> Result<Option<String>, SearchIndexError> {
        Ok(self
            .conn
            .query_row("SELECT text FROM chunks WHERE rowid=?1", [rowid], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn query(&self, q: &str, limit: usize) -> Result<Vec<ChunkHit>, SearchIndexError> {
        let Some((and, or)) = build_match(q) else {
            return Ok(Vec::new());
        };
        let found = self.run_match(&and, limit)?;
        if found.is_empty() {
            if let Some(or) = or {
                return self.run_match(&or, limit);
            }
        }
        Ok(found)
    }

    fn run_match(&self, expr: &str, limit: usize) -> Result<Vec<ChunkHit>, SearchIndexError> {
        let mut stmt = self.conn.prepare("WITH matches AS MATERIALIZED (SELECT c.note_id,c.ordinal,c.rowid,-bm25(chunks_fts,?2,1.0) AS score FROM chunks_fts JOIN chunks c ON c.rowid=chunks_fts.rowid WHERE chunks_fts MATCH ?1), ranked AS (SELECT *,ROW_NUMBER() OVER (PARTITION BY note_id ORDER BY score DESC,ordinal) AS n FROM matches) SELECT note_id,ordinal,rowid,score FROM ranked WHERE n=1 ORDER BY 4 DESC,note_id LIMIT ?3")?;
        let rows = stmt.query_map(params![expr, TITLE_WEIGHT, limit], hit_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn chunks_without_vectors(
        &self,
        model: &str,
        limit: usize,
    ) -> Result<Vec<PendingChunk>, SearchIndexError> {
        let mut stmt = self.conn.prepare("SELECT c.rowid,n.title,c.heading,c.text,c.text_hash FROM chunks c JOIN notes n ON n.id=c.note_id WHERE NOT EXISTS(SELECT 1 FROM vectors v WHERE v.chunk_rowid=c.rowid AND v.model=?1) ORDER BY c.rowid LIMIT ?2")?;
        let rows = stmt.query_map(params![model, limit], |r| {
            let title: String = r.get(1)?;
            let c = Chunk {
                ordinal: 0,
                heading: r.get(2)?,
                byte_start: 0,
                byte_end: 0,
                text: r.get(3)?,
            };
            Ok(PendingChunk {
                rowid: r.get(0)?,
                text_hash: r.get(4)?,
                embedding_text: chunk::embedding_text(&title, &c),
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn put_vectors(
        &mut self,
        model: &str,
        rows: &[(i64, String, Vec<f32>)],
    ) -> Result<usize, SearchIndexError> {
        let tx = self.conn.transaction()?;
        let mut stored = 0;
        for (rowid, hash, vector) in rows {
            let normalized = match normalize_vector(vector) {
                Ok(vector) => vector,
                Err(error) => {
                    tracing::warn!(%error, rowid, "skipping invalid notes search vector");
                    continue;
                }
            };
            let bytes: Vec<u8> = normalized.iter().flat_map(|x| x.to_le_bytes()).collect();
            stored += tx.execute("INSERT INTO vectors(chunk_rowid,model,dim,vec) SELECT rowid,?2,?3,?4 FROM chunks WHERE rowid=?1 AND text_hash=?5 ON CONFLICT(chunk_rowid) DO UPDATE SET model=excluded.model,dim=excluded.dim,vec=excluded.vec",params![rowid,model,normalized.len(),bytes,hash])?;
        }
        tx.commit()?;
        Ok(stored)
    }

    pub fn clear_vectors(&mut self) -> Result<(), SearchIndexError> {
        self.conn.execute("DELETE FROM vectors", [])?;
        Ok(())
    }

    pub fn cosine_top_k(
        &self,
        model: &str,
        query: &[f32],
        k: usize,
    ) -> Result<Vec<ChunkHit>, SearchIndexError> {
        if k == 0 {
            return Ok(Vec::new());
        }
        let query = normalize_vector(query)?;
        let mut stmt = self.conn.prepare("SELECT c.note_id,c.ordinal,c.rowid,v.vec FROM vectors v JOIN chunks c ON c.rowid=v.chunk_rowid WHERE v.model=?1 AND v.dim=?2")?;
        let mut rows = stmt.query(params![model, query.len()])?;
        let mut heap: BinaryHeap<Candidate> = BinaryHeap::new();
        while let Some(row) = rows.next()? {
            let bytes = row
                .get_ref(3)?
                .as_blob()
                .map_err(|e| SearchIndexError::Vector(e.to_string()))?;
            if bytes.len() != query.len() * 4 {
                continue;
            }
            let score: f32 = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .zip(&query)
                .map(|(b, q)| f32::from_le_bytes(*b) * q)
                .sum();
            if !score.is_finite() {
                continue;
            }
            if heap.len() == k && heap.peek().is_some_and(|worst| score < worst.0.score) {
                continue;
            }
            let candidate = Candidate(ChunkHit {
                note_id: row.get(0)?,
                ordinal: row.get(1)?,
                chunk_rowid: row.get(2)?,
                score,
            });
            if heap.len() < k {
                heap.push(candidate);
            } else if heap.peek().is_some_and(|worst| candidate < *worst) {
                heap.pop();
                heap.push(candidate);
            }
        }
        let mut hits: Vec<_> = heap.into_iter().map(|c| c.0).collect();
        hits.sort_by(hit_order);
        Ok(hits)
    }
}

fn disposable_error(error: &rusqlite::Error) -> bool {
    matches!(error, rusqlite::Error::InvalidQuery)
        || matches!(error, rusqlite::Error::SqliteFailure(e, message)
            if matches!(e.code, rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt)
                || message.as_deref() == Some("no such table: meta"))
}

fn writer(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}
fn hash_parts(parts: &[&str]) -> String {
    let mut hash = blake3::Hasher::new();
    for part in parts {
        hash.update(&(part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    hash.finalize().to_hex().to_string()
}
fn hit_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ChunkHit> {
    Ok(ChunkHit {
        note_id: r.get(0)?,
        ordinal: r.get(1)?,
        chunk_rowid: r.get(2)?,
        score: r.get(3)?,
    })
}
fn normalize_vector(vector: &[f32]) -> Result<Vec<f32>, SearchIndexError> {
    let norm = vector
        .iter()
        .map(|x| f64::from(*x).powi(2))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return Err(SearchIndexError::Vector(
            "expected a finite nonzero vector".into(),
        ));
    }
    Ok(vector
        .iter()
        .map(|x| (f64::from(*x) / norm) as f32)
        .collect())
}
fn hit_order(a: &ChunkHit, b: &ChunkHit) -> Ordering {
    b.score
        .total_cmp(&a.score)
        .then_with(|| a.note_id.cmp(&b.note_id))
        .then_with(|| a.ordinal.cmp(&b.ordinal))
}
struct Candidate(ChunkHit);
impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Candidate {}
impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        hit_order(&self.0, &other.0)
    }
}

pub fn build_match(q: &str) -> Option<(String, Option<String>)> {
    let mut terms: Vec<_> = q
        .split_whitespace()
        .map(search::fold_str)
        .filter(|s| s.chars().any(char::is_alphanumeric))
        .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
        .collect();
    terms.last_mut()?.push('*');
    Some((
        terms.join(" AND "),
        (terms.len() > 1).then(|| terms.join(" OR ")),
    ))
}

fn normalized(hits: &[ChunkHit]) -> Vec<f32> {
    let min = hits.iter().map(|h| h.score).fold(f32::INFINITY, f32::min);
    let max = hits
        .iter()
        .map(|h| h.score)
        .fold(f32::NEG_INFINITY, f32::max);
    hits.iter()
        .map(|h| {
            if max == min {
                1.0
            } else {
                (h.score - min) / (max - min)
            }
        })
        .collect()
}
fn collapse(rows: impl IntoIterator<Item = NoteScore>) -> Vec<NoteScore> {
    let mut best: HashMap<String, NoteScore> = HashMap::new();
    for row in rows {
        match best.entry(row.note_id.clone()) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(row);
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let old = entry.get();
                if tier(row.why) > tier(old.why)
                    || (tier(row.why) == tier(old.why)
                        && (row.score > old.score
                            || (row.score == old.score && row.ordinal < old.ordinal)))
                {
                    entry.insert(row);
                }
            }
        }
    }
    let mut rows: Vec<_> = best.into_values().collect();
    rows.sort_by(|a, b| {
        tier(b.why)
            .cmp(&tier(a.why))
            .then_with(|| b.score.total_cmp(&a.score))
            .then_with(|| a.note_id.cmp(&b.note_id))
    });
    rows
}
fn tier(why: MatchWhy) -> u8 {
    match why {
        MatchWhy::Both => 2,
        MatchWhy::Words => 1,
        MatchWhy::Meaning => 0,
    }
}
pub fn lexical_only(lex: &[ChunkHit]) -> Vec<NoteScore> {
    collapse(lex.iter().zip(normalized(lex)).map(|(h, score)| NoteScore {
        note_id: h.note_id.clone(),
        ordinal: h.ordinal,
        chunk_rowid: h.chunk_rowid,
        score,
        why: MatchWhy::Words,
    }))
}
pub fn fuse(lex: &[ChunkHit], vec: &[ChunkHit]) -> Vec<NoteScore> {
    if vec.is_empty() {
        return lexical_only(lex);
    }
    let mut rows: HashMap<i64, NoteScore> = lex
        .iter()
        .zip(normalized(lex))
        .map(|(h, s)| {
            (
                h.chunk_rowid,
                NoteScore {
                    note_id: h.note_id.clone(),
                    ordinal: h.ordinal,
                    chunk_rowid: h.chunk_rowid,
                    score: LEX_WEIGHT * s,
                    why: MatchWhy::Words,
                },
            )
        })
        .collect();
    for (h, s) in vec.iter().zip(normalized(vec)) {
        if let Some(row) = rows.get_mut(&h.chunk_rowid) {
            row.score += VEC_WEIGHT * s + OVERLAP_BONUS;
            row.why = MatchWhy::Both;
        } else if h.score >= MIN_MEANING_COSINE && VEC_WEIGHT * s >= MIN_HYBRID_SCORE {
            rows.insert(
                h.chunk_rowid,
                NoteScore {
                    note_id: h.note_id.clone(),
                    ordinal: h.ordinal,
                    chunk_rowid: h.chunk_rowid,
                    score: VEC_WEIGHT * s,
                    why: MatchWhy::Meaning,
                },
            );
        }
    }
    collapse(rows.into_values())
}

pub fn marks(text: &str, q: &str) -> Vec<(usize, usize)> {
    let mut ranges: Vec<_> = q
        .split_whitespace()
        .filter(|term| term.chars().any(char::is_alphanumeric))
        .flat_map(|term| search::find_spans(text, term, 64))
        .collect();
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

pub fn utf16_ranges(text: &str, byte_ranges: &[(usize, usize)]) -> Vec<[u32; 2]> {
    let mut events: Vec<_> = byte_ranges
        .iter()
        .enumerate()
        .flat_map(|(i, &(s, e))| [(s.min(text.len()), i, 0), (e.min(text.len()), i, 1)])
        .collect();
    events.sort_unstable();
    let mut result = vec![[0, 0]; byte_ranges.len()];
    let mut chars = text.char_indices().peekable();
    let mut units = 0;
    for (offset, i, side) in events {
        while let Some(&(at, c)) = chars.peek() {
            if at + c.len_utf8() > offset {
                break;
            }
            units += c.len_utf16() as u32;
            chars.next();
        }
        result[i][side] = units
            + if side == 1 {
                chars
                    .peek()
                    .filter(|(at, _)| *at < offset)
                    .map_or(0, |(_, c)| c.len_utf16() as u32)
            } else {
                0
            };
    }
    result
}

pub fn excerpt(
    text: &str,
    byte_ranges: &[(usize, usize)],
    budget: usize,
) -> (String, Vec<(usize, usize)>) {
    if budget == 0 {
        return (String::new(), Vec::new());
    }
    let count = text.chars().count();
    let anchor = byte_ranges.first().map_or(0, |(s, _)| {
        text[..text.floor_char_boundary((*s).min(text.len()))]
            .chars()
            .count()
    });
    let start = anchor
        .saturating_sub(budget / 3)
        .min(count.saturating_sub(budget));
    let end = (start + budget).min(count);
    let lo = text
        .char_indices()
        .nth(start)
        .map_or(text.len(), |(i, _)| i);
    let hi = text.char_indices().nth(end).map_or(text.len(), |(i, _)| i);
    let mut out = String::new();
    if lo > 0 {
        out.push('…');
    }
    let prefix = out.len();
    out.push_str(&text[lo..hi]);
    if hi < text.len() {
        out.push('…');
    }
    let ranges = byte_ranges
        .iter()
        .filter_map(|&(s, e)| {
            let s = s.max(lo);
            let e = e.min(hi);
            (s < e).then(|| (s - lo + prefix, e - lo + prefix))
        })
        .collect();
    (out, ranges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
    };

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "keeper-notes-search-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0),
                NEXT.fetch_add(1, AtomicOrdering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).expect("scratch directory");
            Self(dir)
        }
        fn path(&self) -> PathBuf {
            self.0.join(SEARCH_DB_FILE)
        }
        fn index(&self) -> SearchIndex {
            SearchIndex::open(&self.path(), "vault").expect("index")
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn add(index: &mut SearchIndex, id: &str, body: &str) {
        index
            .replace_note(&NoteDoc {
                id,
                path: id,
                title: "Untitled",
                tags: &[],
                fields: &BTreeMap::new(),
                body,
                stat: None,
            })
            .expect("replace");
    }
    fn hit(id: &str, row: i64, score: f32) -> ChunkHit {
        ChunkHit {
            note_id: id.into(),
            ordinal: 1,
            chunk_rowid: row,
            score,
        }
    }
    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 0.00001, "{a} != {b}");
    }

    #[test]
    fn corrupt_and_missing_meta_recreate() {
        let scratch = Scratch::new();
        std::fs::write(scratch.path(), b"not a sqlite database").expect("corrupt");
        let mut index = scratch.index();
        add(&mut index, "old", "needle");
        index
            .conn
            .execute("DROP TABLE meta", [])
            .expect("missing meta");
        drop(index);
        let mut index = scratch.index();
        assert_eq!(index.stats().expect("empty").notes, 0);
        add(&mut index, "new", "needle");
        assert_eq!(index.query("needle", 10).expect("usable")[0].note_id, "new");
    }

    #[test]
    fn lexical_limit_counts_notes_not_sections() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        add(
            &mut index,
            "many",
            "# A\nneedle\n# B\nneedle\n# C\nneedle\n# D\nneedle\n# E\nneedle",
        );
        add(
            &mut index,
            "other",
            &format!("needle {}", "padding ".repeat(100)),
        );
        let hits = index.query("needle", 2).expect("rank");
        assert_eq!(
            hits.iter()
                .map(|h| h.note_id.as_str())
                .collect::<HashSet<_>>(),
            HashSet::from(["many", "other"])
        );
    }

    #[test]
    fn stale_and_deleted_chunks_do_not_poison_embedding_batch() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        add(&mut index, "a", "old");
        let pending = index.chunks_without_vectors("m", 10).expect("pending");
        add(&mut index, "a", "new");
        add(&mut index, "gone", "deleted");
        let deleted = index
            .chunks_without_vectors("m", 10)
            .expect("pending")
            .pop()
            .expect("deleted chunk");
        index.remove_note("gone").expect("delete");
        let rows = vec![
            (pending[1].rowid, pending[1].text_hash.clone(), vec![1.0]),
            (deleted.rowid, deleted.text_hash, vec![1.0]),
            (pending[0].rowid, pending[0].text_hash.clone(), vec![1.0]),
        ];
        assert_eq!(index.put_vectors("m", &rows).expect("batch"), 1);
        let missing = index.chunks_without_vectors("m", 10).expect("pending");
        assert_eq!(missing.len(), 1);
        assert!(missing[0].embedding_text.contains("new"));
    }

    #[test]
    fn stat_changes_survive_identical_content_and_reopen() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        let fields = BTreeMap::new();
        let mut doc = NoteDoc {
            id: "a",
            path: "a",
            title: "a",
            tags: &[],
            fields: &fields,
            body: "body",
            stat: Some("first"),
        };
        assert!(index.replace_note(&doc).expect("insert"));
        doc.stat = Some("second");
        assert!(!index.replace_note(&doc).expect("stat only"));
        drop(index);
        assert_eq!(
            scratch.index().note_stats().expect("stats"),
            HashMap::from([("a".into(), "second".into())])
        );
    }

    #[test]
    fn polish_fold_and_prefix() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        add(&mut index, "a", "notatkę Łódź");
        for q in ["notatke", "Notatk", "lodz", "łódź"] {
            let hits = index.query(q, 20).expect("query");
            assert_eq!(hits.len(), 1, "{q}");
            assert_eq!(hits[0].ordinal, 1);
            assert!(hits[0].score > 0.0);
        }
        let ro = SearchIndex::open_read_only(&scratch.path()).expect("readonly");
        assert_eq!(ro.query("lodz", 20).expect("query").len(), 1);
    }

    #[test]
    fn reader_keeps_ranked_chunk_snapshot() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        add(&mut index, "a", "old needle");
        let reader = SearchIndex::open_read_only(&scratch.path()).expect("reader");
        let hit = reader.query("needle", 10).expect("rank").remove(0);
        add(&mut index, "a", "replacement");
        assert_eq!(
            reader
                .chunk_text(hit.chunk_rowid)
                .expect("snippet")
                .as_deref(),
            Some("old needle")
        );
        assert_eq!(reader.query("needle", 10).expect("same snapshot").len(), 1);
        let fresh = SearchIndex::open_read_only(&scratch.path()).expect("fresh");
        assert!(fresh.query("needle", 10).expect("new snapshot").is_empty());
    }

    #[test]
    fn and_empty_uses_or() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        add(&mut index, "a", "alpha");
        add(&mut index, "b", "beta");
        let hits = index.query("alpha beta", 20).expect("query");
        assert_eq!(
            hits.iter()
                .map(|h| h.note_id.as_str())
                .collect::<HashSet<_>>(),
            HashSet::from(["a", "b"])
        );
        add(&mut index, "c", "alpha beta");
        assert_eq!(
            index
                .query("alpha beta", 20)
                .expect("query")
                .iter()
                .map(|h| h.note_id.as_str())
                .collect::<Vec<_>>(),
            vec!["c"]
        );
    }

    #[test]
    fn prefix_only_last_term() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        add(&mut index, "a", "alpha beta");
        add(&mut index, "b", "alphabet betamax");
        let hits = index.query("alpha bet", 20).expect("query");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].note_id, "a");
    }

    #[test]
    fn punctuation_and_quotes() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        add(&mut index, "a", "alpha beta");
        assert!(index.query("!? \" *", 20).expect("punctuation").is_empty());
        assert_eq!(
            index.query("alpha !!!", 20).expect("surviving word").len(),
            1
        );
        assert_eq!(
            index.query("alpha\"beta", 20).expect("quoted phrase").len(),
            1
        );
    }

    #[test]
    fn head_metadata() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        index
            .replace_note(&NoteDoc {
                id: "a",
                path: "a.md",
                title: "Plain",
                tags: &["uniquetag".into()],
                fields: &BTreeMap::from([
                    ("project".into(), "uniquevalue".into()),
                    ("keeper.origin".into(), "secretvalue".into()),
                ]),
                body: "ordinary body",
                stat: None,
            })
            .expect("replace");
        for q in ["uniquetag", "uniquevalue"] {
            let hits = index.query(q, 20).expect("query");
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].ordinal, 0);
        }
        assert!(index.query("secretvalue", 20).expect("reserved").is_empty());
    }

    #[test]
    fn replace_is_noop() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        add(&mut index, "a", "body");
        let row = index
            .chunks_without_vectors("m", 10)
            .expect("pending")
            .remove(0);
        index
            .put_vectors("m", &[(row.rowid, row.text_hash, vec![3.0, 4.0])])
            .expect("vector");
        let before = index.stats().expect("stats");
        let changes = index.conn.total_changes();
        let hash: String = index
            .conn
            .query_row("SELECT text_hash FROM notes", [], |r| r.get(0))
            .expect("hash");
        assert!(!index
            .replace_note(&NoteDoc {
                id: "a",
                path: "a",
                title: "Untitled",
                tags: &[],
                fields: &BTreeMap::new(),
                body: "body",
                stat: None,
            })
            .expect("no-op"));
        assert_eq!(index.stats().expect("stats"), before);
        assert_eq!(index.conn.total_changes(), changes);
        assert_eq!(
            index
                .conn
                .query_row("SELECT text_hash FROM notes", [], |r| r.get::<_, String>(0))
                .expect("hash"),
            hash
        );
    }

    #[test]
    fn identity_mismatch_recreates() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        add(&mut index, "a", "body");
        index
            .conn
            .execute("CREATE TABLE sentinel(value TEXT)", [])
            .expect("sentinel");
        index
            .conn
            .execute("UPDATE meta SET schema=99", [])
            .expect("old schema");
        drop(index);
        let mut index = scratch.index();
        assert_eq!(index.stats().expect("stats").notes, 0);
        assert!(index.conn.prepare("SELECT * FROM sentinel").is_err());
        add(&mut index, "b", "body");
        drop(index);
        let index = SearchIndex::open(&scratch.path(), "other").expect("new vault");
        assert_eq!(index.stats().expect("stats").notes, 0);
        assert!(SearchIndex::open_read_only(&scratch.0.join("absent.db")).is_err());
    }

    #[test]
    fn remove_and_retain() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        for id in ["a", "b", "c"] {
            add(&mut index, id, "needle");
        }
        let pending = index.chunks_without_vectors("m", 20).expect("pending");
        index
            .put_vectors(
                "m",
                &pending
                    .iter()
                    .map(|c| (c.rowid, c.text_hash.clone(), vec![1.0]))
                    .collect::<Vec<_>>(),
            )
            .expect("vectors");
        index.remove_note("a").expect("remove");
        assert_eq!(
            index
                .retain_notes(&HashSet::from(["c".into()]))
                .expect("retain"),
            1
        );
        assert_eq!(index.stats().expect("stats").vectors, 2);
        assert_eq!(
            index
                .query("needle", 20)
                .expect("query")
                .iter()
                .map(|h| h.note_id.as_str())
                .collect::<Vec<_>>(),
            vec!["c"]
        );
        assert_eq!(index.retain_notes(&HashSet::new()).expect("empty"), 1);
        assert_eq!(index.stats().expect("stats").chunks, 0);
        assert_eq!(index.stats().expect("stats").vectors, 0);
        assert!(index.query("needle", 20).expect("query").is_empty());
    }

    #[test]
    fn vectors_normalize() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        add(&mut index, "a", "body");
        let row = index
            .chunks_without_vectors("m", 10)
            .expect("pending")
            .remove(0);
        index
            .put_vectors("m", &[(row.rowid, row.text_hash, vec![3.0, 4.0])])
            .expect("vector");
        close(
            index.cosine_top_k("m", &[6.0, 8.0], 1).expect("cosine")[0].score,
            1.0,
        );
        close(
            index.cosine_top_k("m", &[5.0, 0.0], 1).expect("cosine")[0].score,
            0.6,
        );
    }

    #[test]
    fn vectors_clear() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        add(&mut index, "a", "body");
        let pending = index.chunks_without_vectors("m", 10).expect("pending");
        index
            .put_vectors(
                "m",
                &pending
                    .iter()
                    .map(|c| (c.rowid, c.text_hash.clone(), vec![1.0]))
                    .collect::<Vec<_>>(),
            )
            .expect("vectors");
        assert!(index
            .chunks_without_vectors("m", 10)
            .expect("pending")
            .is_empty());
        index.clear_vectors().expect("clear");
        assert_eq!(index.stats().expect("stats").model, None);
        assert_eq!(
            index.chunks_without_vectors("new", 10).expect("pending"),
            pending
        );
    }

    #[test]
    fn vectors_top_k() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        for id in ["a", "b", "c"] {
            add(&mut index, id, "body");
        }
        let pending = index.chunks_without_vectors("m", 10).expect("pending");
        index
            .put_vectors(
                "m",
                &pending
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (c.rowid, c.text_hash.clone(), vec![i as f32, 1.0]))
                    .collect::<Vec<_>>(),
            )
            .expect("vectors");
        let hits = index.cosine_top_k("m", &[1.0, 0.0], 2).expect("top");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].chunk_rowid, pending[5].rowid);
        assert_eq!(hits[1].chunk_rowid, pending[4].rowid);
        assert!(index
            .cosine_top_k("other", &[1.0, 0.0], 2)
            .expect("model")
            .is_empty());
        assert!(index
            .cosine_top_k("m", &[1.0], 2)
            .expect("dimension")
            .is_empty());
        assert!(index.cosine_top_k("m", &[], 0).expect("zero k").is_empty());
    }

    #[test]
    fn vectors_reject_invalid() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        add(&mut index, "a", "body");
        let pending = index.chunks_without_vectors("m", 10).expect("pending");
        for bad in [vec![], vec![0.0], vec![f32::NAN], vec![f32::INFINITY]] {
            assert_eq!(
                index
                    .put_vectors(
                        "m",
                        &[
                            (pending[0].rowid, pending[0].text_hash.clone(), vec![1.0]),
                            (pending[1].rowid, pending[1].text_hash.clone(), bad)
                        ]
                    )
                    .expect("skip bad row"),
                1
            );
            assert_eq!(index.stats().expect("stats").vectors, 1);
        }
    }

    #[test]
    fn vectors_survive_unchanged_chunks() {
        let scratch = Scratch::new();
        let mut index = scratch.index();
        let unchanged = "persistent ".repeat(100);
        add(
            &mut index,
            "a",
            &format!("# First\n{}\n\n# Second\n{unchanged}", "old ".repeat(100)),
        );
        let pending = index.chunks_without_vectors("m", 10).expect("pending");
        index
            .put_vectors(
                "m",
                &pending
                    .iter()
                    .map(|c| (c.rowid, c.text_hash.clone(), vec![1.0]))
                    .collect::<Vec<_>>(),
            )
            .expect("vectors");
        add(
            &mut index,
            "a",
            &format!("# First\n{}\n\n# Second\n{unchanged}", "new ".repeat(110)),
        );
        let missing = index.chunks_without_vectors("m", 10).expect("pending");
        assert_eq!(missing.len(), 1);
        assert!(missing[0].embedding_text.contains("new "));
        assert_eq!(index.stats().expect("stats").vectors, 2);
    }

    #[test]
    fn fusion_anchor() {
        let rows = fuse(
            &[hit("strong", 1, 10.0), hit("weak", 2, 1.0)],
            &[hit("semantic", 3, 1.0)],
        );
        let weak = rows
            .iter()
            .find(|r| r.note_id == "weak")
            .expect("lexical anchor");
        assert_eq!(weak.why, MatchWhy::Words);
        close(weak.score, 0.0);
    }

    #[test]
    fn fusion_floor() {
        let rows = fuse(
            &[hit("lexical", 1, 1.0)],
            &[hit("high", 2, 1.0), hit("low", 3, 0.1), hit("mid", 4, 0.2)],
        );
        assert!(rows
            .iter()
            .any(|r| r.note_id == "high" && r.why == MatchWhy::Meaning));
        assert!(!rows
            .iter()
            .any(|r| r.note_id == "low" || r.note_id == "mid"));
    }

    #[test]
    fn fusion_orders_tiers_before_scores() {
        let rows = fuse(
            &[
                hit("lex", 1, 9.0),
                hit("anchor", 2, 1.0),
                hit("both", 5, 1.0),
            ],
            &[
                hit("vec", 3, 0.9),
                hit("floor", 4, 0.5),
                hit("both", 5, 0.5),
            ],
        );
        assert_eq!(
            rows.iter().map(|r| r.note_id.as_str()).collect::<Vec<_>>(),
            ["both", "lex", "anchor", "vec"]
        );
        assert!(rows[3].score > rows[1].score);
    }

    #[test]
    fn meaning_requires_raw_cosine_even_when_best_in_pool() {
        assert!(fuse(&[], &[hit("weak", 1, 0.49)]).is_empty());
        assert_eq!(
            fuse(&[], &[hit("boundary", 1, MIN_MEANING_COSINE)])[0].why,
            MatchWhy::Meaning
        );
        assert_eq!(
            fuse(&[], &[hit("high", 1, 0.9), hit("low", 2, 0.7)]).len(),
            1
        );
    }

    #[test]
    fn fusion_overlap() {
        let rows = fuse(
            &[hit("top", 1, 10.0), hit("bottom", 2, 1.0)],
            &[hit("top", 1, 10.0), hit("bottom", 2, 1.0)],
        );
        close(rows[0].score, 1.15);
        close(rows[1].score, 0.15);
        assert!(rows.iter().all(|r| r.why == MatchWhy::Both));
    }

    #[test]
    fn fusion_best_chunk() {
        let mut second = hit("a", 2, 4.0);
        second.ordinal = 2;
        let rows = fuse(&[hit("a", 1, 1.0), second, hit("b", 3, 4.0)], &[]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].note_id, "a");
        assert_eq!(rows[0].ordinal, 2);
        assert_eq!(rows[1].note_id, "b");
    }

    #[test]
    fn fusion_lexical_only() {
        let lex = [
            hit("first", 1, 5.0),
            hit("middle", 2, 3.0),
            hit("last", 3, 1.0),
        ];
        let rows = fuse(&lex, &[]);
        assert_eq!(
            rows.iter().map(|r| r.score).collect::<Vec<_>>(),
            vec![1.0, 0.5, 0.0]
        );
        assert!(rows.iter().all(|r| r.why == MatchWhy::Words));
    }

    #[test]
    fn marks_merge() {
        let text = "Łódź notatkę";
        let ranges = marks(text, "lodz not notatke");
        assert_eq!(
            ranges.iter().map(|&(s, e)| &text[s..e]).collect::<Vec<_>>(),
            vec!["Łódź", "notatkę"]
        );
    }

    #[test]
    fn utf16_offsets() {
        let text = "ł🙂tax";
        assert_eq!(
            utf16_ranges(text, &[(6, 9), (1, 5), (0, 2)]),
            vec![[3, 6], [0, 3], [0, 1]]
        );
    }

    #[test]
    fn excerpt_rebases() {
        let text = format!("{}notatkę{}", "ł🙂 ".repeat(100), " end".repeat(100));
        let ranges = marks(&text, "notatke");
        let (snippet, rebased) = excerpt(&text, &ranges, 40);
        assert!(snippet.starts_with('…') && snippet.ends_with('…'));
        assert_eq!(snippet.chars().count(), 42);
        assert_eq!(&snippet[rebased[0].0..rebased[0].1], "notatkę");
        assert_eq!(marks(&snippet, "notatke"), rebased);
        assert_eq!(excerpt(&text, &ranges, 0), (String::new(), vec![]));
    }
}
