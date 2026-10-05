//! `<zone>/.keeper/agents.db`: the derived, disposable index of a sessions
//! zone's agent sessions (AD-365, D-21; story 89.5).
//!
//! The board, the session list and a turn answer from here and from the
//! writer's in-memory tail, never by re-reading a log (NFR-116). It is
//! rebuilt from the logs, the session `agent.toml` files and the cards, kept
//! current by [`Index::apply`] after each of this host's appends, and by
//! [`Index::refresh_session`] for what other hosts wrote, which reads only
//! the chunks that grew (R62). A schema version this build does not know is
//! dropped and rebuilt, never migrated and never an error: nothing in it is
//! anywhere but in the files.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::agents::card;
use crate::agents::label::Label;
use crate::agents::log::reader::{read_session, SessionLog};
use crate::agents::log::writer::AppendReceipt;
use crate::agents::log::{ChunkName, ClaimAction, LineBody, LogLine, LOG_DIR};
use crate::agents::session::{self, parse_session_agent_toml, SessionAgent};
use crate::notes::frontmatter::Frontmatter;
use crate::sessions::model::{ARTIFACTS_DIR, WORKSPACE_DIR};
use crate::sessions::pool::{read_one, PoolFile};
use crate::sessions::shape::KindTag;

/// The index's folder and file inside a zone.
pub const INDEX_PATH: [&str; 2] = [".keeper", "agents.db"];

/// The schema this build writes. A different `user_version` rebuilds.
pub const SCHEMA_VERSION: i64 = 3;

/// The most directory entries one session's card walk visits.
const CARD_WALK_BUDGET: usize = 2_000;

/// The most bytes a rebuild reads of one session's cards; a card past it is
/// reported, not read.
const CARD_READ_BUDGET: u64 = 10 * 1024 * 1024;

/// The most bytes of grown log one [`Index::refresh_session`] reads; the
/// rest is read by the next one.
pub const REFRESH_BYTES: u64 = 1024 * 1024;

const SCHEMA: &str = "
CREATE TABLE sessions (
    path TEXT PRIMARY KEY,
    id TEXT NOT NULL,
    agent TEXT NOT NULL,
    drive TEXT NOT NULL,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    room TEXT NOT NULL,
    label TEXT NOT NULL,
    scope TEXT NOT NULL,
    run TEXT,
    run_detail TEXT,
    claim_host TEXT,
    claim_epoch INTEGER,
    lines INTEGER NOT NULL,
    last_ts TEXT
);
CREATE TABLE chunks (
    session TEXT NOT NULL,
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    n INTEGER NOT NULL,
    bytes INTEGER NOT NULL,
    last_offset INTEGER NOT NULL,
    PRIMARY KEY (session, name)
);
CREATE TABLE cards (
    session TEXT NOT NULL,
    rel TEXT NOT NULL,
    run TEXT,
    assignee TEXT,
    host TEXT,
    requested_by TEXT,
    schedule TEXT,
    last_run TEXT,
    workflow TEXT,
    scheduled_by TEXT,
    integrity TEXT,
    PRIMARY KEY (session, rel)
);
CREATE TABLE fences (
    session TEXT PRIMARY KEY,
    last_ns INTEGER,
    last_host TEXT,
    last_id TEXT,
    acquired TEXT NOT NULL
);
CREATE TABLE seen_events (
    session TEXT NOT NULL,
    event_id TEXT NOT NULL,
    PRIMARY KEY (session, event_id)
);
";

/// Why the index could not answer.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("The agents index could not be used: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("The agents index holds a value it cannot read: {0}")]
    Json(#[from] serde_json::Error),
    #[error("The agents index has no session at {0}; it was created without being added.")]
    UnknownSession(String),
}

fn io(path: &Path, source: std::io::Error) -> IndexError {
    IndexError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// One session, as the index projects it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    /// Zone-relative: `active/2026-10-02-release`.
    pub path: String,
    pub id: String,
    pub agent: String,
    pub drive: String,
    pub kind: String,
    pub title: String,
    pub room: String,
    /// The label after every `label` line.
    pub label: Label,
    /// The drives in scope after every `scope` line.
    pub scope: Vec<String>,
    /// The last `run` state.
    pub run: Option<String>,
    /// That `run` line's detail: why, for `waiting` and `blocked`.
    pub run_detail: Option<String>,
    /// The host that holds the claim, while one does.
    pub claim_host: Option<String>,
    /// The last claim epoch.
    pub claim_epoch: Option<u64>,
    /// How many lines the log holds.
    pub lines: u64,
    /// The newest line's `ts`.
    pub last_ts: Option<String>,
}

impl SessionRow {
    /// Why no host can run the session's work, while its latest `run` line
    /// says `waiting` (Q7): that line's detail.
    pub fn waiting(&self) -> Option<&str> {
        match self.run.as_deref() {
            Some(word) if word == crate::agents::log::RunState::Waiting.as_str() => {
                self.run_detail.as_deref()
            }
            _ => None,
        }
    }
}

/// One chunk, as the index projects it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkRow {
    pub name: String,
    pub host: String,
    pub n: u32,
    pub bytes: u64,
    pub last_offset: u64,
}

/// One card's agent keys (the nine of [`card::KEYS`]), as written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CardRow {
    /// Session-relative.
    pub rel: String,
    pub run: Option<String>,
    pub assignee: Option<String>,
    pub host: Option<String>,
    pub requested_by: Option<String>,
    pub schedule: Option<String>,
    pub last_run: Option<String>,
    pub workflow: Option<String>,
    pub scheduled_by: Option<String>,
    pub integrity: Option<String>,
}

/// What a rebuild found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RebuildReport {
    /// How many sessions it indexed.
    pub sessions: usize,
    /// Sessions or files it could not read, as sentences.
    pub problems: Vec<String>,
}

/// The index of one sessions zone.
pub struct Index {
    conn: Connection,
    zone_root: PathBuf,
    rebuilt_schema: bool,
    refresh_bytes: u64,
}

impl Index {
    /// Open `<zone_root>/.keeper/agents.db`, creating it; a schema of
    /// another version is dropped and recreated empty (call
    /// [`Index::rebuild`] when [`Index::needs_rebuild`] says so).
    pub fn open(zone_root: &Path) -> Result<Index, IndexError> {
        let dir = zone_root.join(INDEX_PATH[0]);
        fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        let conn = Connection::open(dir.join(INDEX_PATH[1]))?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let rebuilt_schema = version != SCHEMA_VERSION;
        if rebuilt_schema {
            conn.execute_batch(
                "DROP TABLE IF EXISTS sessions; DROP TABLE IF EXISTS chunks;
                 DROP TABLE IF EXISTS cards; DROP TABLE IF EXISTS seen_events;
                 DROP TABLE IF EXISTS fences;",
            )?;
            conn.execute_batch(SCHEMA)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(Index {
            conn,
            zone_root: zone_root.to_owned(),
            rebuilt_schema,
            refresh_bytes: REFRESH_BYTES,
        })
    }

    /// Whether the open found no index of this schema, so it is empty.
    pub fn needs_rebuild(&self) -> bool {
        self.rebuilt_schema
    }

    /// Forget everything and index every agent session in the zone —
    /// `active/<session>/` and `archive/<year>/<session>/` holding an
    /// `agent.toml` — from its files.
    pub fn rebuild(&mut self) -> Result<RebuildReport, IndexError> {
        let mut report = RebuildReport::default();
        let mut found = Vec::new();
        for (rel, dir) in session_dirs(&self.zone_root, &mut report) {
            let toml_path = dir.join(session::FILE_NAME);
            let text = match fs::read_to_string(&toml_path) {
                Ok(text) => text,
                Err(error) => {
                    report
                        .problems
                        .push(format!("{rel}: agent.toml could not be read: {error}."));
                    continue;
                }
            };
            match parse_session_agent_toml(&text) {
                Ok(agent) => found.push((rel, dir, agent)),
                Err(refusal) => report
                    .problems
                    .push(format!("{rel}: {}", refusal.sentence())),
            }
        }

        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "DELETE FROM sessions; DELETE FROM chunks; DELETE FROM cards; DELETE FROM seen_events;
             DELETE FROM fences;",
        )?;
        for (rel, dir, agent) in &found {
            let log = read_session(dir);
            for problem in &log.problems {
                report
                    .problems
                    .push(format!("{rel}: {}: {}", problem.chunk, problem.sentence));
            }
            insert_session(&tx, rel, agent, &log)?;
            insert_cards(&tx, rel, dir, &mut report)?;
            report.sessions += 1;
        }
        tx.commit()?;
        self.rebuilt_schema = false;
        Ok(report)
    }

    /// Add a session the runtime just created, before its first line.
    pub fn add_session(&mut self, path: &str, agent: &SessionAgent) -> Result<(), IndexError> {
        insert_session(&self.conn, path, agent, &SessionLog::default())
    }

    /// The most bytes of grown log one refresh reads, [`REFRESH_BYTES`]
    /// unless set.
    pub fn set_refresh_bytes(&mut self, bytes: u64) {
        self.refresh_bytes = bytes.max(1);
    }

    /// Bring one session's rows up to its files (R62, R121). The index is a
    /// projection of the log, never its authority, so the row this leaves
    /// is the row a whole read of the log would give:
    ///
    /// - the log chunks that grew are read from where the index stopped, at
    ///   most [`REFRESH_BYTES`] per call (whole lines; the rest on the next
    ///   call), and their lines are projected in (`ts`, `host`, `id`) order
    ///   through the claim epoch fence a whole read runs, against the claims
    ///   already projected;
    /// - when a line read now sorts before one already projected (a chunk
    ///   that arrived late), or a chunk shrank, went away or ends where the
    ///   index did not, the session is read whole again — as one the index
    ///   does not hold yet is;
    /// - all of it in one write transaction taken before anything is read,
    ///   so it never interleaves with [`Index::apply`] on another
    ///   connection.
    ///
    /// `pool` is the session's markdown as the caller already read it (the
    /// board's bounded pool): the cards are projected from it and no card is
    /// read again; `None` leaves them as they are. A folder that is not an
    /// agent session leaves no rows. Returns the session's row.
    pub fn refresh_session(
        &mut self,
        path: &str,
        pool: Option<&[PoolFile<'_>]>,
    ) -> Result<Option<SessionRow>, IndexError> {
        self.refresh_session_with(path, pool, &mut || {})
    }

    /// [`Self::refresh_session`], running `between` once the held row and
    /// chunk cursors are read, before the log is.
    fn refresh_session_with(
        &mut self,
        path: &str,
        pool: Option<&[PoolFile<'_>]>,
        between: &mut dyn FnMut(),
    ) -> Result<Option<SessionRow>, IndexError> {
        let dir = self.zone_root.join(path);
        let budget = self.refresh_bytes;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(agent) = session_agent(&dir) else {
            forget(&tx, path)?;
            tx.commit()?;
            return Ok(None);
        };
        let held = session_in(&tx, path)?;
        let cursors = chunks_in(&tx, path)?;
        between();
        let tails = held
            .as_ref()
            .and_then(|_| grown_chunks(&dir, &cursors, budget));
        match (held, tails) {
            (Some(row), Some(tails)) => {
                let lines: Vec<LogLine> =
                    tails.iter().flat_map(|tail| tail.lines.clone()).collect();
                if append_lines(&tx, path, row, lines)? {
                    for tail in &tails {
                        set_cursor(&tx, path, &tail.name, tail.bytes, tail.last_offset)?;
                    }
                } else {
                    replay_whole(&tx, path, &agent, &dir)?;
                }
            }
            _ => replay_whole(&tx, path, &agent, &dir)?,
        }
        if let Some(pool) = pool {
            tx.execute("DELETE FROM cards WHERE session = ?1", params![path])?;
            for file in pool.iter().filter(|file| card_place(file.rel)) {
                insert_card(&tx, path, file.rel, file.text)?;
            }
        }
        let row = session_in(&tx, path)?;
        tx.commit()?;
        Ok(row)
    }

    /// Project one line this host appended into its session's row and
    /// chunk — once: a line a refresh already read (its chunk's cursor is
    /// past it) is not projected again, and one that sorts before a line
    /// already projected replays the session whole, as a refresh would.
    pub fn apply(&mut self, session: &str, receipt: &AppendReceipt) -> Result<(), IndexError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(row) = session_in(&tx, session)? else {
            return Err(IndexError::UnknownSession(session.to_owned()));
        };
        let chunk = receipt.chunk.to_string();
        let end = receipt.offset + receipt.bytes;
        let cursor = chunks_in(&tx, session)?
            .into_iter()
            .find(|cursor| cursor.name == chunk)
            .map_or(0, |cursor| cursor.bytes);
        if cursor < end {
            // Only a line that starts where the index stopped can follow it.
            if cursor == receipt.offset
                && append_lines(&tx, session, row, vec![receipt.line.clone()])?
            {
                set_cursor(&tx, session, &receipt.chunk, end, receipt.offset)?;
            } else {
                let dir = self.zone_root.join(session);
                match session_agent(&dir) {
                    Some(agent) => replay_whole(&tx, session, &agent, &dir)?,
                    None => return Err(IndexError::UnknownSession(session.to_owned())),
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// One session's row.
    pub fn session(&self, path: &str) -> Result<Option<SessionRow>, IndexError> {
        session_in(&self.conn, path)
    }

    /// Every session, by path.
    pub fn sessions(&self) -> Result<Vec<SessionRow>, IndexError> {
        let mut statement = self
            .conn
            .prepare(&format!("{SESSION_SELECT} ORDER BY path"))?;
        let raws = statement
            .query_map([], raw_session)?
            .collect::<Result<Vec<_>, _>>()?;
        raws.into_iter().map(RawSession::into_row).collect()
    }

    /// One session's chunks, in name order.
    pub fn chunks(&self, session: &str) -> Result<Vec<ChunkRow>, IndexError> {
        chunks_in(&self.conn, session)
    }

    /// One session's cards, by path.
    pub fn cards(&self, session: &str) -> Result<Vec<CardRow>, IndexError> {
        let mut statement = self.conn.prepare(
            "SELECT rel, run, assignee, host, requested_by, schedule, last_run, workflow,
                scheduled_by, integrity
             FROM cards WHERE session = ?1 ORDER BY rel",
        )?;
        let rows = statement
            .query_map(params![session], |row| {
                Ok(CardRow {
                    rel: row.get(0)?,
                    run: row.get(1)?,
                    assignee: row.get(2)?,
                    host: row.get(3)?,
                    requested_by: row.get(4)?,
                    schedule: row.get(5)?,
                    last_run: row.get(6)?,
                    workflow: row.get(7)?,
                    scheduled_by: row.get(8)?,
                    integrity: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Whether `event_id` is already logged in `session`, so an incoming
    /// event is never processed twice.
    pub fn seen(&self, session: &str, event_id: &str) -> Result<bool, IndexError> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM seen_events WHERE session = ?1 AND event_id = ?2",
                params![session, event_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}

const SESSION_SELECT: &str = "SELECT path, id, agent, drive, kind, title, room, label, scope, run,
    claim_host, claim_epoch, lines, last_ts, run_detail FROM sessions";

struct RawSession {
    row: SessionRow,
    label: String,
    scope: String,
}

impl RawSession {
    fn into_row(self) -> Result<SessionRow, IndexError> {
        let mut row = self.row;
        row.label = serde_json::from_str(&self.label)?;
        row.scope = serde_json::from_str(&self.scope)?;
        Ok(row)
    }
}

fn raw_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawSession> {
    Ok(RawSession {
        row: SessionRow {
            path: row.get(0)?,
            id: row.get(1)?,
            agent: row.get(2)?,
            drive: row.get(3)?,
            kind: row.get(4)?,
            title: row.get(5)?,
            room: row.get(6)?,
            label: Label::top(),
            scope: Vec::new(),
            run: row.get(9)?,
            run_detail: row.get(14)?,
            claim_host: row.get(10)?,
            claim_epoch: row.get::<_, Option<i64>>(11)?.map(|e| e as u64),
            lines: row.get::<_, i64>(12)? as u64,
            last_ts: row.get(13)?,
        },
        label: row.get(7)?,
        scope: row.get(8)?,
    })
}

fn session_in(conn: &Connection, path: &str) -> Result<Option<SessionRow>, IndexError> {
    let raw = conn
        .query_row(
            &format!("{SESSION_SELECT} WHERE path = ?1"),
            params![path],
            raw_session,
        )
        .optional()?;
    raw.map(RawSession::into_row).transpose()
}

fn chunks_in(conn: &Connection, session: &str) -> Result<Vec<ChunkRow>, IndexError> {
    let mut statement = conn.prepare(
        "SELECT name, host, n, bytes, last_offset FROM chunks WHERE session = ?1 ORDER BY name",
    )?;
    let rows = statement
        .query_map(params![session], |row| {
            Ok(ChunkRow {
                name: row.get(0)?,
                host: row.get(1)?,
                n: row.get(2)?,
                bytes: row.get::<_, i64>(3)? as u64,
                last_offset: row.get::<_, i64>(4)? as u64,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn set_cursor(
    conn: &Connection,
    session: &str,
    chunk: &ChunkName,
    bytes: u64,
    last_offset: u64,
) -> Result<(), IndexError> {
    conn.execute(
        "INSERT OR REPLACE INTO chunks (session, name, host, n, bytes, last_offset)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            session,
            chunk.to_string(),
            chunk.host.as_str(),
            chunk.n,
            bytes as i64,
            last_offset as i64
        ],
    )?;
    Ok(())
}

/// A session folder's `agent.toml`, when it is a regular file that reads.
fn session_agent(dir: &Path) -> Option<SessionAgent> {
    let toml = dir.join(session::FILE_NAME);
    fs::symlink_metadata(&toml)
        .is_ok_and(|meta| meta.file_type().is_file())
        .then(|| fs::read_to_string(&toml).ok())
        .flatten()
        .and_then(|text| parse_session_agent_toml(&text).ok())
}

/// Read the session's whole log again and project it, as a first read does.
fn replay_whole(
    conn: &Connection,
    path: &str,
    agent: &SessionAgent,
    dir: &Path,
) -> Result<(), IndexError> {
    for table in [
        "sessions WHERE path",
        "chunks WHERE session",
        "seen_events WHERE session",
        "fences WHERE session",
    ] {
        conn.execute(&format!("DELETE FROM {table} = ?1"), params![path])?;
    }
    insert_session(conn, path, agent, &read_session(dir))
}

/// A line's place in the merged order of a session's log: (`ts`, `host`,
/// `id`), as the reader sorts.
type LineKey = (i64, String, String);

fn key_of(line: &LogLine) -> LineKey {
    (
        line.ts.timestamp_nanos_opt().unwrap_or(i64::MAX),
        line.host.as_str().to_owned(),
        line.id.to_string(),
    )
}

/// What the projection needs to carry on where it stopped as a whole read
/// would: the last line projected, and when each claim epoch was first
/// acquired (the fence).
#[derive(Debug, Default)]
struct Fence {
    last: Option<LineKey>,
    acquired: BTreeMap<u64, i64>,
}

impl Fence {
    fn load(conn: &Connection, session: &str) -> Result<Fence, IndexError> {
        let row = conn
            .query_row(
                "SELECT last_ns, last_host, last_id, acquired FROM fences WHERE session = ?1",
                params![session],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((ns, host, id, acquired)) = row else {
            return Ok(Fence::default());
        };
        Ok(Fence {
            last: ns.zip(host).zip(id).map(|((ns, host), id)| (ns, host, id)),
            acquired: serde_json::from_str(&acquired)?,
        })
    }

    fn store(&self, conn: &Connection, session: &str) -> Result<(), IndexError> {
        let (ns, host, id) = self
            .last
            .clone()
            .map_or((None, None, None), |(ns, host, id)| {
                (Some(ns), Some(host), Some(id))
            });
        conn.execute(
            "INSERT OR REPLACE INTO fences (session, last_ns, last_host, last_id, acquired)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                session,
                ns,
                host,
                id,
                serde_json::to_string(&self.acquired)?
            ],
        )?;
        Ok(())
    }

    /// Take `line` in order: whether a whole read keeps it. A claim
    /// transition is never fenced; any other line of an epoch below one
    /// acquired before it is a stale host's late line (AD-378).
    fn admit(&mut self, line: &LogLine) -> bool {
        let ns = line.ts.timestamp_nanos_opt().unwrap_or(i64::MAX);
        self.last = Some(key_of(line));
        if let LineBody::Claim(claim) = &line.body {
            if claim.action == ClaimAction::Acquired {
                self.acquired.entry(claim.epoch).or_insert(ns);
            }
            return true;
        }
        let superseded_at = self
            .acquired
            .range(line.epoch.saturating_add(1)..)
            .map(|(_, at)| *at)
            .min();
        superseded_at.is_none_or(|at| ns <= at)
    }
}

/// Project `lines` onto the session's `row` as a whole read of the log
/// would, when every one of them sorts after the last line projected;
/// `false`, projecting nothing, when one does not.
fn append_lines(
    conn: &Connection,
    path: &str,
    mut row: SessionRow,
    mut lines: Vec<LogLine>,
) -> Result<bool, IndexError> {
    let mut fence = Fence::load(conn, path)?;
    lines.sort_by_key(key_of);
    if let (Some(first), Some(last)) = (lines.first(), &fence.last) {
        if key_of(first) <= *last {
            return Ok(false);
        }
    }
    for line in &lines {
        if !fence.admit(line) {
            continue;
        }
        project(&mut row, line);
        if let Some(event) = &line.matrix_event {
            conn.execute(
                "INSERT OR IGNORE INTO seen_events (session, event_id) VALUES (?1, ?2)",
                params![path, event.as_str()],
            )?;
        }
    }
    write_row(conn, &row)?;
    fence.store(conn, path)?;
    Ok(true)
}

/// Fold one line into a session's row.
fn project(row: &mut SessionRow, line: &LogLine) {
    row.lines += 1;
    let ts = line.ts_text();
    if row.last_ts.as_ref().is_none_or(|last| *last < ts) {
        row.last_ts = Some(ts);
    }
    match &line.body {
        LineBody::Label(body) => row.label = body.label(),
        LineBody::Scope(body) => row.scope = body.drives.clone(),
        LineBody::Run(body) => {
            row.run = Some(body.state.as_str().to_owned());
            row.run_detail = body.detail.clone();
        }
        LineBody::Claim(body) => {
            row.claim_epoch = Some(body.epoch);
            row.claim_host = match body.action {
                ClaimAction::Acquired | ClaimAction::Renewed => Some(line.host.to_string()),
                ClaimAction::Released | ClaimAction::Lost => None,
            };
        }
        _ => {}
    }
}

fn write_row(conn: &Connection, row: &SessionRow) -> Result<(), IndexError> {
    conn.execute(
        "INSERT INTO sessions (path, id, agent, drive, kind, title, room, label, scope, run,
            claim_host, claim_epoch, lines, last_ts, run_detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT (path) DO UPDATE SET id = ?2, agent = ?3, drive = ?4, kind = ?5,
            title = ?6, room = ?7, label = ?8, scope = ?9, run = ?10, claim_host = ?11,
            claim_epoch = ?12, lines = ?13, last_ts = ?14, run_detail = ?15",
        params![
            row.path,
            row.id,
            row.agent,
            row.drive,
            row.kind,
            row.title,
            row.room,
            serde_json::to_string(&row.label)?,
            serde_json::to_string(&row.scope)?,
            row.run,
            row.claim_host,
            row.claim_epoch.map(|e| e as i64),
            row.lines as i64,
            row.last_ts,
            row.run_detail,
        ],
    )?;
    Ok(())
}

fn insert_session(
    conn: &Connection,
    path: &str,
    agent: &SessionAgent,
    log: &SessionLog,
) -> Result<(), IndexError> {
    let mut row = SessionRow {
        path: path.to_owned(),
        id: agent.id.to_string(),
        agent: agent.agent.clone(),
        drive: agent.drive.clone(),
        kind: agent.kind.as_str().to_owned(),
        title: agent.title.clone(),
        room: agent.room.to_string(),
        label: agent.label.clone(),
        scope: agent.drives.clone(),
        run: None,
        run_detail: None,
        claim_host: None,
        claim_epoch: None,
        lines: 0,
        last_ts: None,
    };
    let mut fence = Fence::default();
    for line in &log.lines {
        // The reader fenced these already; this records where it stopped.
        fence.admit(line);
        project(&mut row, line);
        if let Some(event) = &line.matrix_event {
            conn.execute(
                "INSERT OR IGNORE INTO seen_events (session, event_id) VALUES (?1, ?2)",
                params![path, event.as_str()],
            )?;
        }
    }
    write_row(conn, &row)?;
    fence.store(conn, path)?;
    for chunk in &log.chunks {
        conn.execute(
            "INSERT OR REPLACE INTO chunks (session, name, host, n, bytes, last_offset)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                path,
                chunk.name.to_string(),
                chunk.name.host.as_str(),
                chunk.name.n,
                chunk.bytes as i64,
                chunk.last_offset as i64
            ],
        )?;
    }
    Ok(())
}

/// The cards of one session: its markdown files tagged `task`, read as the
/// pool reads them, wherever the board's pool finds them (R52) — at most
/// [`CARD_READ_BUDGET`] bytes of them, the rest reported.
fn insert_cards(
    conn: &Connection,
    session: &str,
    dir: &Path,
    report: &mut RebuildReport,
) -> Result<(), IndexError> {
    let read = read_card_files(session, dir, CARD_READ_BUDGET, CARD_READ_BUDGET);
    report.problems.extend(read.problems);
    for file in read.files {
        if !file.whole {
            report.problems.push(format!(
                "{session}: more than {CARD_READ_BUDGET} bytes of cards; {} and the cards after it were not read.",
                file.rel
            ));
            break;
        }
        insert_card(conn, session, &file.rel, &file.text)?;
    }
    Ok(())
}

/// One file a bounded read of a session's cards reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardFile {
    /// Session-relative.
    pub rel: String,
    /// Its text, as far as the read went.
    pub text: String,
    /// Whether `text` is the whole file: a longer one is cut, never read on.
    pub whole: bool,
}

/// What a bounded read of one session's card files found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CardFiles {
    pub files: Vec<CardFile>,
    /// What could not be listed or read, as sentences. While there is one,
    /// `files` is not every card file of the session.
    pub problems: Vec<String>,
}

/// Every `.md` file of the session folder `dir` where the board's pool reads
/// a card ([`card_rels`]'s walk), read by bounded reads rather than by
/// size: at most `per_file` bytes of each — a longer file comes back cut —
/// and `total` bytes in all, past which the files left are reported, not
/// read.
pub fn read_card_files(session: &str, dir: &Path, per_file: u64, total: u64) -> CardFiles {
    let mut report = RebuildReport::default();
    let rels = card_rels(session, dir, &mut report);
    let mut files = Vec::new();
    let mut left = total;
    for rel in rels {
        if left == 0 {
            report.problems.push(format!(
                "{session}: more than {total} bytes of cards; {rel} and the cards after it were not read."
            ));
            break;
        }
        let path = rel
            .split('/')
            .fold(dir.to_path_buf(), |path, part| path.join(part));
        match read_prefix(&path, per_file.min(left)) {
            Ok((text, read, whole)) => {
                left -= read;
                files.push(CardFile { rel, text, whole });
            }
            Err(error) => report.problems.push(format!("{session}/{rel}: {error}.")),
        }
    }
    CardFiles {
        files,
        problems: report.problems,
    }
}

/// At most `limit` bytes of the file at `path`: its text, the bytes read,
/// and whether that is the whole file. A cut text ends at the last whole
/// character; an uncut one that is not UTF-8 is an error.
pub fn read_prefix(path: &Path, limit: u64) -> std::io::Result<(String, u64, bool)> {
    use std::io::Read;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    let whole = bytes.len() as u64 <= limit;
    bytes.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
    let read = bytes.len() as u64;
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) if !whole => {
            let valid = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).unwrap_or_default()
        }
        Err(error) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                error.utf8_error(),
            ))
        }
    };
    Ok((text, read, whole))
}

/// One file's card row, when it is a card.
fn insert_card(conn: &Connection, session: &str, rel: &str, text: &str) -> Result<(), IndexError> {
    if read_one(PoolFile { rel, text }).kind != Some(KindTag::Task) {
        return Ok(());
    }
    let (fm, _) = Frontmatter::parse(text);
    let field = |key: &str| card::raw_key(&fm, key);
    conn.execute(
        "INSERT OR REPLACE INTO cards
         (session, rel, run, assignee, host, requested_by, schedule, last_run, workflow,
          scheduled_by, integrity)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            session,
            rel,
            field(card::RUN),
            field(card::ASSIGNEE),
            field(card::HOST),
            field(card::REQUESTED_BY),
            field(card::SCHEDULE),
            field(card::LAST_RUN),
            field(card::WORKFLOW),
            field(card::SCHEDULED_BY),
            field(card::INTEGRITY),
        ],
    )?;
    Ok(())
}

/// Whether a session-relative path is where the board's pool reads a card:
/// markdown in no dotted folder and under none of `artifacts/`,
/// `workspace/` and `log/` (folded, as the drive folds) — [`card_rels`]'s
/// walk, asked of one path.
fn card_place(rel: &str) -> bool {
    let skipped = [ARTIFACTS_DIR, WORKSPACE_DIR, LOG_DIR];
    let parts: Vec<&str> = rel.split('/').collect();
    let Some((name, folders)) = parts.split_last() else {
        return false;
    };
    !name.starts_with('.')
        && name.to_ascii_lowercase().ends_with(".md")
        && folders.iter().all(|folder| {
            !folder.starts_with('.')
                && !skipped.iter().any(|skip| folder.eq_ignore_ascii_case(skip))
        })
}

/// Every `.md` file of a session the board's pool can hold, session-relative
/// and sorted: each folder but a dotted one, `artifacts/`, `workspace/` and
/// `log/` (folded, as the drive folds), no link followed, at most
/// [`CARD_WALK_BUDGET`] entries visited.
fn card_rels(session: &str, dir: &Path, report: &mut RebuildReport) -> Vec<String> {
    let skipped = [ARTIFACTS_DIR, WORKSPACE_DIR, LOG_DIR];
    let mut out = Vec::new();
    let mut budget = CARD_WALK_BUDGET;
    let mut pending = vec![String::new()];
    while let Some(prefix) = pending.pop() {
        let here = prefix
            .split('/')
            .filter(|part| !part.is_empty())
            .fold(dir.to_path_buf(), |path, part| path.join(part));
        let entries = match fs::read_dir(&here) {
            Ok(entries) => entries,
            Err(error) => {
                report.problems.push(format!(
                    "{session}/{prefix}: the folder could not be listed: {error}."
                ));
                continue;
            }
        };
        for entry in entries.flatten() {
            if budget == 0 {
                report.problems.push(format!(
                    "{session}: more than {CARD_WALK_BUDGET} entries; the cards past them were not read."
                ));
                out.sort();
                return out;
            }
            budget -= 1;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if kind.is_dir() {
                if !skipped.iter().any(|skip| name.eq_ignore_ascii_case(skip)) {
                    pending.push(rel);
                }
            } else if kind.is_file() && name.to_ascii_lowercase().ends_with(".md") {
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}

/// Drop every row of the session at `path`.
fn forget(conn: &Connection, path: &str) -> Result<(), IndexError> {
    for table in [
        "sessions WHERE path",
        "chunks WHERE session",
        "cards WHERE session",
        "seen_events WHERE session",
        "fences WHERE session",
    ] {
        conn.execute(&format!("DELETE FROM {table} = ?1"), params![path])?;
    }
    Ok(())
}

/// What one grown chunk added past what the index held.
struct Tail {
    name: ChunkName,
    lines: Vec<LogLine>,
    /// Where its last complete line ends.
    bytes: u64,
    last_offset: u64,
}

/// The new lines of the chunks of `session_dir/log/` that grew since
/// `indexed`, read from where the index stopped — whole lines, about
/// `budget` bytes in all (a line longer than what is left is still read
/// whole, so a refresh always moves on); a chunk past the budget keeps its
/// cursor for the next read. `None` when the log was not only appended to —
/// a chunk shrank, went away, is not a file, or grew from mid-line — and
/// only a whole read can say what it holds.
fn grown_chunks(session_dir: &Path, indexed: &[ChunkRow], budget: u64) -> Option<Vec<Tail>> {
    use std::io::{Read, Seek, SeekFrom};

    let log_dir = session_dir.join(LOG_DIR);
    let mut on_disk: Vec<(ChunkName, u64)> = Vec::new();
    match fs::symlink_metadata(&log_dir) {
        Ok(meta) if meta.is_dir() => {
            for entry in fs::read_dir(&log_dir).ok()?.flatten() {
                let Some(Ok(name)) = entry.file_name().to_str().map(str::parse::<ChunkName>) else {
                    continue;
                };
                if !entry.file_type().ok()?.is_file() {
                    return None;
                }
                on_disk.push((name, entry.metadata().ok()?.len()));
            }
        }
        Ok(_) => return None,
        Err(_) => {}
    }
    let gone = indexed
        .iter()
        .any(|row| !on_disk.iter().any(|(name, _)| name.to_string() == row.name));
    if gone {
        return None;
    }
    let mut tails = Vec::new();
    let mut left = budget;
    on_disk.sort();
    for (name, size) in on_disk {
        let file = name.to_string();
        let (from, last_offset) = indexed
            .iter()
            .find(|row| row.name == file)
            .map_or((0, 0), |row| (row.bytes, row.last_offset));
        if size == from {
            continue;
        }
        if size < from {
            return None;
        }
        if left == 0 {
            continue;
        }
        // One byte before where the index stopped, so a stop that was not at
        // a line's end is seen rather than read from mid-line.
        let start = from.saturating_sub(1);
        let mut chunk = fs::File::open(log_dir.join(&file)).ok()?;
        chunk.seek(SeekFrom::Start(start)).ok()?;
        let mut bytes = Vec::new();
        // The byte before, then what the budget leaves.
        (&mut chunk).take(left + 1).read_to_end(&mut bytes).ok()?;
        if !bytes[1.min(bytes.len())..].contains(&b'\n') {
            chunk.read_to_end(&mut bytes).ok()?;
        }
        let tail = if from == 0 {
            &bytes[..]
        } else if bytes.first() == Some(&b'\n') {
            &bytes[1..]
        } else {
            return None;
        };
        let mut read = Tail {
            name,
            lines: Vec::new(),
            bytes: from,
            last_offset,
        };
        let mut at = 0usize;
        while let Some(end) = tail[at..].iter().position(|b| *b == b'\n') {
            if let Some(line) = std::str::from_utf8(&tail[at..at + end])
                .ok()
                .and_then(|text| LogLine::parse(text).ok())
            {
                read.lines.push(line);
            }
            read.last_offset = from + at as u64;
            at += end + 1;
            read.bytes = from + at as u64;
        }
        left = left.saturating_sub(read.bytes - from);
        tails.push(read);
    }
    Some(tails)
}

/// Every real (non-link) session folder holding an `agent.toml`, as
/// (zone-relative path, absolute path), in path order.
fn session_dirs(zone_root: &Path, report: &mut RebuildReport) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let real_dirs = |dir: &Path| -> Vec<(String, PathBuf)> {
        let Ok(entries) = fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut dirs: Vec<(String, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|entry| Some((entry.file_name().to_str()?.to_owned(), entry.path())))
            .collect();
        dirs.sort();
        dirs
    };
    for (name, dir) in real_dirs(&zone_root.join("active")) {
        out.push((format!("active/{name}"), dir));
    }
    for (year, year_dir) in real_dirs(&zone_root.join("archive")) {
        for (name, dir) in real_dirs(&year_dir) {
            out.push((format!("archive/{year}/{name}"), dir));
        }
    }
    out.retain(
        |(rel, dir)| match fs::symlink_metadata(dir.join(session::FILE_NAME)) {
            Ok(meta) if meta.file_type().is_file() => true,
            Ok(_) => {
                report.problems.push(format!(
                    "{rel}: agent.toml is not a regular file; the session was skipped."
                ));
                false
            }
            Err(_) => false,
        },
    );
    out
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use chrono::{TimeZone, Utc};
    use ulid::Ulid;

    use super::*;
    use crate::agents::log::{HostSlug, RunBody, RunState, LINE_VERSION};

    const SESSION: &str = "active/s";
    const CHUNK: &str = "2026-09-30.hesperia.1.jsonl";

    /// A zone holding one agent session with an empty log, removed on drop.
    struct Zone(PathBuf);

    impl Drop for Zone {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn zone() -> Zone {
        let root = std::env::temp_dir().join(format!("keeper-index-{}", Ulid::new()));
        let dir = root.join(SESSION);
        fs::create_dir_all(dir.join(LOG_DIR)).expect("log");
        fs::write(
            dir.join(session::FILE_NAME),
            include_str!(
                "../../tests/fixtures/agents/sessions/active/2026-09-30-release-notes/agent.toml"
            ),
        )
        .expect("agent.toml");
        Zone(root)
    }

    fn run_line(minute: u32, state: RunState) -> LogLine {
        let ts = Utc
            .with_ymd_and_hms(2026, 9, 30, 9, minute, 0)
            .single()
            .expect("ts");
        LogLine {
            v: LINE_VERSION,
            id: Ulid::from_parts(ts.timestamp_millis() as u64, u128::from(minute)),
            parent: None,
            ts,
            host: HostSlug::new("hesperia").expect("host"),
            epoch: 0,
            claim: None,
            matrix_event: None,
            body: LineBody::Run(RunBody {
                state,
                detail: None,
            }),
        }
    }

    /// Append `line` to the session's chunk, as the writer does, and its
    /// receipt.
    fn append(zone: &Path, line: &LogLine) -> AppendReceipt {
        let path = zone.join(SESSION).join(LOG_DIR).join(CHUNK);
        let offset = fs::metadata(&path).map_or(0, |meta| meta.len());
        let text = format!("{}\n", line.to_json().expect("json"));
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("chunk")
            .write_all(text.as_bytes())
            .expect("append");
        AppendReceipt {
            chunk: CHUNK.parse().expect("chunk name"),
            offset,
            bytes: text.len() as u64,
            line: line.clone(),
            blob: None,
        }
    }

    /// R121 (R4-12): a refresh holds the index's write lock from before it
    /// reads the row, so the live writer's `apply` on another connection
    /// waits for it rather than being written over; and a line a refresh
    /// already read is not projected again by its writer's `apply`.
    #[test]
    fn a_refresh_never_interleaves_with_the_writers_apply() {
        let zone = zone();
        let root = zone.0.clone();
        let mut board = Index::open(&root).expect("open");
        board.refresh_session(SESSION, None).expect("first read");
        let mut writer = Index::open(&root).expect("open");
        writer
            .apply(SESSION, &append(&root, &run_line(1, RunState::Running)))
            .expect("apply");

        let mut live = None;
        board
            .refresh_session_with(SESSION, None, &mut || {
                let root = root.clone();
                live = Some(std::thread::spawn(move || {
                    let receipt = append(&root, &run_line(2, RunState::Waiting));
                    Index::open(&root)
                        .expect("open")
                        .apply(SESSION, &receipt)
                        .expect("apply");
                }));
                std::thread::sleep(Duration::from_millis(300));
            })
            .expect("a refresh beside a live writer");
        live.expect("spawned").join().expect("writer");

        let read_first = append(&root, &run_line(3, RunState::Review));
        board.refresh_session(SESSION, None).expect("refresh");
        writer.apply(SESSION, &read_first).expect("apply");

        let row = board.session(SESSION).expect("row").expect("session");
        assert_eq!(row.lines, 3, "each line projected once");
        assert_eq!(row.run.as_deref(), Some("review"));
    }

    /// A card read never reads past its budgets: at most `per_file` bytes
    /// of a file, which then says it is cut, and at most `total` across the
    /// session, past which the files left are reported, not read.
    #[test]
    fn a_card_read_stays_within_its_budgets() {
        let dir = std::env::temp_dir().join(format!("keeper-cards-{}", Ulid::new()));
        fs::create_dir_all(&dir).expect("session");
        fs::write(dir.join("a.md"), "x".repeat(3 * 1024 * 1024)).expect("a");
        fs::write(dir.join("b.md"), "short").expect("b");
        fs::write(dir.join("c.md"), "y".repeat(800)).expect("c");
        fs::write(dir.join("d.md"), "z").expect("d");
        let read = read_card_files("s", &dir, 1_000, 1_600);
        fs::remove_dir_all(&dir).expect("cleaned");
        let seen: Vec<(&str, usize, bool)> = read
            .files
            .iter()
            .map(|file| (file.rel.as_str(), file.text.len(), file.whole))
            .collect();
        assert_eq!(
            seen,
            [
                ("a.md", 1_000, false),
                ("b.md", 5, true),
                ("c.md", 595, false)
            ]
        );
        assert_eq!(read.problems.len(), 1, "{:?}", read.problems);
        assert!(read.problems[0].contains("d.md"), "{:?}", read.problems);
    }
}
