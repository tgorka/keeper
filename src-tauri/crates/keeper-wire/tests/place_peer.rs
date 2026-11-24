//! 123.3-AC2 and 123.3-AC3: socket place, permissions, path length, stale socket, and peer uid check.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::ErrorKind;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use common::{data_dir, id, limits, serve, Counts, Fixture, FixtureDispatch, Raw};
use keeper_wire::{Hooks, Server};
use keeper_wire_api::client::Client;
use keeper_wire_api::place::{effective_uid, socket_path, SUN_PATH_BYTES};
use keeper_wire_api::DataLock;
use serde_json::json;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::UnixStream;

fn start_in(dir: &std::path::Path) -> Result<(Arc<DataLock>, Server), keeper_wire::ServeError> {
    let lock = Arc::new(DataLock::acquire(dir, "place_peer test").unwrap());
    let fixture = Fixture::new(1);
    let server = Server::start(
        Arc::clone(&lock),
        Arc::new(FixtureDispatch(fixture)),
        limits(),
        Hooks::default(),
    )?;
    Ok((lock, server))
}

fn make_data_dir_for_socket_len(root: &std::path::Path, target_socket_len: usize) -> PathBuf {
    let suffix_len = "/wire/wire.sock".len();
    let base_len = root.as_os_str().len();
    assert!(
        target_socket_len > base_len + suffix_len + 1,
        "tempdir path {} is too long for target socket length {target_socket_len}",
        root.display()
    );
    let component_len = target_socket_len - base_len - suffix_len - 1;
    let dir = root.join("a".repeat(component_len));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let sock = socket_path(&dir);
    assert_eq!(sock.as_os_str().len(), target_socket_len);
    dir
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn socket_lives_under_data_dir_with_0700_dir_and_0600_socket() {
    let dir = data_dir();
    let (_lock, server) = start_in(dir.path()).unwrap();

    let expected = dir.path().join("wire").join("wire.sock");
    assert_eq!(server.socket(), expected.as_path());

    let wire_meta = std::fs::symlink_metadata(dir.path().join("wire")).unwrap();
    assert!(wire_meta.is_dir());
    assert_eq!(wire_meta.mode() & 0o777, 0o700);

    let sock_meta = std::fs::symlink_metadata(&expected).unwrap();
    assert_eq!(sock_meta.mode() & 0o777, 0o600);

    let client = Client::connect(dir.path(), "t").await.unwrap();
    let reply = client
        .call("fx.echo", json!({"ok": true}), Default::default())
        .await
        .unwrap();
    assert_eq!(reply, json!({"ok": true}));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn preexisting_0755_wire_dir_is_narrowed_to_0700() {
    let dir = data_dir();
    let wire_dir = dir.path().join("wire");
    std::fs::create_dir(&wire_dir).unwrap();
    std::fs::set_permissions(&wire_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        std::fs::symlink_metadata(&wire_dir).unwrap().mode() & 0o777,
        0o755
    );

    let (_lock, _server) = start_in(dir.path()).unwrap();
    assert_eq!(
        std::fs::symlink_metadata(&wire_dir).unwrap().mode() & 0o777,
        0o700
    );

    let client = Client::connect(dir.path(), "t").await.unwrap();
    let reply = client
        .call("fx.echo", json!({"narrowed": 1}), Default::default())
        .await
        .unwrap();
    assert_eq!(reply, json!({"narrowed": 1}));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn symlink_wire_dir_is_refused_without_creating_socket() {
    let dir = data_dir();
    let target = data_dir();
    let wire_link = dir.path().join("wire");
    std::os::unix::fs::symlink(target.path(), &wire_link).unwrap();

    let err = start_in(dir.path())
        .err()
        .expect("symlink wire/ must be refused");
    assert_eq!(err.code(), "wire_dir_unsafe", "{err}");
    assert!(std::fs::symlink_metadata(socket_path(dir.path())).is_err());
    assert!(std::fs::symlink_metadata(target.path().join("wire.sock")).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn socket_path_length_boundary_refuses_sun_path_bytes_and_serves_one_less() {
    let root_long = tempfile::tempdir().unwrap();
    let long_dir = make_data_dir_for_socket_len(root_long.path(), SUN_PATH_BYTES);
    let err = start_in(&long_dir)
        .err()
        .expect("path of SUN_PATH_BYTES must be refused");
    assert_eq!(err.code(), "wire_path_too_long", "{err}");
    assert!(std::fs::symlink_metadata(socket_path(&long_dir)).is_err());
    assert!(std::fs::symlink_metadata(long_dir.join("wire")).is_err());

    let root_fit = tempfile::tempdir().unwrap();
    let fit_dir = make_data_dir_for_socket_len(root_fit.path(), SUN_PATH_BYTES - 1);
    let (_lock, server) = start_in(&fit_dir).unwrap();
    assert_eq!(server.socket(), socket_path(&fit_dir).as_path());

    let client = Client::connect(&fit_dir, "t").await.unwrap();
    let reply = client
        .call(
            "fx.echo",
            json!({"fit": SUN_PATH_BYTES - 1}),
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(reply, json!({"fit": SUN_PATH_BYTES - 1}));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn world_writable_data_dir_refuses_lock_without_creating_lock_file() {
    let dir = data_dir();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777)).unwrap();

    let err = DataLock::acquire(dir.path(), "place_peer test")
        .expect_err("0777 data dir must be refused");
    assert_eq!(err.code(), "data_dir_unsafe", "{err}");
    assert!(std::fs::symlink_metadata(dir.path().join("keeper.lock")).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn regular_file_at_socket_path_is_refused_and_preserved() {
    let dir = data_dir();
    let wire_dir = dir.path().join("wire");
    std::fs::create_dir(&wire_dir).unwrap();
    std::fs::set_permissions(&wire_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

    let sock = socket_path(dir.path());
    let sentinel = b"do-not-unlink-regular-file";
    std::fs::write(&sock, sentinel).unwrap();

    let err = start_in(dir.path())
        .err()
        .expect("regular file at socket path must be refused");
    assert_eq!(err.code(), "wire_path_occupied", "{err}");
    assert_eq!(std::fs::read(&sock).unwrap(), sentinel);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stale_socket_at_path_is_cleared_and_served() {
    let dir = data_dir();
    let wire_dir = dir.path().join("wire");
    std::fs::create_dir(&wire_dir).unwrap();
    std::fs::set_permissions(&wire_dir, std::fs::Permissions::from_mode(0o700)).unwrap();

    let sock = socket_path(dir.path());
    let stale = StdUnixListener::bind(&sock).unwrap();
    drop(stale);
    assert!(std::fs::symlink_metadata(&sock).is_ok());

    let (_lock, _server) = start_in(dir.path()).unwrap();
    let client = Client::connect(dir.path(), "t").await.unwrap();
    let reply = client
        .call("fx.echo", json!({"stale": "cleared"}), Default::default())
        .await
        .unwrap();
    assert_eq!(reply, json!({"stale": "cleared"}));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn foreign_uid_peer_is_dropped_unread_and_unwritten_interleaved_1000_times() {
    let foreign = serve(
        limits(),
        Hooks {
            expect_uid: Some(effective_uid().saturating_add(4242)),
            ..Hooks::default()
        },
    );
    let normal = serve(limits(), Hooks::default());

    // First verify a silent foreign peer observes clean EOF with 0 bytes written.
    {
        let mut raw = Raw::connect(&foreign.socket()).await;
        let bytes = raw.drain_bytes().await;
        assert!(
            bytes.is_empty(),
            "expected 0 bytes from foreign server, got {bytes:?}"
        );
    }

    let hello_line =
        b"{\"type\":\"hello\",\"wire\":[\"keeper.wire.v1\"],\"client\":\"foreign/1\"}\n";
    for i in 0..1000usize {
        let mut stream = UnixStream::connect(&foreign.socket()).await.unwrap();
        let call_id = id();
        let mut payload = Vec::from(hello_line.as_slice());
        payload.extend_from_slice(
            serde_json::to_vec(&json!({
                "type": "call",
                "call_id": call_id,
                "operation": "fx.echo",
                "input": {"i": i}
            }))
            .unwrap()
            .as_slice(),
        );
        payload.push(b'\n');
        // Server may close before our write lands; BrokenPipe / ConnectionReset is expected.
        let _ = stream.write_all(&payload).await;

        let mut bytes = Vec::new();
        let read_res = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut bytes))
            .await
            .expect("foreign connection must close within 5s");
        match read_res {
            Ok(0) => {}
            Ok(_) => panic!("foreign server wrote bytes on iter {i}: {bytes:?}"),
            Err(e) if e.kind() == ErrorKind::ConnectionReset => {}
            Err(e) => panic!("unexpected read error on iter {i}: {e}"),
        }
        assert!(
            bytes.is_empty(),
            "foreign server wrote bytes on iter {i}: {bytes:?}"
        );

        let mut ok = normal.raw().await;
        let ok_id = id();
        ok.call(&ok_id, "fx.echo", json!({"i": i})).await;
        let reply = ok.terminal(&ok_id, &mut Vec::new()).await;
        assert_eq!(reply["type"], "reply");
        assert_eq!(reply["output"], json!({"i": i}));
    }

    assert_eq!(Counts::get(&foreign.fixture.counts.calls), 0);
    assert_eq!(Counts::get(&normal.fixture.counts.calls), 1000);
}
