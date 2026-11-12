//! 110.5 AC4 (R-B6) and CC-6 (review BLOCKING-1), over a real socket and the
//! real transport: what a person's read authority (`graph.read_status`)
//! becomes for each way keeper-hq can answer it.
//!
//! - The server refusing the caller (`operation.forbidden`/`forbidden`, or
//!   HTTP `401`/`403` on the credential) is `forbidden`, never
//!   `sync_pending` (which once hid every refusal).
//! - Every other answered error — keeper-hq's retryable storage outage
//!   (`sync.internal_error`/`storage_unavailable`), a failed handler, an
//!   unknown or invalid operation — is `sync_pending`: never a refusal
//!   (which would invalidate the reader, and from 112.1 fence the domain),
//!   and never `Unreachable` (an outage of the server, not its answer).
//!
//! Only `/operations` answers as the case says; a sync round gets `500`,
//! which is no refusal, so the replica's own round never closes the gate
//! under the read.

use std::io::{Read, Write};
use std::sync::Arc;

use keeper_core::graph::query::{GraphRemote, QueryError};
use keeper_core::hq::port::{Link, ReplicaSpec};
use keeper_core::hq::sessions::Source;
use keeper_core::hq::syncular::SyncularReplica;
use serde_json::{json, Value};

/// syncular's tagged operation value encoding.
fn tagged(value: &Value) -> Value {
    match value {
        Value::Null => json!({"t": "null"}),
        Value::Bool(b) => json!({"t": "boolean", "v": b}),
        Value::Number(n) => json!({"t": "integer", "v": n.to_string()}),
        Value::String(s) => json!({"t": "string", "v": s}),
        Value::Array(a) => json!({"t": "array", "v": a.iter().map(tagged).collect::<Vec<_>>()}),
        Value::Object(o) => json!({"t": "object", "v": o
            .iter()
            .map(|(k, v)| json!([k, tagged(v)]))
            .collect::<Vec<_>>()}),
    }
}

/// How the server answers `/operations`.
#[derive(Clone, Copy, Debug)]
enum Answer {
    /// HTTP 200 with an operation error frame.
    Frame(&'static str, &'static str, bool),
    /// This HTTP status, no frame.
    Status(u16),
}

fn serve(mut socket: std::net::TcpStream, answer: Answer) {
    let mut seen = Vec::new();
    let mut chunk = [0; 8192];
    let head = loop {
        if let Some(end) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
            break end + 4;
        }
        match socket.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => seen.extend_from_slice(&chunk[..n]),
        }
    };
    let text = String::from_utf8_lossy(&seen[..head]).to_string();
    let length = text
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
        })
        .unwrap_or(0);
    let mut body = seen.len() - head;
    while body < length {
        match socket.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => body += n,
        }
    }
    let operation = text
        .lines()
        .next()
        .is_some_and(|line| line.contains("/operations"));
    let (status, payload) = match (operation, answer) {
        (true, Answer::Frame(code, message, retryable)) => (
            200,
            serde_json::to_vec(&tagged(&json!({
                "revision": 1, "kind": "error", "code": code,
                "message": message, "retryable": retryable,
            })))
            .expect("encode"),
        ),
        (true, Answer::Status(status)) => (status, Vec::new()),
        (false, _) => (500, Vec::new()),
    };
    let _ = write!(
        socket,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    let _ = socket.write_all(&payload);
    let _ = socket.flush();
}

/// A person's `read_status` against a server answering `answer`.
fn read_status(answer: Answer) -> Result<(), QueryError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listen");
    let base = format!("http://{}", listener.local_addr().expect("addr"));
    std::thread::spawn(move || {
        for socket in listener.incoming().flatten() {
            std::thread::spawn(move || serve(socket, answer));
        }
    });
    let dir = tempfile::tempdir().expect("dir");
    let replica = SyncularReplica::open(ReplicaSpec {
        database: "tgdrive".to_owned(),
        path: dir.path().join("tgdrive.sqlite"),
        client_id: Some("ola-dev".to_owned()),
        link: Some(Link {
            base_url: format!("{base}/tgdrive"),
            token: "t".to_owned(),
        }),
        subscriptions: Vec::new(),
        watch_presence: None,
    })
    .expect("opens");
    let source = Source {
        database: "tgdrive".to_owned(),
        replica: Arc::new(replica),
        actor: "ola".to_owned(),
        client: "ola-dev".to_owned(),
    };
    let status = source.read_status("tgdrive").map(|_| ());
    source.replica.close();
    source.replica.join();
    status
}

#[test]
fn a_refused_read_status_is_forbidden_not_sync_pending() {
    for answer in [
        Answer::Frame("operation.forbidden", "forbidden", false),
        Answer::Status(401),
        Answer::Status(403),
    ] {
        assert_eq!(
            read_status(answer),
            Err(QueryError::Forbidden),
            "{answer:?}"
        );
    }
}

#[test]
fn a_server_error_is_sync_pending_never_a_refusal_or_an_outage() {
    for answer in [
        Answer::Frame("sync.internal_error", "storage_unavailable", true),
        Answer::Frame("operation.execution_failed", "handler failed", false),
        Answer::Frame("operation.unknown", "graph.read_status", false),
        Answer::Frame("operation.invalid_request", "invalid", false),
        Answer::Frame("operation.forbidden", "graph_unavailable", false),
    ] {
        assert_eq!(
            read_status(answer),
            Err(QueryError::SyncPending),
            "{answer:?}"
        );
    }
}
