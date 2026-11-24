//! 123.3-AC14 H1 (client) and H6 (what the caller is told)
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use keeper_wire_api::client::{CallOptions, Client, ClientError};
use keeper_wire_api::Code;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::unix::OwnedWriteHalf;
use tokio::net::UnixListener;

const SERVER_HELLO: &str = r#"{"type":"hello","wire":"keeper.wire.v1","app_version":"t","catalogue_digest":"d","max_line_bytes":4194304,"max_in_flight":64}"#;
const SERVER_CLOSE: &str = r#"{"type":"close","error":{"code":"INVALID_ARGUMENT","message":"m","retryable":false},"wire":["keeper.wire.v1"]}"#;

async fn write_line(write: &mut OwnedWriteHalf, line: &str) {
    write.write_all(line.as_bytes()).await.unwrap();
    write.write_all(b"\n").await.unwrap();
    write.flush().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dropped_call_sends_cancel_and_the_connection_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();

        let hello_line = lines.next_line().await.unwrap().unwrap();
        let hello: Value = serde_json::from_str(&hello_line).unwrap();
        assert_eq!(hello["type"], "hello");
        write_line(&mut write, SERVER_HELLO).await;

        let held_line = lines.next_line().await.unwrap().unwrap();
        let held: Value = serde_json::from_str(&held_line).unwrap();
        assert_eq!(held["type"], "call");
        assert_eq!(held["operation"], "fx.held");
        let held_id = held["call_id"].as_str().unwrap().to_owned();

        let cancel_line = lines.next_line().await.unwrap().unwrap();
        let cancel: Value = serde_json::from_str(&cancel_line).unwrap();
        assert_eq!(cancel, json!({"type": "cancel", "call_id": held_id}));

        let echo_line = lines.next_line().await.unwrap().unwrap();
        let echo: Value = serde_json::from_str(&echo_line).unwrap();
        assert_eq!(echo["type"], "call");
        assert_eq!(echo["operation"], "fx.echo");
        let echo_id = echo["call_id"].as_str().unwrap().to_owned();

        let reply = json!({"type": "reply", "call_id": echo_id, "output": {"ok": 1}}).to_string();
        write_line(&mut write, &reply).await;
    });

    let client = Client::connect_socket(&socket, "test").await.unwrap();
    let timed_out = tokio::time::timeout(
        Duration::from_millis(100),
        client.call("fx.held", json!({}), CallOptions::default()),
    )
    .await;
    assert!(timed_out.is_err());

    let answer = client
        .call("fx.echo", json!({}), CallOptions::default())
        .await
        .unwrap();
    assert_eq!(answer, json!({"ok": 1}));

    drop(client);
    server.await.unwrap();
}

async fn assert_in_flight_unavailable(send_close: bool) {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();

        let _hello = lines.next_line().await.unwrap().unwrap();
        write_line(&mut write, SERVER_HELLO).await;

        let call_line = lines.next_line().await.unwrap().unwrap();
        let call: Value = serde_json::from_str(&call_line).unwrap();
        assert_eq!(call["type"], "call");
        let call_id = call["call_id"].as_str().unwrap().to_owned();

        if send_close {
            write_line(&mut write, SERVER_CLOSE).await;
        }
        drop(write);
        drop(lines);
        call_id
    });

    let client = Client::connect_socket(&socket, "test").await.unwrap();
    let err = client
        .call("fx.slow", json!({}), CallOptions::default())
        .await
        .unwrap_err();
    let call_id = server.await.unwrap();

    assert_eq!(err.code, Code::Unavailable);
    assert!(err.retryable);
    assert_eq!(err.request_id.as_deref(), Some(call_id.as_str()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_in_flight_when_the_server_closes_is_unavailable_with_its_id() {
    assert_in_flight_unavailable(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_in_flight_when_the_server_drops_without_close_is_unavailable_with_its_id() {
    assert_in_flight_unavailable(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_close_instead_of_hello_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();

        let _hello = lines.next_line().await.unwrap().unwrap();
        write_line(&mut write, SERVER_CLOSE).await;
    });

    let err = Client::connect_socket(&socket, "test").await.err().unwrap();
    match err {
        ClientError::Refused(wire_err) => assert_eq!(wire_err.code, Code::InvalidArgument),
        other => panic!("expected ClientError::Refused, got {other:?}"),
    }
    server.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watch_reads_events_then_end() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();

        let _hello = lines.next_line().await.unwrap().unwrap();
        write_line(&mut write, SERVER_HELLO).await;

        let watch_line = lines.next_line().await.unwrap().unwrap();
        let watch: Value = serde_json::from_str(&watch_line).unwrap();
        assert_eq!(watch["type"], "watch");
        let call_id = watch["call_id"].as_str().unwrap().to_owned();

        for seq in 1..=3_u64 {
            let event =
                json!({"type": "event", "call_id": call_id, "seq": seq, "output": {"seq": seq}})
                    .to_string();
            write_line(&mut write, &event).await;
        }
        let end = json!({"type": "end", "call_id": call_id}).to_string();
        write_line(&mut write, &end).await;
    });

    let client = Client::connect_socket(&socket, "test").await.unwrap();
    let mut watching = client
        .watch("fx.stream", json!({}), CallOptions::default())
        .await;

    let mut got = Vec::new();
    while let Some(item) = watching.next().await {
        got.push(item.unwrap());
    }
    assert_eq!(
        got,
        vec![json!({"seq": 1}), json!({"seq": 2}), json!({"seq": 3})]
    );

    drop(client);
    server.await.unwrap();
}
