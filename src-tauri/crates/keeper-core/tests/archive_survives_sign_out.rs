//! Integration coverage for Story 5.7 (archive survives sign-out).
//!
//! The default keep-archive sign-out (`auth::sign_out_cleanup`) removes exactly
//! the account's SDK store dir, Keychain session entry, and `keeper.db` registry
//! row — and NEVER `archive.db`. This test archives an account's history through
//! the single writer, runs `sign_out_cleanup`, and asserts the persisted-session
//! targets are gone while `archive.db` + its rows survive and `archive::search`
//! still returns them with no active session (FR-37, FR-6).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use keeper_core::archive::db::{event_count, open_archive_db, open_readonly_archive_db};
use keeper_core::archive::{search, ArchiveEvent, ArchiveWriter, SearchFilter};
use keeper_core::auth::{session_keychain_key, sign_out_cleanup};
use keeper_core::error::CoreError;
use keeper_core::platform::Platform;
use keeper_core::registry;
use rusqlite::Connection;

/// A unique temp data dir per test run.
fn temp_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "keeper-archive-survive-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    dir
}

/// A plain message row (no relation).
fn text_event(account_id: &str, event_id: &str, body: &str) -> ArchiveEvent {
    ArchiveEvent {
        account_id: account_id.to_owned(),
        event_id: event_id.to_owned(),
        room_id: "!room:example.org".to_owned(),
        sender: "@bob:example.org".to_owned(),
        origin_ts: 1_720_000_000_000,
        event_type: "m.room.message".to_owned(),
        content_json: format!(r#"{{"msgtype":"m.text","body":"{body}"}}"#),
        body: body.to_owned(),
        media: None,
        relates_to_event_id: None,
        rel_type: None,
    }
}

/// Poll a read connection until `pred` holds or the deadline elapses (the writer
/// drains its channel asynchronously).
fn wait_until(dir: &Path, mut pred: impl FnMut(&Connection) -> bool) {
    for _ in 0..100 {
        let conn = open_archive_db(dir).expect("open for poll");
        if pred(&conn) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let conn = open_archive_db(dir).expect("open final poll");
    assert!(pred(&conn), "condition not reached before deadline");
}

/// A local platform with a fixed data dir and an in-memory keychain map, mirroring
/// the auth-test pattern, so `sign_out_cleanup` can drive the registry row +
/// keychain entry together against a real on-disk data dir.
struct LocalPlatform {
    data_dir: PathBuf,
    keychain: Mutex<std::collections::HashMap<String, String>>,
}

impl Platform for LocalPlatform {
    fn data_dir(&self) -> Result<PathBuf, CoreError> {
        Ok(self.data_dir.clone())
    }
    fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError> {
        self.keychain
            .lock()
            .expect("lock poisoned")
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }
    fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError> {
        Ok(self
            .keychain
            .lock()
            .expect("lock poisoned")
            .get(key)
            .cloned())
    }
    fn keychain_delete(&self, key: &str) -> Result<(), CoreError> {
        self.keychain.lock().expect("lock poisoned").remove(key);
        Ok(())
    }
    fn open_url(&self, _url: &str) -> Result<(), CoreError> {
        Ok(())
    }
    fn notify(
        &self,
        _title: &str,
        _body: &str,
        _target: &keeper_core::vm::NotifyTarget,
    ) -> Result<(), CoreError> {
        Ok(())
    }
    fn sidecar_path(&self, _name: &str) -> Result<PathBuf, CoreError> {
        Err(CoreError::Unsupported("sidecar unused in tests".to_owned()))
    }
    fn set_badge_count(&self, _count: Option<u32>) -> Result<(), CoreError> {
        Ok(())
    }
}

#[test]
fn archive_survives_default_sign_out() {
    let dir = temp_dir("default");
    let account_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    // Archive some history through the single writer.
    let handle = ArchiveWriter::spawn(&dir).expect("spawn writer");
    handle.ingest(text_event(account_id, "$e1", "delectable survivor text"));
    handle.ingest(text_event(account_id, "$e2", "another kept message"));
    drop(handle);
    wait_until(&dir, |conn| event_count(conn, account_id).unwrap_or(0) == 2);

    // Stand up the persisted-session targets `sign_out_cleanup` removes: a registry
    // row + a Keychain session entry, plus a fake SDK store dir with a file in it.
    let platform = LocalPlatform {
        data_dir: dir.clone(),
        keychain: Mutex::new(std::collections::HashMap::new()),
    };
    registry::insert_account(
        &dir,
        account_id,
        "@bob:example.org",
        "https://matrix.example.org/",
        "DEVID",
        1,
        3,
        "password",
    )
    .expect("insert registry row");
    platform
        .keychain_set(&session_keychain_key(account_id), "opaque-session-json")
        .expect("set session");
    let sdk_dir = dir.join("accounts").join(account_id).join("sdk");
    std::fs::create_dir_all(&sdk_dir).expect("create sdk dir");
    std::fs::write(sdk_dir.join("matrix.db"), b"x").expect("write sdk file");

    // Default sign-out: removes SDK dir + keychain session + registry row only.
    sign_out_cleanup(&platform, account_id).expect("sign out cleanup");

    // The persisted-session targets are gone.
    assert!(!sdk_dir.exists(), "SDK store dir removed");
    assert_eq!(
        platform
            .keychain_get(&session_keychain_key(account_id))
            .expect("get session"),
        None,
        "keychain session removed"
    );
    assert!(
        registry::get_account(&dir, account_id)
            .expect("get registry")
            .is_none(),
        "registry row removed"
    );

    // But `archive.db` and its rows survive — and search still returns them with no
    // active session (a fresh read-only connection, exactly as the IPC path uses).
    let archive_file = dir.join("archive.db");
    assert!(archive_file.exists(), "archive.db survives sign-out");
    let conn = open_readonly_archive_db(&dir).expect("open archive read-only");
    assert_eq!(
        event_count(&conn, account_id).expect("count survives"),
        2,
        "archived rows survive sign-out"
    );
    let hits = search(
        &conn,
        &SearchFilter {
            query: "survivor".to_owned(),
            ..Default::default()
        },
        false,
    )
    .expect("search survives");
    assert_eq!(
        hits.len(),
        1,
        "search returns the surviving row post-sign-out"
    );
    assert_eq!(hits[0].event_id, "$e1");
    drop(conn);
    let _ = std::fs::remove_dir_all(&dir);
}
