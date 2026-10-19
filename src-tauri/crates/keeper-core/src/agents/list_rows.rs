//! The Agents window's last-known rows, kept in `keeper.db` (story 102.5,
//! R468).
//!
//! The Agents list is drawn from the account's room list, which exists only
//! once the account is activated — a keychain read, the store opened, the
//! sync started — and before this story each row's state was read before
//! anything was drawn. So what each account's Agents window last showed is
//! kept here, and the next subscription draws it before activation: the
//! room, its name (an agent's `<agent> — <title>`), its kind, the agent's
//! letter and picture, its state, its last activity and its unread counts.
//! No message text is kept: a row drawn from here has no preview until the
//! live row replaces it.
//!
//! A kept state is a past reading, not a reading: *nothing pending* is not
//! kept as such, since an old reading is not grounds to say nothing waits —
//! it comes back as *Unknown* until the first fresh reading (R436, R453).
//!
//! One table beside the seats (`agents/seats.rs`), under the same rules: WAL,
//! a busy timeout, `CREATE TABLE IF NOT EXISTS` on every open — a `keeper.db`
//! from before it gains it on first use and nothing else changes — and no
//! JSON in a row.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{params, Connection};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::account::lifetime::Lifetime;
use crate::agents::attention::AgentAttentionVm;
use crate::agents::room::AgentRoomKindVm;
use crate::error::{CoreError, PlatformError};
use crate::vm::{MuteState, RoomVm};

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

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
        "CREATE TABLE IF NOT EXISTS agent_list_rows(\
            account TEXT NOT NULL, \
            room_id TEXT NOT NULL, \
            display_name TEXT NOT NULL, \
            avatar_url TEXT, \
            agent_room TEXT NOT NULL, \
            agent_mark TEXT, \
            agent_avatar TEXT, \
            attention TEXT, \
            last_activity INTEGER, \
            is_unread INTEGER NOT NULL, \
            mention_count INTEGER NOT NULL, \
            mute_state TEXT NOT NULL, \
            PRIMARY KEY(account, room_id)\
        )",
        [],
    )
    .map_err(internal("could not ensure agent_list_rows schema"))?;
    Ok(conn)
}

/// An enum's one wire word (`proxy`, `needsYou`, `muted`…), as the webview
/// reads it.
fn word<T: Serialize>(value: T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
}

fn from_word<T: DeserializeOwned>(word: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(word.to_owned())).ok()
}

/// What a kept state says when drawn again: *nothing pending* was a reading
/// of then, so it is *Unknown* now (R436).
fn drawn_again(kept: Option<AgentAttentionVm>) -> AgentAttentionVm {
    kept.unwrap_or(AgentAttentionVm::Unknown)
}

/// Where the rows are kept: one device's `keeper.db`. Every write goes
/// through a [`RowWriter`], and the writes of one store are ordered and
/// fenced ([`Writes`]).
#[derive(Debug, Clone)]
pub struct ListRowStore {
    data_dir: PathBuf,
    writes: Arc<Writes>,
}

/// The order and the lifetime of a store's writes (R501, R516; reviews
/// R102L5-01, R102L6-01; DW-1550 closed): a write runs on the blocking pool
/// after its producer handed it over, so neither stopping the producer nor
/// its own place in the queue says when it lands. So every write and every
/// removal holds `gate`, one at a time; a write is made by a writer of one
/// account [`Lifetime`] and writes only while that lifetime is live — a
/// sign-out ends it before it removes the rows, so a write queued before it
/// writes nothing, however late it runs, and no writer is made for an ended
/// lifetime; and a write carries its place in the order its snapshot was
/// taken, so an older snapshot landing after a newer one writes nothing.
#[derive(Debug, Default)]
struct Writes {
    /// Held across every write and removal; per account, the place of the
    /// newest snapshot written.
    gate: Mutex<HashMap<String, u64>>,
    /// The place of the next snapshot handed over.
    next: AtomicU64,
    /// Tests hold a write between its hand-over and the gate.
    #[cfg(test)]
    held: (Mutex<bool>, std::sync::Condvar),
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Plain maps of numbers: a panic mid-update tears nothing.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// While alive, every write waits between its hand-over and the gate
/// ([`ListRowStore::hold_writes`]).
#[cfg(test)]
pub(crate) struct HeldWrites(Arc<Writes>);

#[cfg(test)]
impl Drop for HeldWrites {
    fn drop(&mut self) {
        *locked(&self.0.held.0) = false;
        self.0.held.1.notify_all();
    }
}

/// What became of one snapshot handed to [`RowWriter::keep`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// It is the account's kept rows now.
    Written,
    /// A newer snapshot was written first; this one was dropped.
    Superseded,
    /// The account's lifetime its writer was made in ended (a sign-out, a
    /// teardown): it was dropped.
    Fenced,
}

/// One producer's writer of an account's kept rows: what it hands over is
/// written in order, and only while the account lifetime it was made in is
/// live.
#[derive(Debug, Clone)]
pub struct RowWriter {
    store: ListRowStore,
    life: Lifetime,
}

impl RowWriter {
    /// Keep `rows` as the account's Agents rows, on the blocking pool;
    /// resolves to what became of them.
    pub fn keep(&self, rows: Vec<RoomVm>) -> tokio::task::JoinHandle<Result<Keep, CoreError>> {
        let place = self.store.writes.next.fetch_add(1, Ordering::SeqCst) + 1;
        let writer = self.clone();
        tokio::task::spawn_blocking(move || writer.store.commit(&writer, place, &rows))
    }
}

impl ListRowStore {
    pub fn new(data_dir: &Path) -> ListRowStore {
        ListRowStore {
            data_dir: data_dir.to_owned(),
            writes: Arc::default(),
        }
    }

    /// A writer of the account's rows in `life`; none once `life` is not
    /// live — work that resumes after its account's sign-out began gets no
    /// writer (R516).
    pub(crate) fn writer(&self, life: &Lifetime) -> Option<RowWriter> {
        life.is_live().then(|| RowWriter {
            store: self.clone(),
            life: life.clone(),
        })
    }

    fn commit(&self, writer: &RowWriter, place: u64, rows: &[RoomVm]) -> Result<Keep, CoreError> {
        #[cfg(test)]
        {
            let (held, released) = &self.writes.held;
            let mut holding = locked(held);
            while *holding {
                holding = released
                    .wait(holding)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        }
        let mut written = locked(&self.writes.gate);
        let account = writer.life.account();
        if !writer.life.is_live() {
            return Ok(Keep::Fenced);
        }
        if written.get(account).is_some_and(|newest| *newest > place) {
            return Ok(Keep::Superseded);
        }
        self.write(account, rows)?;
        written.insert(account.to_owned(), place);
        Ok(Keep::Written)
    }

    /// Hold every write between its hand-over and the gate until the
    /// returned guard drops.
    #[cfg(test)]
    pub(crate) fn hold_writes(&self) -> HeldWrites {
        *locked(&self.writes.held.0) = true;
        HeldWrites(Arc::clone(&self.writes))
    }

    /// `account`'s kept Agents rows, newest activity first, each as the
    /// row it was, its state drawn again ([`drawn_again`]). A row whose
    /// kind no longer reads is left out.
    pub fn of(&self, account: &str) -> Result<Vec<RoomVm>, CoreError> {
        let conn = open(&self.data_dir)?;
        let mut stmt = conn
            .prepare(
                "SELECT room_id, display_name, avatar_url, agent_room, agent_mark, \
                 agent_avatar, attention, last_activity, is_unread, mention_count, mute_state \
                 FROM agent_list_rows WHERE account = ?1 \
                 ORDER BY last_activity IS NULL, last_activity DESC, room_id",
            )
            .map_err(internal("could not read agent_list_rows"))?;
        let rows = stmt
            .query_map(params![account], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, bool>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, String>(10)?,
                ))
            })
            .map_err(internal("could not read agent_list_rows"))?;
        let mut kept = Vec::new();
        for row in rows {
            let (
                room_id,
                display_name,
                avatar_url,
                agent_room,
                agent_mark,
                agent_avatar,
                attention,
                timestamp,
                is_unread,
                mention_count,
                mute_state,
            ) = row.map_err(internal("could not read agent_list_rows"))?;
            let Some(agent_room) = from_word::<AgentRoomKindVm>(&agent_room) else {
                continue;
            };
            let attention = attention.as_deref().and_then(from_word::<AgentAttentionVm>);
            kept.push(RoomVm {
                room_id,
                display_name,
                last_message: None,
                timestamp,
                avatar_url,
                is_unread,
                mention_count: u32::try_from(mention_count).unwrap_or(0),
                is_archived: false,
                is_favourite: false,
                is_space: false,
                network: None,
                network_id: None,
                mute_state: from_word(&mute_state).unwrap_or(MuteState::None),
                agent_room: Some(agent_room),
                agent_avatar,
                agent_mark,
                // Not kept: the row's agent is read with its live list, and
                // its Default marker appears then (DW-1573).
                agent: None,
                agent_attention: Some(drawn_again(attention)),
            });
        }
        Ok(kept)
    }

    /// Keep `rows` as `account`'s Agents rows, replacing what was kept: a
    /// row that left the window leaves the table. Rows that are no agent
    /// session room's are not kept. Unordered and unfenced: tests seed with
    /// it, everything else writes through a [`RowWriter`].
    #[cfg(test)]
    pub(crate) fn replace(&self, account: &str, rows: &[RoomVm]) -> Result<(), CoreError> {
        self.write(account, rows)
    }

    fn write(&self, account: &str, rows: &[RoomVm]) -> Result<(), CoreError> {
        let mut conn = open(&self.data_dir)?;
        let tx = conn
            .transaction()
            .map_err(internal("could not write agent_list_rows"))?;
        tx.execute(
            "DELETE FROM agent_list_rows WHERE account = ?1",
            params![account],
        )
        .map_err(internal("could not write agent_list_rows"))?;
        for row in rows {
            let Some(agent_room) = row
                .agent_room
                .filter(|kind| *kind != AgentRoomKindVm::Control)
                .and_then(word)
            else {
                continue;
            };
            tx.execute(
                "INSERT INTO agent_list_rows(account, room_id, display_name, avatar_url, \
                 agent_room, agent_mark, agent_avatar, attention, last_activity, is_unread, \
                 mention_count, mute_state) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    account,
                    row.room_id,
                    row.display_name,
                    row.avatar_url,
                    agent_room,
                    row.agent_mark,
                    row.agent_avatar,
                    row.agent_attention.and_then(word),
                    row.timestamp,
                    row.is_unread,
                    i64::from(row.mention_count),
                    word(row.mute_state).unwrap_or_else(|| "none".to_owned()),
                ],
            )
            .map_err(internal("could not write agent_list_rows"))?;
        }
        tx.commit()
            .map_err(internal("could not write agent_list_rows"))?;
        Ok(())
    }

    /// Remove every row kept for `account`: it signed out. Call it once the
    /// account's lifetime ended ([`crate::account::lifetime::AccountLifetimes::close`]):
    /// a write after that is fenced, and one under way holds the gate until
    /// it landed, so the rows go after it. Blocks on a write in progress:
    /// call it off the async runtime.
    pub fn forget(&self, account: &str) -> Result<(), CoreError> {
        let mut written = locked(&self.writes.gate);
        written.remove(account);
        let conn = open(&self.data_dir)?;
        conn.execute(
            "DELETE FROM agent_list_rows WHERE account = ?1",
            params![account],
        )
        .map_err(internal("could not forget agent_list_rows"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::lifetime::AccountLifetimes;

    fn row(room: &str, at: i64, attention: Option<AgentAttentionVm>) -> RoomVm {
        RoomVm {
            room_id: room.to_owned(),
            display_name: format!("Nixi — {room}"),
            last_message: Some("the agent's last words".to_owned()),
            timestamp: Some(at),
            avatar_url: None,
            is_unread: true,
            mention_count: 2,
            is_archived: false,
            is_favourite: false,
            is_space: false,
            network: None,
            network_id: None,
            mute_state: MuteState::Muted,
            agent_room: Some(AgentRoomKindVm::Proxy),
            agent_avatar: Some("mxc://h/nixi".to_owned()),
            agent_mark: Some("N".to_owned()),
            agent: None,
            agent_attention: attention,
        }
    }

    fn rooms_of(store: &ListRowStore, account: &str) -> Vec<String> {
        store
            .of(account)
            .expect("read")
            .into_iter()
            .map(|row| row.room_id)
            .collect()
    }

    /// R501, R516 (reviews R102L5-01, R102L6-01): a write handed over before
    /// a sign-out and still queued when the account's rows were removed
    /// lands after the removal and writes nothing; its writer writes nothing
    /// after either, and the ended lifetime gets no new writer. A writer of
    /// a new activation's lifetime writes again.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_write_queued_before_a_sign_out_never_brings_the_rows_back() {
        let dir = tempfile::tempdir().expect("dir");
        let store = ListRowStore::new(dir.path());
        let lives = AccountLifetimes::default();
        store
            .replace("acct", &[row("!kept:h", 1, None)])
            .expect("seed");
        let life = lives.begin("acct").expect("activated");
        let writer = store.writer(&life).expect("a live account's writer");

        let held = store.hold_writes();
        let queued = writer.keep(vec![row("!late:h", 2, None)]);
        let closing = lives.close("acct");
        let removing = store.clone();
        tokio::task::spawn_blocking(move || removing.forget("acct"))
            .await
            .expect("join")
            .expect("forget");
        drop(closing);
        assert!(rooms_of(&store, "acct").is_empty(), "signed out");
        drop(held);

        assert_eq!(queued.await.expect("join").expect("keep"), Keep::Fenced);
        assert!(
            rooms_of(&store, "acct").is_empty(),
            "the queued write brought no row back"
        );
        assert_eq!(
            writer
                .keep(vec![row("!later:h", 3, None)])
                .await
                .expect("join")
                .expect("keep"),
            Keep::Fenced
        );
        assert!(
            store.writer(&life).is_none(),
            "no writer for work that resumes after the sign-out"
        );
        assert!(rooms_of(&store, "acct").is_empty());

        let again = lives.begin("acct").expect("signed in again");
        let again = store.writer(&again).expect("the new activation's writer");
        assert_eq!(
            again
                .keep(vec![row("!back:h", 4, None)])
                .await
                .expect("join")
                .expect("keep"),
            Keep::Written
        );
        assert_eq!(rooms_of(&store, "acct"), ["!back:h"]);
    }

    /// R501: the newest snapshot wins — one taken earlier that lands later
    /// writes nothing; another account's writes are their own.
    #[test]
    fn an_older_snapshot_landing_after_a_newer_one_is_dropped() {
        let dir = tempfile::tempdir().expect("dir");
        let store = ListRowStore::new(dir.path());
        let lives = AccountLifetimes::default();
        let life = lives.begin("acct").expect("activated");
        let writer = store.writer(&life).expect("writer");
        assert_eq!(
            store
                .commit(&writer, 2, &[row("!newer:h", 2, None)])
                .expect("keep"),
            Keep::Written
        );
        assert_eq!(
            store
                .commit(&writer, 1, &[row("!older:h", 1, None)])
                .expect("keep"),
            Keep::Superseded
        );
        assert_eq!(rooms_of(&store, "acct"), ["!newer:h"]);
        let other = lives.begin("other").expect("activated");
        let other = store.writer(&other).expect("writer");
        assert_eq!(
            store
                .commit(&other, 1, &[row("!theirs:h", 1, None)])
                .expect("keep"),
            Keep::Written
        );
        assert_eq!(rooms_of(&store, "other"), ["!theirs:h"]);
    }

    /// R468: an account's rows outlive the store's handle (the next launch
    /// reads them), newest activity first, each account its own; a kept
    /// *nothing pending* comes back *Unknown*, never as a reading (R436);
    /// no message text is kept; a replacement drops the rows that left; a
    /// signed-out account's rows go.
    #[test]
    fn the_agents_rows_outlive_a_restart_and_nothing_pending_comes_back_unknown() {
        let dir = tempfile::tempdir().expect("dir");
        let store = ListRowStore::new(dir.path());
        store
            .replace(
                "acct",
                &[
                    row("!old:h", 10, Some(AgentAttentionVm::NeedsYou)),
                    row("!new:h", 20, None),
                ],
            )
            .expect("keep");
        store
            .replace("other", &[row("!theirs:h", 30, None)])
            .expect("keep");

        let again = ListRowStore::new(dir.path()).of("acct").expect("read");
        let rooms: Vec<&str> = again.iter().map(|row| row.room_id.as_str()).collect();
        assert_eq!(rooms, ["!new:h", "!old:h"], "newest first, this account's");
        assert_eq!(again[0].agent_attention, Some(AgentAttentionVm::Unknown));
        assert_eq!(again[1].agent_attention, Some(AgentAttentionVm::NeedsYou));
        assert_eq!(again[1].display_name, "Nixi — !old:h");
        assert_eq!(again[1].agent_room, Some(AgentRoomKindVm::Proxy));
        assert_eq!(again[1].agent_mark.as_deref(), Some("N"));
        assert_eq!(again[1].agent_avatar.as_deref(), Some("mxc://h/nixi"));
        assert_eq!((again[1].is_unread, again[1].mention_count), (true, 2));
        assert_eq!(again[1].mute_state, MuteState::Muted);
        assert_eq!(again[1].last_message, None, "no message text is kept");

        store
            .replace(
                "acct",
                &[row("!old:h", 40, Some(AgentAttentionVm::Working))],
            )
            .expect("keep");
        let replaced = store.of("acct").expect("read");
        assert_eq!(replaced.len(), 1, "a row that left the window is gone");
        assert_eq!(replaced[0].agent_attention, Some(AgentAttentionVm::Working));

        store.forget("acct").expect("forget");
        assert!(store.of("acct").expect("read").is_empty());
        assert_eq!(store.of("other").expect("read").len(), 1);
    }

    /// A `keeper.db` made before the table gains it on first use, and its
    /// other tables — the seats included — are untouched.
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
        let store = ListRowStore::new(dir.path());
        assert!(store.of("acct").expect("read").is_empty());
        store
            .replace("acct", &[row("!dm:h", 1, None)])
            .expect("keep");
        let conn = Connection::open(dir.path().join("keeper.db")).expect("open");
        let value: String = conn
            .query_row("SELECT value FROM settings WHERE key = 'a'", [], |row| {
                row.get(0)
            })
            .expect("kept");
        assert_eq!(value, "b");
    }
}
