//! The upgrade a `keeper.db` written by a build with the ⌘9 bots surface takes
//! on its first open by this one (Story 108.3a, FR-866, R-HQ-12).
//!
//! The database here is built with the schema those builds wrote, every table
//! holding a row, and the bots' settings keys set. One open later: every table
//! the surface kept is gone, `bot_providers` and its row are still there for
//! notes search, the voice settings live under `voice.*`, and the retired
//! setting is gone. The drop happens once — the agent runtime still in keeper
//! may create some of those tables again, and a later open leaves them alone.

use std::path::Path;

use keeper_core::providers::store::list_providers;
use keeper_core::registry;
use rusqlite::Connection;

/// The `keeper.db` tables and indexes the bots surface created, as builds
/// before Story 108.3a wrote them.
const LEGACY_SCHEMA: &str = "
CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE bot_providers(
    id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL, base_url TEXT NOT NULL,
    created_ms INTEGER NOT NULL, health_state TEXT NOT NULL, health_checked_ms INTEGER,
    health_detail TEXT, read_timeout_ms INTEGER);
CREATE TABLE bots(
    id TEXT PRIMARY KEY, provider_id TEXT NOT NULL REFERENCES bot_providers(id),
    target TEXT NOT NULL, name TEXT NOT NULL, pin_order INTEGER NOT NULL,
    identity_shape TEXT, identity_colour TEXT, identity_mark TEXT,
    created_ms INTEGER NOT NULL, UNIQUE(provider_id, target));
CREATE TABLE bot_grants(
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL REFERENCES bot_providers(id) ON DELETE CASCADE,
    bot_id TEXT REFERENCES bots(id) ON DELETE CASCADE, scope_kind TEXT NOT NULL,
    profile_id TEXT, subtree TEXT, mode TEXT NOT NULL, created_ms INTEGER NOT NULL,
    revoked_ms INTEGER);
CREATE INDEX bot_grants_provider ON bot_grants(provider_id, revoked_ms);
CREATE TABLE bot_sessions(
    id TEXT PRIMARY KEY, bot_id TEXT NOT NULL, provider_id TEXT NOT NULL,
    title TEXT NOT NULL, created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0, remote_session_id TEXT);
CREATE TABLE bot_messages(
    id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES bot_sessions(id),
    seq INTEGER NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL, model TEXT,
    provider_id TEXT, prompt_tokens INTEGER, completion_tokens INTEGER,
    total_tokens INTEGER, ttft_ms INTEGER, duration_ms INTEGER, finish_reason TEXT,
    request_id TEXT, tool_call_count INTEGER NOT NULL DEFAULT 0,
    partial INTEGER NOT NULL DEFAULT 0, created_ms INTEGER NOT NULL);
CREATE INDEX bot_messages_in_order ON bot_messages (session_id, seq);
CREATE INDEX bot_sessions_by_activity ON bot_sessions (updated_ms DESC, id DESC);
CREATE INDEX bot_messages_by_time ON bot_messages (session_id, created_ms);
CREATE TABLE bot_dismissed_remote_sessions(
    provider_id TEXT NOT NULL, remote_session_id TEXT NOT NULL,
    dismissed_ms INTEGER NOT NULL, PRIMARY KEY(provider_id, remote_session_id));
CREATE TABLE bot_audit(
    id INTEGER PRIMARY KEY AUTOINCREMENT, started_ms INTEGER NOT NULL,
    finished_ms INTEGER, provider_id TEXT NOT NULL, bot_id TEXT,
    session_id TEXT NOT NULL, message_id TEXT, tool TEXT NOT NULL,
    profile_id TEXT NOT NULL, subpath TEXT NOT NULL, display_path TEXT NOT NULL,
    effect TEXT NOT NULL, verdict TEXT NOT NULL, reason TEXT, grant_id TEXT,
    outcome TEXT NOT NULL, bytes INTEGER, truncated INTEGER NOT NULL);
CREATE INDEX bot_audit_session ON bot_audit(session_id);

INSERT INTO bot_providers VALUES
    ('p1', 'ollama', 'Studio', 'http://studio.local:11434', 1, 'unknown', NULL, NULL, NULL);
INSERT INTO bots VALUES ('b1', 'p1', 'qwen3', 'Qwen', 0, NULL, NULL, NULL, 2);
INSERT INTO bot_grants VALUES ('g1', 'p1', 'b1', 'profile', 'drive', NULL, 'read', 3, NULL);
INSERT INTO bot_sessions VALUES ('s1', 'b1', 'p1', 'Hello', 4, 5, 0, NULL);
INSERT INTO bot_messages (id, session_id, seq, role, content, created_ms)
    VALUES ('m1', 's1', 0, 'user', 'hi', 6);
INSERT INTO bot_dismissed_remote_sessions VALUES ('p1', 'r1', 7);
INSERT INTO bot_audit (started_ms, provider_id, session_id, tool, profile_id, subpath,
    display_path, effect, verdict, outcome, truncated)
    VALUES (8, 'p1', 's1', 'read_file', 'drive', 'a.md', 'drive/a.md', 'read', 'allow', 'ok', 0);
";

/// A `keeper.db` as a build with the bots surface left it, with `settings`
/// holding `pairs`.
fn legacy_database(dir: &Path, pairs: &[(&str, &str)]) {
    let conn = Connection::open(dir.join("keeper.db")).expect("open");
    conn.execute_batch(LEGACY_SCHEMA).expect("legacy schema");
    for (key, value) in pairs {
        conn.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)",
            [key, value],
        )
        .expect("setting");
    }
}

/// Every table and index name in `keeper.db` starting with `bot`.
fn bot_objects(dir: &Path) -> Vec<String> {
    let conn = Connection::open(dir.join("keeper.db")).expect("open");
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE name LIKE 'bot%' ORDER BY name")
        .expect("prepare");
    stmt.query_map([], |row| row.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("names")
}

/// Every settings row, sorted by key.
fn settings(dir: &Path) -> Vec<(String, String)> {
    let conn = Connection::open(dir.join("keeper.db")).expect("open");
    let mut stmt = conn
        .prepare("SELECT key, value FROM settings ORDER BY key")
        .expect("prepare");
    stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows")
}

fn owned(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn the_first_open_drops_the_bots_keeps_the_provider_and_moves_the_voice_keys() {
    let dir = tempfile::tempdir().expect("tempdir");
    legacy_database(
        dir.path(),
        &[
            ("bots.wake_enabled", "1"),
            ("bots.wake_phrase", "nixie"),
            // Set under the new key already (a newer build ran here once):
            // that value wins over the old one.
            ("voice.wake_phrase", "computer"),
            ("bots.voice_locale", "pl-PL"),
            ("bots.stop_phrase", "enough"),
            ("bots.voice_target", "agent:nixie"),
            ("bots.message_details", "1"),
            // Kept under its name (DW-E108-6).
            ("bots.provider_credential_source.p1", "@tg:example.org"),
            ("notify.previews_enabled", "0"),
        ],
    );

    // Any read opens the database; the voice target is the one voice asks.
    assert_eq!(
        registry::get_voice_target(dir.path())
            .expect("read")
            .as_deref(),
        Some("agent:nixie")
    );

    assert_eq!(bot_objects(dir.path()), vec!["bot_providers".to_owned()]);
    let providers = list_providers(dir.path()).expect("providers");
    assert_eq!(providers.rows.len(), 1);
    assert_eq!(providers.rows[0].provider.id, "p1");
    assert_eq!(providers.rows[0].provider.name, "Studio");
    assert_eq!(
        providers.rows[0].provider.base_url,
        "http://studio.local:11434"
    );

    assert_eq!(
        settings(dir.path()),
        owned(&[
            ("bots.provider_credential_source.p1", "@tg:example.org"),
            ("notify.previews_enabled", "0"),
            ("upgrade.bots_retired", "1"),
            ("voice.locale", "pl-PL"),
            ("voice.stop_phrase", "enough"),
            ("voice.target", "agent:nixie"),
            ("voice.wake_enabled", "1"),
            ("voice.wake_phrase", "computer"),
        ])
    );
    assert!(registry::get_voice_wake_enabled(dir.path()).expect("wake"));
    assert_eq!(
        registry::get_voice_stop_phrase(dir.path()).expect("stop"),
        "enough"
    );
}

#[test]
fn a_pinned_bot_as_the_voice_target_becomes_no_choice() {
    let dir = tempfile::tempdir().expect("tempdir");
    legacy_database(dir.path(), &[("bots.voice_target", "b1")]);
    assert_eq!(registry::get_voice_target(dir.path()).expect("read"), None);
    assert!(settings(dir.path())
        .iter()
        .all(|(key, _)| !key.ends_with("voice_target") && key != "voice.target"));
}

/// A *Speak to* choice of one of the agents' old Matrix rooms (before 108.2)
/// moves to `voice.target` like any agent choice, so the launch's cleanup
/// (`agents_cleanup` → `registry::clear_old_voice_target`) still finds it,
/// clears it and says so once.
#[test]
fn an_old_room_voice_target_moves_and_is_then_cleared_with_its_notice() {
    let dir = tempfile::tempdir().expect("tempdir");
    legacy_database(
        dir.path(),
        &[("bots.voice_target", "agent:!dm:example.org")],
    );
    assert_eq!(
        registry::get_voice_target(dir.path())
            .expect("read")
            .as_deref(),
        Some("agent:!dm:example.org")
    );
    assert!(registry::clear_old_voice_target(dir.path()).expect("cleared"));
    assert_eq!(registry::get_voice_target(dir.path()).expect("read"), None);
    assert_eq!(
        registry::get_voice_old_target_notice(dir.path()).expect("notice"),
        Some(keeper_core::hq::spoken::OLD_ROOM_TARGET_NOTICE.to_owned())
    );
}

#[test]
fn the_drop_happens_once_and_a_table_made_again_afterwards_stays() {
    let dir = tempfile::tempdir().expect("tempdir");
    legacy_database(dir.path(), &[]);
    registry::get_voice_target(dir.path()).expect("first open");
    assert_eq!(bot_objects(dir.path()), vec!["bot_providers".to_owned()]);

    // The agent runtime still in keeper (until Story 108.3c) makes its own
    // session table again and writes to it.
    let conn = Connection::open(dir.path().join("keeper.db")).expect("open");
    conn.execute_batch(
        "CREATE TABLE bot_sessions(id TEXT PRIMARY KEY, title TEXT NOT NULL);
         INSERT INTO bot_sessions VALUES ('s2', 'Kept');",
    )
    .expect("runtime table");
    drop(conn);

    registry::get_voice_target(dir.path()).expect("second open");
    let conn = Connection::open(dir.path().join("keeper.db")).expect("open");
    let title: String = conn
        .query_row(
            "SELECT title FROM bot_sessions WHERE id = 's2'",
            [],
            |row| row.get(0),
        )
        .expect("the runtime's row is still there");
    assert_eq!(title, "Kept");
}

#[test]
fn a_fresh_install_opens_with_nothing_to_retire() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(registry::get_voice_target(dir.path()).expect("read"), None);
    assert!(bot_objects(dir.path()).is_empty());
    assert_eq!(
        settings(dir.path()),
        owned(&[("upgrade.bots_retired", "1")])
    );
}
