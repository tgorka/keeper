//! `<zone>/.keeper/agents.db`: the derived, disposable index of a sessions
//! zone's agent sessions (AD-365, D-21; story 89.5).
//!
//! The board, the session list and a turn answer from here and from the
//! writer's in-memory tail, never by re-reading a log (NFR-116). It is
//! rebuilt from the logs, the session `agent.toml` files and the cards, and
//! kept current by [`Index::apply`] after each append. A schema version this
//! build does not know is dropped and rebuilt, never migrated and never an
//! error: nothing in it is anywhere but in the files.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};

use crate::agents::label::Label;
use crate::agents::log::reader::{read_session, SessionLog};
use crate::agents::log::writer::AppendReceipt;
use crate::agents::log::{ClaimAction, LineBody, LogLine};
use crate::agents::session::{self, parse_session_agent_toml, SessionAgent};
use crate::sessions::pool::{read_one, PoolFile};
use crate::sessions::shape::KindTag;

/// The index's folder and file inside a zone.
pub const INDEX_PATH: [&str; 2] = [".keeper", "agents.db"];

/// The schema this build writes. A different `user_version` rebuilds.
pub const SCHEMA_VERSION: i64 = 1;

/// The card fields the index projects (ruling R2).
pub const CARD_FIELDS: [&str; 7] = [
    "run",
    "assignee",
    "host",
    "requested_by",
    "schedule",
    "last_run",
    "workflow",
];

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
    PRIMARY KEY (session, rel)
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
    /// The host that holds the claim, while one does.
    pub claim_host: Option<String>,
    /// The last claim epoch.
    pub claim_epoch: Option<u64>,
    /// How many lines the log holds.
    pub lines: u64,
    /// The newest line's `ts`.
    pub last_ts: Option<String>,
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

/// One card's agent fields.
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
                 DROP TABLE IF EXISTS cards; DROP TABLE IF EXISTS seen_events;",
            )?;
            conn.execute_batch(SCHEMA)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(Index {
            conn,
            zone_root: zone_root.to_owned(),
            rebuilt_schema,
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
            "DELETE FROM sessions; DELETE FROM chunks; DELETE FROM cards; DELETE FROM seen_events;",
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

    /// Project one appended line into its session's row and chunk.
    pub fn apply(&mut self, session: &str, receipt: &AppendReceipt) -> Result<(), IndexError> {
        let Some(mut row) = self.session(session)? else {
            return Err(IndexError::UnknownSession(session.to_owned()));
        };
        project(&mut row, &receipt.line);
        let tx = self.conn.transaction()?;
        write_row(&tx, &row)?;
        tx.execute(
            "INSERT INTO chunks (session, name, host, n, bytes, last_offset)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (session, name) DO UPDATE SET bytes = ?5, last_offset = ?6",
            params![
                session,
                receipt.chunk.to_string(),
                receipt.chunk.host.as_str(),
                receipt.chunk.n,
                receipt.offset + receipt.bytes,
                receipt.offset
            ],
        )?;
        if let Some(event) = &receipt.line.matrix_event {
            tx.execute(
                "INSERT OR IGNORE INTO seen_events (session, event_id) VALUES (?1, ?2)",
                params![session, event.as_str()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// One session's row.
    pub fn session(&self, path: &str) -> Result<Option<SessionRow>, IndexError> {
        let raw = self
            .conn
            .query_row(
                &format!("{SESSION_SELECT} WHERE path = ?1"),
                params![path],
                raw_session,
            )
            .optional()?;
        raw.map(RawSession::into_row).transpose()
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
        let mut statement = self.conn.prepare(
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

    /// One session's cards, by path.
    pub fn cards(&self, session: &str) -> Result<Vec<CardRow>, IndexError> {
        let mut statement = self.conn.prepare(
            "SELECT rel, run, assignee, host, requested_by, schedule, last_run, workflow
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
    claim_host, claim_epoch, lines, last_ts FROM sessions";

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
            claim_host: row.get(10)?,
            claim_epoch: row.get::<_, Option<i64>>(11)?.map(|e| e as u64),
            lines: row.get::<_, i64>(12)? as u64,
            last_ts: row.get(13)?,
        },
        label: row.get(7)?,
        scope: row.get(8)?,
    })
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
        LineBody::Run(body) => row.run = Some(body.state.as_str().to_owned()),
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
            claim_host, claim_epoch, lines, last_ts)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT (path) DO UPDATE SET id = ?2, agent = ?3, drive = ?4, kind = ?5,
            title = ?6, room = ?7, label = ?8, scope = ?9, run = ?10, claim_host = ?11,
            claim_epoch = ?12, lines = ?13, last_ts = ?14",
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
        claim_host: None,
        claim_epoch: None,
        lines: 0,
        last_ts: None,
    };
    for line in &log.lines {
        project(&mut row, line);
        if let Some(event) = &line.matrix_event {
            conn.execute(
                "INSERT OR IGNORE INTO seen_events (session, event_id) VALUES (?1, ?2)",
                params![path, event.as_str()],
            )?;
        }
    }
    write_row(conn, &row)?;
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

/// The cards of one session: its root's markdown files tagged `task`, read
/// as the pool reads them.
fn insert_cards(
    conn: &Connection,
    session: &str,
    dir: &Path,
    report: &mut RebuildReport,
) -> Result<(), IndexError> {
    let entries = fs::read_dir(dir).map_err(|e| io(dir, e))?;
    for entry in entries.filter_map(Result::ok) {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.ends_with(".md") || !entry.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let text = match fs::read_to_string(entry.path()) {
            Ok(text) => text,
            Err(error) => {
                report.problems.push(format!("{session}/{name}: {error}."));
                continue;
            }
        };
        let card = read_one(PoolFile {
            rel: &name,
            text: &text,
        });
        if card.kind != Some(KindTag::Task) {
            continue;
        }
        let field = |key: &str| card.fields.get(key).cloned();
        conn.execute(
            "INSERT OR REPLACE INTO cards
             (session, rel, run, assignee, host, requested_by, schedule, last_run, workflow)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                session,
                card.rel,
                field(CARD_FIELDS[0]),
                field(CARD_FIELDS[1]),
                field(CARD_FIELDS[2]),
                field(CARD_FIELDS[3]),
                field(CARD_FIELDS[4]),
                field(CARD_FIELDS[5]),
                field(CARD_FIELDS[6]),
            ],
        )?;
    }
    Ok(())
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
