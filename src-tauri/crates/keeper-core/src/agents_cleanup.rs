//! What an install upgraded from a build with the agents' Matrix path leaves
//! behind, removed at launch (story 108.2, FR-863, FR-864).
//!
//! Before 108.2 keeper on a Mac could host the person's agents ("This Mac as
//! a host"): each agent copy signed in to Matrix with its own session and
//! store, the Mac kept its MCP servers, sandbox paths, web search engine and
//! pinned drives in `keeper.db`, every device kept the agent rooms' seats and
//! the Agents list's last rows there, and *Speak to* could name an agent's
//! Matrix room. None of it is read any more. [`run`] removes it:
//!
//! - the `keeper.db` tables [`TABLES`], dropped (`DROP TABLE IF EXISTS`,
//!   the precedent of `agents::index`'s schema rebuild) — after the MCP
//!   servers' bearer tokens they name are taken out of the keychain, so no
//!   secret outlives the row that named it;
//! - every agent copy's keychain items (`agents/<user>/session`,
//!   `agents/<user>/sdk-passphrase`) and its store, `<data_dir>/agents/`;
//! - the setting `agents.claims_lease`;
//! - a *Speak to* choice that names an old agent room, cleared with a
//!   one-time notice (UX-DR-E108-1).
//!
//! Every step is idempotent, so it runs on every launch and does nothing
//! once there is nothing left; each step's failure is a WARN line (R381)
//! and the others still run. Nothing here touches a drive: a drive's own
//! `.keeper/agents.db` index and its session files are the drive's (FR-862).
//! The phone's Matrix pusher is removed by its own path
//! (`AccountManager::push_remove`, DW-E108-1).

use std::path::Path;

use rusqlite::Connection;

use crate::error::CoreError;
use crate::platform::Platform;
use crate::registry;

/// The `keeper.db` tables only the agents' Matrix path wrote.
pub const TABLES: [&str; 12] = [
    "agent_mcp_servers",
    "agent_mcp_server_argv",
    "agent_mcp_server_readers",
    "agent_mcp_server_tiers",
    "agent_sandbox_read_exec",
    "agent_sandbox_env",
    "agent_web_search",
    "agent_mac_revision",
    "agent_pins",
    "agent_pin_readers",
    "agent_room_seats",
    "agent_list_rows",
];

/// What one launch's cleanup removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cleaned {
    /// The tables of [`TABLES`] that were there and are gone.
    pub tables: Vec<String>,
    /// Keychain items removed: the copies' sessions and passphrases, and
    /// the MCP servers' tokens.
    pub keychain_items: usize,
    /// Whether `<data_dir>/agents/` was there and is gone.
    pub stores: bool,
    /// Whether `agents.claims_lease` was set and is gone.
    pub claims_lease: bool,
    /// Whether *Speak to* named an old agent room and was cleared.
    pub voice_target: bool,
}

/// Remove everything the agents' Matrix path left in `data_dir` and in
/// `platform`'s keychain. Never fails: each step that cannot finish is
/// logged and tried again at the next launch.
pub fn run(platform: &dyn Platform, data_dir: &Path) -> Cleaned {
    let mut cleaned = Cleaned::default();
    let warn = |what: &str, error: &dyn std::fmt::Display| {
        tracing::warn!(%error, "agents cleanup: {what}");
    };
    match drop_tables(platform, data_dir) {
        Ok((tables, tokens)) => {
            cleaned.tables = tables;
            cleaned.keychain_items += tokens;
        }
        Err(error) => warn("the old agent tables could not be dropped", &error),
    }
    match remove_copies(platform, data_dir) {
        Ok((items, stores)) => {
            cleaned.keychain_items += items;
            cleaned.stores = stores;
        }
        Err(error) => warn(
            "the agent copies' sessions and stores could not be removed",
            &error,
        ),
    }
    match registry::remove_agents_claims_lease(data_dir) {
        Ok(removed) => cleaned.claims_lease = removed,
        Err(error) => warn("agents.claims_lease could not be removed", &error),
    }
    match registry::clear_old_voice_target(data_dir) {
        Ok(cleared) => cleaned.voice_target = cleared,
        Err(error) => warn("an old Speak to choice could not be cleared", &error),
    }
    if cleaned != Cleaned::default() {
        tracing::info!(
            tables = cleaned.tables.len(),
            keychain_items = cleaned.keychain_items,
            stores = cleaned.stores,
            claims_lease = cleaned.claims_lease,
            voice_target = cleaned.voice_target,
            "agents cleanup: what the agents' Matrix path left is removed"
        );
    }
    cleaned
}

fn sqlite(what: &'static str) -> impl Fn(rusqlite::Error) -> CoreError {
    move |error| CoreError::Internal(format!("{what}: {error}"))
}

/// Drop [`TABLES`] from `keeper.db`, after taking the MCP servers' tokens
/// out of the keychain; the tables that were there, and how many tokens
/// went. A `keeper.db` that does not exist yet holds nothing.
fn drop_tables(
    platform: &dyn Platform,
    data_dir: &Path,
) -> Result<(Vec<String>, usize), CoreError> {
    let path = data_dir.join("keeper.db");
    if !path.exists() {
        return Ok((Vec::new(), 0));
    }
    let conn = Connection::open(&path).map_err(sqlite("could not open keeper.db"))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(sqlite("could not set a busy timeout"))?;
    let present = |name: &str| -> Result<bool, CoreError> {
        conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [name],
            |row| row.get(0),
        )
        .map_err(sqlite("could not read keeper.db's tables"))
    };
    let mut tokens = 0;
    if present("agent_mcp_servers")? {
        let has_ref: bool = conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM pragma_table_info('agent_mcp_servers') \
                 WHERE name = 'token_ref')",
                [],
                |row| row.get(0),
            )
            .map_err(sqlite("could not read agent_mcp_servers' columns"))?;
        let sql = if has_ref {
            "SELECT name, token_ref FROM agent_mcp_servers"
        } else {
            "SELECT name, NULL FROM agent_mcp_servers"
        };
        let mut statement = conn
            .prepare(sql)
            .map_err(sqlite("could not read agent_mcp_servers"))?;
        let rows: Vec<(String, Option<String>)> = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(sqlite("could not read agent_mcp_servers"))?
            .collect::<Result<_, _>>()
            .map_err(sqlite("could not read agent_mcp_servers"))?;
        for (name, generation) in rows {
            let keys = std::iter::once(format!("agent_mcp_token/{name}"))
                .chain(generation.map(|generation| format!("agent_mcp_bearer/{generation}")));
            for key in keys {
                if platform.keychain_get(&key)?.is_some() {
                    platform.keychain_delete(&key)?;
                    tokens += 1;
                }
            }
        }
    }
    let mut dropped = Vec::new();
    for table in TABLES {
        if present(table)? {
            conn.execute_batch(&format!("DROP TABLE IF EXISTS {table};"))
                .map_err(sqlite("could not drop an old agent table"))?;
            dropped.push(table.to_owned());
        }
    }
    Ok((dropped, tokens))
}

/// Remove every agent copy's keychain items and `<data_dir>/agents/`, which
/// holds one store per copy, named for its Matrix user; how many items went
/// and whether the folder was there.
fn remove_copies(platform: &dyn Platform, data_dir: &Path) -> Result<(usize, bool), CoreError> {
    let root = data_dir.join("agents");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, false)),
        Err(error) => {
            return Err(CoreError::Internal(format!(
                "could not read {}: {error}",
                root.display()
            )))
        }
    };
    let mut items = 0;
    for entry in entries {
        let entry = entry.map_err(|error| CoreError::Internal(error.to_string()))?;
        let Some(user) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !user.starts_with('@') {
            continue;
        }
        for key in [
            format!("agents/{user}/session"),
            format!("agents/{user}/sdk-passphrase"),
        ] {
            if platform.keychain_get(&key)?.is_some() {
                platform.keychain_delete(&key)?;
                items += 1;
            }
        }
    }
    std::fs::remove_dir_all(&root).map_err(|error| {
        CoreError::Internal(format!("could not remove {}: {error}", root.display()))
    })?;
    Ok((items, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forges::testing::FakePlatform;

    const NIXI: &str = "@nixi:example.org";

    /// A `keeper.db` as a 0.10.x Mac left it: every table only the agents'
    /// Matrix path wrote, with rows, beside tables keeper still reads.
    fn upgraded(data_dir: &Path) {
        registry::set_setting(data_dir, "agents.claims_lease", "quarter-hour").expect("lease");
        registry::set_setting(data_dir, "notify.dnd", "1").expect("a setting that stays");
        let conn = Connection::open(data_dir.join("keeper.db")).expect("open");
        conn.execute_batch(
            "CREATE TABLE agent_mcp_servers(name TEXT PRIMARY KEY, url TEXT, role TEXT, \
                fingerprint TEXT, trust_annotations INTEGER NOT NULL, token INTEGER NOT NULL, \
                token_ref TEXT);
             INSERT INTO agent_mcp_servers VALUES ('notes', 'https://n.example.org/mcp', NULL, \
                NULL, 0, 1, 'g1');
             CREATE TABLE agent_mcp_server_argv(name TEXT, position INTEGER, arg TEXT);
             CREATE TABLE agent_mcp_server_readers(name TEXT, reader TEXT);
             CREATE TABLE agent_mcp_server_tiers(name TEXT, tool TEXT, tier TEXT);
             CREATE TABLE agent_sandbox_read_exec(position INTEGER PRIMARY KEY, path TEXT);
             CREATE TABLE agent_sandbox_env(name TEXT PRIMARY KEY, path TEXT);
             CREATE TABLE agent_web_search(id INTEGER PRIMARY KEY, url TEXT);
             INSERT INTO agent_web_search VALUES (1, 'http://127.0.0.1:5191');
             CREATE TABLE agent_mac_revision(id INTEGER PRIMARY KEY, revision INTEGER);
             CREATE TABLE agent_pins(drive TEXT PRIMARY KEY, agent TEXT);
             INSERT INTO agent_pins VALUES ('tgdrive', 'nixi');
             CREATE TABLE agent_pin_readers(drive TEXT, reader TEXT);
             CREATE TABLE agent_room_seats(account TEXT, room_id TEXT, agent TEXT);
             CREATE TABLE agent_list_rows(account TEXT, room_id TEXT, display_name TEXT);
             CREATE TABLE bot_providers(id TEXT PRIMARY KEY, name TEXT);
             INSERT INTO bot_providers VALUES ('p1', 'Hermes');",
        )
        .expect("the old tables");
    }

    fn tables(data_dir: &Path) -> Vec<String> {
        let conn = Connection::open(data_dir.join("keeper.db")).expect("open");
        let mut statement = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .expect("prepare");
        statement
            .query_map([], |row| row.get(0))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("names")
    }

    /// 108.2 acceptance 2: a `keeper.db` holding every listed table loses
    /// exactly those; every other table and its rows, and every other
    /// setting, are intact. The MCP server's token, the copy's session and
    /// passphrase and its store go with them; the claims lease setting too.
    /// A second launch finds nothing left to do.
    #[test]
    fn an_upgraded_install_loses_the_agents_matrix_state_and_nothing_else() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = FakePlatform::default();
        upgraded(dir.path());
        let before = tables(dir.path());
        for table in TABLES {
            assert!(before.iter().any(|t| t == table), "{table} seeded");
        }
        platform
            .keychain_set("agent_mcp_bearer/g1", "token")
            .expect("the MCP token");
        platform
            .keychain_set("agent_mcp_token/notes", "legacy")
            .expect("an earlier keeper's token");
        platform
            .keychain_set(&format!("agents/{NIXI}/session"), "{}")
            .expect("the copy's session");
        platform
            .keychain_set(&format!("agents/{NIXI}/sdk-passphrase"), "pass")
            .expect("its passphrase");
        platform
            .keychain_set("session/01ACCOUNT", "{}")
            .expect("the person's own account");
        let store = dir.path().join("agents").join(NIXI).join("sdk");
        std::fs::create_dir_all(&store).expect("the copy's store");
        std::fs::write(store.join("matrix-sdk-state.sqlite3"), b"x").expect("a file in it");

        let cleaned = run(&platform, dir.path());

        let mut dropped = cleaned.tables.clone();
        dropped.sort();
        let mut listed: Vec<String> = TABLES.iter().map(|t| (*t).to_owned()).collect();
        listed.sort();
        assert_eq!(dropped, listed);
        let after = tables(dir.path());
        assert_eq!(
            after,
            before
                .iter()
                .filter(|table| !TABLES.contains(&table.as_str()))
                .cloned()
                .collect::<Vec<_>>(),
            "every other table stays"
        );
        let conn = Connection::open(dir.path().join("keeper.db")).expect("open");
        let providers: i64 = conn
            .query_row("SELECT count(*) FROM bot_providers", [], |row| row.get(0))
            .expect("count");
        assert_eq!(providers, 1, "the rows of a kept table stay");
        assert_eq!(cleaned.keychain_items, 4);
        for gone in [
            "agent_mcp_bearer/g1".to_owned(),
            "agent_mcp_token/notes".to_owned(),
            format!("agents/{NIXI}/session"),
            format!("agents/{NIXI}/sdk-passphrase"),
        ] {
            assert_eq!(platform.keychain_get(&gone).expect("read"), None, "{gone}");
        }
        assert_eq!(
            platform.keychain_get("session/01ACCOUNT").expect("read"),
            Some("{}".to_owned()),
            "the person's own account is not the agents'"
        );
        assert!(cleaned.stores);
        assert!(!dir.path().join("agents").exists());
        assert!(cleaned.claims_lease);
        assert_eq!(
            registry::stored_settings(dir.path(), &["agents.claims_lease", "notify.dnd"])
                .expect("read"),
            [("notify.dnd".to_owned(), "1".to_owned())]
                .into_iter()
                .collect()
        );

        assert_eq!(
            run(&platform, dir.path()),
            Cleaned::default(),
            "nothing left"
        );
    }

    /// UX-DR-E108-1: a *Speak to* choice naming an old agent room is cleared
    /// and the notice latched once; an agent or a bot is left alone.
    #[test]
    fn an_old_speak_to_room_is_cleared_once_with_a_notice() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = FakePlatform::default();
        registry::set_voice_target(dir.path(), Some("agent:!dm:example.org"))
            .expect("an old choice");
        assert!(run(&platform, dir.path()).voice_target);
        assert_eq!(registry::get_voice_target(dir.path()).expect("read"), None);
        assert_eq!(
            registry::get_voice_old_target_notice(dir.path()).expect("read"),
            Some(crate::hq::spoken::OLD_ROOM_TARGET_NOTICE.to_owned())
        );
        registry::dismiss_voice_old_target_notice(dir.path()).expect("dismissed");
        assert_eq!(
            registry::get_voice_old_target_notice(dir.path()).expect("read"),
            None
        );

        for kept in ["agent:nixie", "01JBOT"] {
            registry::set_voice_target(dir.path(), Some(kept)).expect("a choice");
            assert!(!run(&platform, dir.path()).voice_target, "{kept}");
            assert_eq!(
                registry::get_voice_target(dir.path())
                    .expect("read")
                    .as_deref(),
                Some(kept)
            );
        }
        assert_eq!(
            registry::get_voice_old_target_notice(dir.path()).expect("read"),
            None
        );
    }

    /// A fresh install holds none of it: nothing happens, and no
    /// `keeper.db` is made.
    #[test]
    fn a_fresh_install_has_nothing_to_remove() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = FakePlatform::default();
        let cleaned = drop_tables(&platform, dir.path()).expect("nothing");
        assert_eq!(cleaned, (Vec::new(), 0));
        assert!(!dir.path().join("keeper.db").exists());
        assert_eq!(
            remove_copies(&platform, dir.path()).expect("nothing"),
            (0, false)
        );
    }
}
