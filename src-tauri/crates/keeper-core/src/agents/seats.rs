//! An agent room's admitted seat, kept in `keeper.db` (story 101.1, R416).
//!
//! A session room's kind, the agent working it and its title come from the
//! newest readable status, and a status can be out of reach: a host's claim
//! renewals and a quiet night bury it past the header's page-back, and the
//! event cache is empty after a restart. So once a status is read, what it
//! said — its seat — is kept here per account (the account's own Matrix user)
//! and room, and read when no status is in reach. A newer readable status
//! always replaces it, whatever kind or agent it names; an older one never
//! does.
//!
//! One table beside the registry's, under its rules: WAL, a busy timeout,
//! `CREATE TABLE IF NOT EXISTS` on every open — so a `keeper.db` from before
//! it gains it on first use and nothing else changes — and no JSON in a row.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, RoomId, UserId};
use rusqlite::{params, Connection};

use crate::agents::session::SessionKind;
use crate::error::{CoreError, PlatformError};

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// What a room's newest readable status said of its session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    pub kind: SessionKind,
    /// The agent working the session: the delegate in a delegated room.
    pub agent: OwnedUserId,
    pub title: String,
    /// The status's `origin_server_ts`, ms: which of two seats is newer.
    pub status_ts: u64,
}

impl Seat {
    /// Whether `self` may replace `held`: it is from the same status or a
    /// newer one.
    pub fn replaces(&self, held: Option<&Seat>) -> bool {
        held.is_none_or(|held| self.status_ts >= held.status_ts)
    }
}

fn internal(what: &str) -> impl Fn(rusqlite::Error) -> CoreError + '_ {
    move |error| CoreError::Internal(format!("{what}: {error}"))
}

fn open(data_dir: &Path) -> Result<Connection, CoreError> {
    std::fs::create_dir_all(data_dir).map_err(|e| {
        CoreError::Platform(PlatformError::DirUnavailable(format!(
            "could not create data dir: {e}"
        )))
    })?;
    let conn = Connection::open(data_dir.join("keeper.db"))
        .map_err(internal("could not open keeper.db"))?;
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(internal("could not set busy timeout"))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(internal("could not set WAL mode"))?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_room_seats(\
            account TEXT NOT NULL, \
            room_id TEXT NOT NULL, \
            kind TEXT NOT NULL, \
            agent TEXT NOT NULL, \
            title TEXT NOT NULL, \
            status_ts INTEGER NOT NULL, \
            PRIMARY KEY(account, room_id)\
        )",
        [],
    )
    .map_err(internal("could not ensure agent_room_seats schema"))?;
    Ok(conn)
}

/// Where the seats are kept: one device's `keeper.db`.
#[derive(Debug, Clone)]
pub struct SeatStore {
    data_dir: PathBuf,
}

impl SeatStore {
    pub fn new(data_dir: &Path) -> SeatStore {
        SeatStore {
            data_dir: data_dir.to_owned(),
        }
    }

    /// Every seat kept, by account and room. A row whose ids or kind no
    /// longer read is left out: it seats nothing.
    pub fn all(&self) -> Result<HashMap<(OwnedUserId, OwnedRoomId), Seat>, CoreError> {
        let conn = open(&self.data_dir)?;
        let mut stmt = conn
            .prepare("SELECT account, room_id, kind, agent, title, status_ts FROM agent_room_seats")
            .map_err(internal("could not read agent_room_seats"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(internal("could not read agent_room_seats"))?;
        let mut seats = HashMap::new();
        for row in rows {
            let (account, room, kind, agent, title, status_ts) =
                row.map_err(internal("could not read agent_room_seats"))?;
            let (Ok(account), Ok(room), Ok(kind), Ok(agent)) = (
                OwnedUserId::try_from(account),
                OwnedRoomId::try_from(room),
                SessionKind::from_str(&kind),
                OwnedUserId::try_from(agent),
            ) else {
                continue;
            };
            seats.insert(
                (account, room),
                Seat {
                    kind,
                    agent,
                    title,
                    status_ts: u64::try_from(status_ts).unwrap_or(0),
                },
            );
        }
        Ok(seats)
    }

    /// Keep `seat` for `account`'s `room`, unless the row holds a newer one.
    pub fn put(&self, account: &UserId, room: &RoomId, seat: &Seat) -> Result<(), CoreError> {
        let conn = open(&self.data_dir)?;
        conn.execute(
            "INSERT INTO agent_room_seats(account, room_id, kind, agent, title, status_ts) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             ON CONFLICT(account, room_id) DO UPDATE SET \
                kind = excluded.kind, agent = excluded.agent, title = excluded.title, \
                status_ts = excluded.status_ts \
             WHERE excluded.status_ts >= agent_room_seats.status_ts",
            params![
                account.as_str(),
                room.as_str(),
                seat.kind.as_str(),
                seat.agent.as_str(),
                seat.title,
                i64::try_from(seat.status_ts).unwrap_or(i64::MAX),
            ],
        )
        .map_err(internal("could not write agent_room_seats"))?;
        Ok(())
    }
}

impl SeatStore {
    /// Remove every seat kept for `account`: it signed out.
    pub fn forget(&self, account: &UserId) -> Result<(), CoreError> {
        let conn = open(&self.data_dir)?;
        conn.execute(
            "DELETE FROM agent_room_seats WHERE account = ?1",
            params![account.as_str()],
        )
        .map_err(internal("could not forget agent_room_seats"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    fn room(id: &str) -> OwnedRoomId {
        OwnedRoomId::try_from(id).expect("room")
    }

    fn seat(kind: SessionKind, agent: &str, title: &str, status_ts: u64) -> Seat {
        Seat {
            kind,
            agent: user(agent),
            title: title.to_owned(),
            status_ts,
        }
    }

    /// R416: a seat kept outlives the store's handle (a restart reads it),
    /// is kept apart per account, and a newer status replaces it — another
    /// kind or agent included — while an older one never does.
    #[test]
    fn a_kept_seat_survives_a_restart_and_only_a_newer_status_replaces_it() {
        let dir = tempfile::tempdir().expect("dir");
        let (me, other) = (user("@tgorka:example.org"), user("@marta:example.org"));
        let dm = room("!dm:example.org");
        let store = SeatStore::new(dir.path());
        let first = seat(SessionKind::Main, "@nixi:example.org", "main", 100);
        store.put(&me, &dm, &first).expect("put");
        store
            .put(
                &other,
                &dm,
                &seat(SessionKind::Conversation, "@tola:example.org", "x", 50),
            )
            .expect("put");

        let again = SeatStore::new(dir.path()).all().expect("read");
        assert_eq!(again.get(&(me.clone(), dm.clone())), Some(&first));
        assert_eq!(
            again
                .get(&(other.clone(), dm.clone()))
                .map(|s| s.agent.as_str()),
            Some("@tola:example.org"),
            "each account its own"
        );

        let older = seat(SessionKind::Delegated, "@tola:example.org", "old", 99);
        store.put(&me, &dm, &older).expect("put");
        assert_eq!(
            store.all().expect("read").get(&(me.clone(), dm.clone())),
            Some(&first)
        );

        let newer = seat(SessionKind::Delegated, "@tola:example.org", "triage", 101);
        store.put(&me, &dm, &newer).expect("put");
        assert_eq!(store.all().expect("read").get(&(me, dm)), Some(&newer));
    }

    /// A `keeper.db` made before the table gains it on first use, and its
    /// other tables are untouched.
    #[test]
    fn an_existing_database_gains_the_table() {
        let dir = tempfile::tempdir().expect("dir");
        let conn = Connection::open(dir.path().join("keeper.db")).expect("open");
        conn.execute(
            "CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT)",
            [],
        )
        .expect("create");
        conn.execute("INSERT INTO settings VALUES ('a', 'b')", [])
            .expect("insert");
        drop(conn);
        let store = SeatStore::new(dir.path());
        assert!(store.all().expect("read").is_empty());
        store
            .put(
                &user("@tgorka:example.org"),
                &room("!dm:example.org"),
                &seat(SessionKind::Main, "@nixi:example.org", "main", 1),
            )
            .expect("put");
        let conn = Connection::open(dir.path().join("keeper.db")).expect("open");
        let value: String = conn
            .query_row("SELECT value FROM settings WHERE key = 'a'", [], |row| {
                row.get(0)
            })
            .expect("kept");
        assert_eq!(value, "b");
    }
}
