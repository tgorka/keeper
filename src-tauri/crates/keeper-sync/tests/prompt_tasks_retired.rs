//! The upgrade a `sync.db` holding scheduled prompt tasks — the kind the ⌘9
//! bots surface ran — takes on its next open (Story 108.3a, UX-DR-E108-3).
//!
//! The store here is the one an older build left: the `bot` kind's three
//! columns on `tasks`, two prompt tasks with run history beside a sync task.
//! One open later the prompt tasks and their runs are gone, each named once in
//! a WARN line, the sync task and its runs untouched, and the Tasks list
//! carries the names for its one notice until a person dismisses it.

use std::io::Write;
use std::sync::{Arc, Mutex};

use keeper_sync::db;

/// The log lines written while the closure ran.
fn captured<T>(run: impl FnOnce() -> T) -> (T, String) {
    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);
    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("buffer").extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let buffer = Buffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let out = tracing::subscriber::with_default(subscriber, run);
    let text = String::from_utf8(buffer.0.lock().expect("buffer").clone()).expect("utf-8");
    (out, text)
}

fn task_ids(conn: &rusqlite::Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT id FROM tasks ORDER BY id")
        .expect("prepare");
    stmt.query_map([], |row| row.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("ids")
}

fn run_task_ids(conn: &rusqlite::Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT task_id FROM task_runs ORDER BY id")
        .expect("prepare");
    stmt.query_map([], |row| row.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("ids")
}

#[test]
fn the_prompt_tasks_are_removed_once_and_named_until_dismissed() {
    let dir = tempfile::tempdir().expect("tempdir");
    {
        let conn = db::open(dir.path()).expect("open");
        conn.execute_batch(
            "ALTER TABLE tasks ADD COLUMN bot_id TEXT;
             ALTER TABLE tasks ADD COLUMN prompt_subpath TEXT;
             ALTER TABLE tasks ADD COLUMN model TEXT;
             INSERT INTO tasks (id, profile_id, kind, mode, updated_ms)
                 VALUES ('sync-1', '01DRIVE', 'sync', 'manual', 1);
             INSERT INTO tasks (id, profile_id, kind, mode, updated_ms, description,
                     bot_id, prompt_subpath, model)
                 VALUES ('bot-1', '01DRIVE', 'bot', 'scheduled', 2, 'Morning digest',
                     '01BOT', 'prompts/digest.md', 'qwen3');
             INSERT INTO tasks (id, profile_id, kind, mode, updated_ms, description,
                     bot_id, prompt_subpath)
                 VALUES ('bot-2', '01DRIVE', 'bot', 'manual', 3, '   ', '01BOT', 'p.md');
             INSERT INTO task_runs (task_id, started_ms, host) VALUES ('bot-1', 10, 'app');
             INSERT INTO task_runs (task_id, started_ms, host) VALUES ('sync-1', 11, 'app');
             INSERT INTO task_runs (task_id, started_ms, host) VALUES ('bot-2', 12, 'app');",
        )
        .expect("an older build's rows");
    }

    let (conn, log) = captured(|| db::open(dir.path()).expect("reopen"));
    assert_eq!(task_ids(&conn), vec!["sync-1".to_owned()]);
    assert_eq!(run_task_ids(&conn), vec!["sync-1".to_owned()]);
    for id in ["bot-1", "bot-2"] {
        let lines: Vec<&str> = log
            .lines()
            .filter(|line| line.contains("WARN") && line.contains(&format!("task={id}")))
            .collect();
        assert_eq!(lines.len(), 1, "{id} is named once: {log}");
    }

    // A blank description names the task by its id.
    let listing = db::list_tasks(&conn).expect("listing");
    assert_eq!(
        listing.retired,
        vec!["Morning digest".to_owned(), "bot-2".to_owned()]
    );
    assert_eq!(listing.tasks.len(), 1);
    assert!(listing.unknown.is_empty());
    drop(conn);

    // Nothing left to remove: the next open names nothing twice.
    let (conn, log) = captured(|| db::open(dir.path()).expect("third open"));
    assert!(!log.contains("WARN"), "{log}");
    assert_eq!(
        db::retired_tasks(&conn).expect("retired"),
        vec!["Morning digest".to_owned(), "bot-2".to_owned()]
    );

    db::dismiss_retired_tasks(&conn).expect("dismiss");
    drop(conn);
    let conn = db::open(dir.path()).expect("fourth open");
    assert!(db::list_tasks(&conn).expect("listing").retired.is_empty());
}

#[test]
fn a_store_with_no_prompt_task_carries_no_notice() {
    let dir = tempfile::tempdir().expect("tempdir");
    let conn = db::open(dir.path()).expect("open");
    conn.execute(
        "INSERT INTO tasks (id, profile_id, kind, mode, updated_ms)
             VALUES ('sync-1', '01DRIVE', 'sync', 'manual', 1)",
        [],
    )
    .expect("row");
    drop(conn);
    let conn = db::open(dir.path()).expect("reopen");
    let listing = db::list_tasks(&conn).expect("listing");
    assert!(listing.retired.is_empty());
    assert_eq!(listing.tasks.len(), 1);
}
