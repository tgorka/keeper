//! The self-update install path, against a real HTTP server.
//!
//! `is_newer` is unit-tested; what those tests cannot reach is the part that
//! touches the disk and the network — and that is the part where a mistake
//! leaves an unusable daemon on someone's machine. So these exercise
//! [`update::apply`] end to end: fetch the checksum, stream the payload, verify
//! it, and swap the file.
//!
//! The stub serves whatever bytes the test asks for, so the integrity failures
//! here are real mismatches rather than mocked-out ones.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;

// The daemon is a binary crate, so its modules are compiled into the test via
// `#[path]` rather than imported. `update` is self-contained apart from the
// shared error type, which keeps this honest: it is the same code that ships.
// `check` and RELEASES_API belong to the command path, not this one; the whole
// module is compiled in so `apply` is literally the shipped code.
#[allow(dead_code)]
#[path = "../src/update.rs"]
mod update;

/// A one-route file server: `/<name>` returns the registered bytes.
struct Stub {
    port: u16,
}

impl Stub {
    fn start(routes: Vec<(String, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let routes = Arc::new(routes);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let routes = Arc::clone(&routes);
                std::thread::spawn(move || serve(stream, &routes));
            }
        });
        Self { port }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}/{path}", self.port)
    }
}

fn serve(mut stream: TcpStream, routes: &[(String, Vec<u8>)]) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    // Drain headers so the client sees a well-formed exchange.
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) if line.trim().is_empty() => break,
            Ok(_) => {}
            Err(_) => return,
        }
    }
    let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let name = path.trim_start_matches('/');
    let body = routes
        .iter()
        .find(|(route, _)| route == name)
        .map(|(_, body)| body.clone());

    let response = match body {
        Some(body) => {
            let mut head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\n\r\n",
                body.len()
            )
            .into_bytes();
            head.extend_from_slice(&body);
            head
        }
        None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
    };
    let _ = stream.write_all(&response);
    let _ = stream.flush();
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    hex::encode(Sha256::digest(bytes))
}

fn available(stub: &Stub) -> update::Available {
    update::Available {
        tag: "v9.9.9".into(),
        version: "9.9.9".into(),
        download_url: stub.url("bin"),
        sha256_url: stub.url("sum"),
    }
}

#[test]
fn a_verified_payload_replaces_the_binary_and_stays_executable() {
    let payload = b"#!/bin/sh\necho new build\n".to_vec();
    let stub = Stub::start(vec![
        ("bin".into(), payload.clone()),
        (
            "sum".into(),
            format!("{}  keeper-syncd\n", sha256_hex(&payload)).into_bytes(),
        ),
    ]);

    let dir = tempfile::tempdir().expect("tempdir");
    let destination = dir.path().join("keeper-syncd");
    std::fs::write(&destination, b"old build").expect("seed");

    let installed = update::apply(&available(&stub), &destination).expect("installs");

    assert_eq!(installed, destination);
    assert_eq!(std::fs::read(&destination).expect("read"), payload);
    assert!(
        is_executable(&destination),
        "an installed daemon that is not executable is not installed"
    );
}

#[test]
fn a_payload_that_fails_its_checksum_never_lands_on_disk() {
    // The whole point of the sidecar: a substituted or truncated asset must not
    // reach the destination, and the previous build must survive untouched.
    let stub = Stub::start(vec![
        ("bin".into(), b"tampered".to_vec()),
        (
            "sum".into(),
            format!("{}  keeper-syncd\n", sha256_hex(b"expected")).into_bytes(),
        ),
    ]);

    let dir = tempfile::tempdir().expect("tempdir");
    let destination = dir.path().join("keeper-syncd");
    std::fs::write(&destination, b"old build").expect("seed");

    let err = update::apply(&available(&stub), &destination).expect_err("refused");
    assert!(
        err.to_string().contains("keeper-syncd v9.9.9"),
        "the error must name what failed, got: {err}"
    );
    assert_eq!(
        std::fs::read(&destination).expect("read"),
        b"old build",
        "a rejected update must leave the working binary in place"
    );
    assert_eq!(
        staged_files(dir.path()),
        1,
        "the staged temp file must be cleaned up, leaving only the original"
    );
}

#[test]
fn a_malformed_checksum_is_refused_before_anything_is_downloaded() {
    // A checksum endpoint answering with an error page would otherwise be
    // compared against, and always mismatch, hiding the real cause.
    let stub = Stub::start(vec![
        ("bin".into(), b"payload".to_vec()),
        ("sum".into(), b"<html>404</html>".to_vec()),
    ]);

    let dir = tempfile::tempdir().expect("tempdir");
    let destination = dir.path().join("keeper-syncd");
    std::fs::write(&destination, b"old build").expect("seed");

    let err = update::apply(&available(&stub), &destination).expect_err("refused");
    assert!(err.to_string().contains("64 hex characters"), "got: {err}");
    assert_eq!(std::fs::read(&destination).expect("read"), b"old build");
}

#[test]
fn an_unreachable_release_is_a_network_error_not_a_silent_success() {
    // Reported as a failure so `doctor` never implies a machine is current when
    // it merely could not ask.
    let stub = Stub::start(vec![]);
    let dir = tempfile::tempdir().expect("tempdir");
    let destination = dir.path().join("keeper-syncd");
    std::fs::write(&destination, b"old build").expect("seed");

    let err = update::apply(&available(&stub), &destination).expect_err("refused");
    assert!(err.to_string().contains("404"), "got: {err}");
}

#[test]
fn the_install_survives_a_destination_that_does_not_exist_yet() {
    // First install on a fresh host: there is no previous binary to replace.
    let payload = b"fresh install".to_vec();
    let stub = Stub::start(vec![
        ("bin".into(), payload.clone()),
        (
            "sum".into(),
            format!("{}  keeper-syncd\n", sha256_hex(&payload)).into_bytes(),
        ),
    ]);

    let dir = tempfile::tempdir().expect("tempdir");
    let destination = dir.path().join("keeper-syncd");

    update::apply(&available(&stub), &destination).expect("installs");
    assert_eq!(std::fs::read(&destination).expect("read"), payload);
    assert!(is_executable(&destination));
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::metadata(path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o111
            != 0
    }
    #[cfg(not(unix))]
    {
        path.exists()
    }
}

fn staged_files(dir: &Path) -> usize {
    std::fs::read_dir(dir).expect("read_dir").count()
}
