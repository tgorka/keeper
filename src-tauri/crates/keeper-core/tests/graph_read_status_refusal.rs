//! 110.5 AC4 (R-B6) over a real socket and the real transport: a server that
//! answers every request the way keeper-hq answers `graph.read_status` for a
//! person with no graph role (an operation error, `operation.forbidden` /
//! `forbidden`, retryable false). The native replica reports the server's
//! answer as its refusal, and a person's read authority is `forbidden`, never
//! `sync_pending` (which once hid every refusal).

use std::io::{Read, Write};
use std::sync::Arc;

use keeper_core::graph::query::{GraphRemote, QueryError};
use keeper_core::hq::port::{Link, Replica, ReplicaError, ReplicaSpec};
use keeper_core::hq::sessions::Source;
use keeper_core::hq::syncular::SyncularReplica;
use serde_json::{json, Value};

fn tagged(value: &Value) -> Value {
    match value {
        Value::Null => json!({"t": "null"}),
        Value::Bool(b) => json!({"t": "boolean", "v": b}),
        Value::Number(n) => json!({"t": "integer", "v": n.to_string()}),
        Value::String(s) => json!({"t": "string", "v": s}),
        Value::Array(a) => json!({"t": "array", "v": a.iter().map(tagged).collect::<Vec<_>>()}),
        Value::Object(o) => json!({"t": "object", "v": o.iter()
            .map(|(k, v)| json!([k, tagged(v)])).collect::<Vec<_>>()}),
    }
}

fn serve(mut socket: std::net::TcpStream) {
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
        .find_map(|l| {
            l.to_ascii_lowercase()
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
    let answer = serde_json::to_vec(&tagged(&json!({
        "revision": 1, "kind": "error", "code": "operation.forbidden",
        "message": "forbidden", "retryable": false,
    })))
    .expect("encode");
    let _ = write!(
        socket,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        answer.len()
    );
    let _ = socket.write_all(&answer);
    let _ = socket.flush();
}

#[test]
fn a_refused_read_status_is_forbidden_not_sync_pending() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listen");
    let base = format!("http://{}", listener.local_addr().expect("addr"));
    std::thread::spawn(move || {
        for socket in listener.incoming().flatten() {
            std::thread::spawn(move || serve(socket));
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
    let raw = replica.remote_query(
        "graph.read_status",
        json!({"version": 1, "domain": "tgdrive"}),
    );
    let source = Source {
        database: "tgdrive".to_owned(),
        replica: Arc::new(replica),
        actor: "ola".to_owned(),
        client: "ola-dev".to_owned(),
    };
    let status = source.read_status("tgdrive");
    assert!(
        matches!(raw, Err(ReplicaError::Remote { .. })),
        "the server's refusal must be its answer, got {raw:?}"
    );
    assert_eq!(status.err(), Some(QueryError::Forbidden));
}
