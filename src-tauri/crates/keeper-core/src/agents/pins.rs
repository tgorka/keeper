//! The desktop's pins: who may read a drive whose agents this Mac hosts
//! (ruling R28 S-15; story 90.6).
//!
//! A pin is the owner, the readers and `local_only` of a drive's
//! `_drive.toml` as the person saw them when they signed an agent in on this
//! Mac, kept in `keeper.db` — device-local, never in the account's synced
//! settings, and never in a `keeper.toml` layer, because `_drive.toml` is
//! editable by every reader and the pin is what the Mac believes instead.
//! [`crate::agents::mount::pin_matches`] compares the two; a zone that
//! differs from its pin hosts nothing here until the person re-pins.
//!
//! Two tables beside the registry's, under its rules: WAL, a busy timeout,
//! `CREATE TABLE IF NOT EXISTS` on every open, no JSON blob in a row.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use matrix_sdk::ruma::OwnedUserId;
use rusqlite::{params, Connection};

use crate::agents::agentd::DrivePin;
use crate::error::{CoreError, PlatformError};

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
    // One row per synced folder whose agents zone this Mac pinned, keyed by
    // the sync profile: two folders of one drive are pinned apart.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_pins(\
            profile_id TEXT PRIMARY KEY, \
            drive_id TEXT NOT NULL, \
            remote TEXT NOT NULL, \
            owner TEXT NOT NULL, \
            local_only INTEGER NOT NULL, \
            pinned_ms INTEGER NOT NULL\
        )",
        [],
    )
    .map_err(internal("could not ensure agent_pins schema"))?;
    // No foreign key: rusqlite leaves `foreign_keys` off, so a cascade would
    // promise what the connection does not do. `set_pin` replaces a
    // profile's readers itself, in the pin's transaction.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS agent_pin_readers(\
            profile_id TEXT NOT NULL, \
            reader TEXT NOT NULL, \
            PRIMARY KEY(profile_id, reader)\
        )",
        [],
    )
    .map_err(internal("could not ensure agent_pin_readers schema"))?;
    Ok(conn)
}

/// Every pin on this Mac, by sync profile id. A row whose ids no longer
/// read is dropped from the answer, so it pins nothing.
pub fn pins(data_dir: &Path) -> Result<BTreeMap<String, DrivePin>, CoreError> {
    let conn = open(data_dir)?;
    let mut readers: BTreeMap<String, BTreeSet<OwnedUserId>> = BTreeMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT profile_id, reader FROM agent_pin_readers")
            .map_err(internal("could not read agent_pin_readers"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(internal("could not read agent_pin_readers"))?;
        for row in rows {
            let (profile, reader) = row.map_err(internal("could not read agent_pin_readers"))?;
            if let Ok(reader) = OwnedUserId::try_from(reader) {
                readers.entry(profile).or_default().insert(reader);
            }
        }
    }
    let mut stmt = conn
        .prepare("SELECT profile_id, drive_id, remote, owner, local_only FROM agent_pins")
        .map_err(internal("could not read agent_pins"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, bool>(4)?,
            ))
        })
        .map_err(internal("could not read agent_pins"))?;
    let mut pins = BTreeMap::new();
    for row in rows {
        let (profile, id, remote, owner, local_only) =
            row.map_err(internal("could not read agent_pins"))?;
        let Ok(owner) = OwnedUserId::try_from(owner) else {
            continue;
        };
        let readers = readers.remove(&profile).unwrap_or_default();
        pins.insert(
            profile,
            DrivePin {
                id,
                remote,
                credential: None,
                owner,
                readers,
                local_only,
            },
        );
    }
    Ok(pins)
}

/// Pin `pin` for `profile_id`, replacing what was pinned before, readers
/// and all, in one transaction.
pub fn set_pin(
    data_dir: &Path,
    profile_id: &str,
    pin: &DrivePin,
    now_ms: i64,
) -> Result<(), CoreError> {
    let mut conn = open(data_dir)?;
    let tx = conn
        .transaction()
        .map_err(internal("could not start a pin write"))?;
    tx.execute(
        "DELETE FROM agent_pin_readers WHERE profile_id = ?1",
        params![profile_id],
    )
    .map_err(internal("could not replace the pinned readers"))?;
    tx.execute(
        "INSERT INTO agent_pins(profile_id, drive_id, remote, owner, local_only, pinned_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(profile_id) DO UPDATE SET drive_id = excluded.drive_id, \
         remote = excluded.remote, owner = excluded.owner, \
         local_only = excluded.local_only, pinned_ms = excluded.pinned_ms",
        params![
            profile_id,
            pin.id,
            pin.remote,
            pin.owner.as_str(),
            pin.local_only,
            now_ms
        ],
    )
    .map_err(internal("could not write the pin"))?;
    for reader in &pin.readers {
        tx.execute(
            "INSERT INTO agent_pin_readers(profile_id, reader) VALUES (?1, ?2)",
            params![profile_id, reader.as_str()],
        )
        .map_err(internal("could not write a pinned reader"))?;
    }
    tx.commit().map_err(internal("could not commit the pin"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::mount::readers_from;

    fn pin(readers: &[&str]) -> DrivePin {
        DrivePin {
            id: "tgdrive".to_owned(),
            remote: "git@forge:tgorka/tgdrive.git".to_owned(),
            credential: None,
            owner: OwnedUserId::try_from("@tgorka:example.org").expect("owner"),
            readers: readers_from(readers).expect("readers"),
            local_only: true,
        }
    }

    fn temp_dir() -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "keeper-agent-pins-test-{}-{}-{n}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ))
    }

    /// A re-pin replaces the readers rather than adding to them: a reader the
    /// person dropped from the drive must not stay believed on this Mac.
    #[test]
    fn a_pin_survives_a_reopen_and_a_repin_replaces_its_readers() {
        let dir = temp_dir();
        assert!(pins(&dir).expect("empty").is_empty());

        let first = pin(&["@tgorka:example.org", "@marta:example.org"]);
        set_pin(&dir, "p1", &first, 1).expect("pin");
        assert_eq!(pins(&dir).expect("read").get("p1"), Some(&first));

        let second = pin(&["@tgorka:example.org"]);
        set_pin(&dir, "p1", &second, 2).expect("repin");
        let read = pins(&dir).expect("read again");
        assert_eq!(read.len(), 1);
        assert_eq!(read.get("p1"), Some(&second));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
