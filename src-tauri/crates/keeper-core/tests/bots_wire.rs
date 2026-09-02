//! The bot chat client against a socket that misbehaves (Story 61.2, FR-372,
//! FR-373, FR-374, NFR-46).
//!
//! # Why this file stands up a real server
//!
//! Every previous epic that failed review failed because the assertion sat on
//! a pure function while the risk sat in the impure shell, and the epic names
//! this story's impure shell explicitly: *a real SSE socket that dies
//! mid-frame*. The framer and the delta machine already have unit tests beside
//! them; what those cannot reach is the part where `reqwest` hands over bytes
//! in whatever sizes the kernel chose, where a silence budget is enforced by
//! the client rather than by a `sleep`, and where a connection is or is not
//! opened a second time.
//!
//! So the server here is a `tokio::net::TcpListener` writing raw HTTP/1.1 by
//! hand. No new dependency, and — more to the point — no framework between the
//! test and the bytes: a test that cannot write half a frame and then stop
//! cannot prove the thing this story is about. It also counts its connections,
//! which is the only way to observe that a partially-streamed request was not
//! retried.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_core::bots::chat::{
    cancellation, stream_chat, ChatEvent, ChatMessage, ChatOptions, ChatRequest, FinishReason, Role,
};
use keeper_core::bots::error::BotsError;
use keeper_core::bots::http::client;
use keeper_core::bots::{Endpoint, ProviderKind};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// ---------------------------------------------------------------------------
// The misbehaving server
// ---------------------------------------------------------------------------

type Handler =
    Arc<dyn Fn(u32, TcpStream) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static>;

/// A local HTTP/1.1 server that answers each connection from a script.
struct TestServer {
    addr: SocketAddr,
    connections: Arc<AtomicU32>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl TestServer {
    /// Bind on an ephemeral loopback port and serve every connection with
    /// `handler`, which is given the 0-based connection number.
    async fn start(handler: Handler) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind a loopback port");
        let addr = listener.local_addr().expect("local addr");
        let connections = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&connections);
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let nth = counter.fetch_add(1, Ordering::SeqCst);
                let handler = Arc::clone(&handler);
                tokio::spawn(async move {
                    read_request(&mut stream).await;
                    handler(nth, stream).await;
                });
            }
        });
        Self {
            addr,
            connections,
            task,
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn connection_count(&self) -> u32 {
        self.connections.load(Ordering::SeqCst)
    }
}

/// Consume the request head and its body, so the client's write completes.
async fn read_request(stream: &mut TcpStream) {
    let mut buffer = Vec::new();
    let mut scratch = [0u8; 4096];
    loop {
        let Ok(read) = stream.read(&mut scratch).await else {
            return;
        };
        if read == 0 {
            return;
        }
        buffer.extend_from_slice(&scratch[..read]);
        let Some(head_end) = find(&buffer, b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&buffer[..head_end]).to_ascii_lowercase();
        let content_length = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if buffer.len() >= head_end + 4 + content_length {
            return;
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The 200 response head for an event stream.
const SSE_HEAD: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";

async fn write(stream: &mut TcpStream, bytes: &[u8]) {
    let _ = stream.write_all(bytes).await;
    let _ = stream.flush().await;
}

/// A handler that writes a fixed body and closes.
fn responds_with(body: &'static [u8]) -> Handler {
    Arc::new(move |_nth, mut stream| {
        Box::pin(async move {
            write(&mut stream, body).await;
        })
    })
}

/// A status-only response with a short JSON body.
fn status_response(status: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn endpoint(server: &TestServer) -> Endpoint {
    Endpoint {
        kind: ProviderKind::Hermes,
        base_url: server.base_url(),
        bot: None,
        // A real-looking credential, so every "no token in the error" assertion
        // has something to find if the redaction is broken.
        token: Some(TOKEN.to_owned()),
    }
}

const TOKEN: &str = "sk-keeper-test-do-not-leak";

fn request() -> ChatRequest {
    ChatRequest {
        model: "test-model".to_owned(),
        messages: vec![ChatMessage::text(Role::User, "hello")],
        ..ChatRequest::default()
    }
}

/// Options with a millisecond silence budget and no retrying, unless a test
/// says otherwise.
fn options(read_timeout_ms: u64) -> ChatOptions {
    ChatOptions {
        read_timeout: Duration::from_millis(read_timeout_ms),
        max_attempts: 1,
        retry_backoff: Duration::from_millis(10),
        ..ChatOptions::default()
    }
}

/// Run one completion against `server`, collecting every event.
async fn run(
    server: &TestServer,
    options: &ChatOptions,
) -> (
    Result<keeper_core::bots::chat::ChatOutcome, BotsError>,
    Vec<String>,
    Arc<Mutex<Vec<BotsError>>>,
) {
    let client = client(options.read_timeout).expect("client");
    let endpoint = endpoint(server);
    let (_handle, cancel) = cancellation();
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let failures: Arc<Mutex<Vec<BotsError>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_for_sink = Arc::clone(&seen);
    let failures_for_sink = Arc::clone(&failures);
    let mut sink = move |event: ChatEvent| {
        let label = match event {
            ChatEvent::FirstToken { .. } => "first-token".to_owned(),
            ChatEvent::ContentDelta(text) => format!("content:{text}"),
            ChatEvent::ReasoningDelta(text) => format!("reasoning:{text}"),
            ChatEvent::ToolCallDelta { index, .. } => format!("tool:{index}"),
            ChatEvent::Usage(_) => "usage".to_owned(),
            ChatEvent::Finished { reason } => format!("finished:{reason:?}"),
            ChatEvent::Failed { error } => {
                let label = format!("failed:{error}");
                failures_for_sink.lock().expect("lock").push(error);
                label
            }
        };
        seen_for_sink.lock().expect("lock").push(label);
    };
    let outcome = stream_chat(&client, &endpoint, &request(), options, cancel, &mut sink).await;
    let events = seen.lock().expect("lock").clone();
    (outcome, events, failures)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_clean_stream_becomes_one_reply() {
    let server = TestServer::start(responds_with(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
          data: {\"id\":\"chatcmpl-1\",\"model\":\"served-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}\n\n\
          : keep-alive\n\n\
          data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"}}]}\n\n\
          data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\", world\"}}],\"usage\":null}\n\n\
          data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
          data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":3,\"total_tokens\":12}}\n\n\
          data: [DONE]\n\n",
    ))
    .await;

    let (outcome, events, _) = run(&server, &options(5_000)).await;
    let outcome = outcome.expect("a clean stream is a completion");

    assert_eq!(outcome.content, "Hello, world");
    assert_eq!(outcome.finish_reason, FinishReason::Stop);
    assert_eq!(outcome.model.as_deref(), Some("served-model"));
    assert_eq!(outcome.response_id.as_deref(), Some("chatcmpl-1"));
    assert_eq!(outcome.usage.and_then(|usage| usage.total_tokens), Some(12));
    assert!(outcome.first_token_ms.is_some());
    assert_eq!(events.first().map(String::as_str), Some("first-token"));
    assert_eq!(
        events
            .iter()
            .filter(|event| *event == "first-token")
            .count(),
        1,
        "FirstToken must be emitted exactly once"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_frame_split_across_two_writes_is_reassembled() {
    // The half-frame sits in the client's buffer across a real socket pause,
    // which is the case a whole-body fixture can never produce.
    let server = TestServer::start(Arc::new(|_nth, mut stream| {
        Box::pin(async move {
            write(&mut stream, SSE_HEAD).await;
            write(
                &mut stream,
                b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"split ",
            )
            .await;
            tokio::time::sleep(Duration::from_millis(60)).await;
            write(
                &mut stream,
                b"across chunks\"}}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            )
            .await;
        })
    }))
    .await;

    let (outcome, _, _) = run(&server, &options(5_000)).await;
    let outcome = outcome.expect("a split frame is still one frame");
    assert_eq!(outcome.content, "split across chunks");
    assert_eq!(outcome.finish_reason, FinishReason::Stop);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_interleaved_tool_calls_reassemble_with_valid_arguments() {
    let server = TestServer::start(responds_with(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
          data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[\
            {\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"\"}},\
            {\"index\":1,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"list_dir\",\"arguments\":\"\"}}]}}]}\n\n\
          data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n\
          data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n\
          data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"a.txt\\\"}\"}}]}}]}\n\n\
          data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"\\\"b/\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n\
          data: [DONE]\n\n",
    ))
    .await;

    let (outcome, _, _) = run(&server, &options(5_000)).await;
    let outcome = outcome.expect("a tool-call stream is a completion");

    assert_eq!(outcome.finish_reason, FinishReason::ToolCalls);
    assert_eq!(outcome.tool_calls.len(), 2);

    let first = &outcome.tool_calls[0];
    assert_eq!(first.id, "call_a");
    assert_eq!(first.name, "read_file");
    assert_eq!(
        first.arguments,
        Some(serde_json::json!({ "path": "a.txt" })),
        "arguments must be concatenated then parsed once, not per fragment"
    );

    let second = &outcome.tool_calls[1];
    assert_eq!(second.id, "call_b");
    assert_eq!(second.name, "list_dir");
    assert_eq!(second.arguments, Some(serde_json::json!({ "path": "b/" })));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_sends_headers_then_nothing_fails_on_silence() {
    // Headers, then the socket is held open forever.
    let server = TestServer::start(Arc::new(|_nth, mut stream| {
        Box::pin(async move {
            write(&mut stream, SSE_HEAD).await;
            tokio::time::sleep(Duration::from_secs(60)).await;
        })
    }))
    .await;

    let started = std::time::Instant::now();
    let (outcome, _, _) = run(&server, &options(200)).await;
    let elapsed = started.elapsed();

    match outcome {
        Err(BotsError::Timeout { after_ms }) => assert_eq!(after_ms, 200),
        other => panic!("expected a silence timeout, got {other:?}"),
    }
    assert!(
        elapsed < Duration::from_secs(5),
        "the silence budget must fire in milliseconds, not after {elapsed:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_but_alive_stream_outlives_the_silence_budget() {
    // The proof that the budget bounds SILENCE and not DURATION: every gap is
    // under the budget, the total is several times it. A whole-request
    // deadline in place of the read timeout kills this stream; nothing else in
    // this file notices the difference.
    let server = TestServer::start(Arc::new(|_nth, mut stream| {
        Box::pin(async move {
            write(&mut stream, SSE_HEAD).await;
            for word in ["one ", "two ", "three ", "four ", "five"] {
                tokio::time::sleep(Duration::from_millis(120)).await;
                let frame = format!(
                    "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{word}\"}}}}]}}\n\n"
                );
                write(&mut stream, frame.as_bytes()).await;
            }
            tokio::time::sleep(Duration::from_millis(120)).await;
            write(
                &mut stream,
                b"data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            )
            .await;
        })
    }))
    .await;

    let started = std::time::Instant::now();
    // 300 ms of silence allowed; the whole exchange takes ~720 ms.
    let (outcome, _, _) = run(&server, &options(300)).await;
    let outcome = outcome.expect("a stream that keeps delivering must never time out");

    assert_eq!(outcome.content, "one two three four five");
    assert!(
        started.elapsed() > Duration::from_millis(400),
        "the fixture did not actually outrun the budget; the test proves nothing"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_cut_mid_message_keeps_the_partial_and_reports_the_failure() {
    let server = TestServer::start(Arc::new(|_nth, mut stream| {
        Box::pin(async move {
            write(&mut stream, SSE_HEAD).await;
            write(
                &mut stream,
                b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"the answer is \"}}]}\n\n\
                  data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"fo",
            )
            .await;
            // Close mid-frame. Dropping the stream is the socket dying.
        })
    }))
    .await;

    let (outcome, events, failures) = run(&server, &options(5_000)).await;
    let outcome = outcome.expect("a truncated stream still yields the partial record");

    assert_eq!(
        outcome.content, "the answer is ",
        "the content that did arrive is a real record and must survive"
    );
    assert_eq!(outcome.finish_reason, FinishReason::Failed);

    let failures = failures.lock().expect("lock");
    assert_eq!(failures.len(), 1, "exactly one Failed event");
    assert!(
        matches!(failures[0], BotsError::Protocol { .. }),
        "a stream that ended without [DONE] or a finish reason is a protocol failure, got {:?}",
        failures[0]
    );
    assert!(events.iter().any(|event| event.starts_with("failed:")));
    assert!(events.iter().any(|event| event == "finished:Failed"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_partially_streamed_request_is_never_retried() {
    // The failure here is a SILENCE TIMEOUT, which `BotsError::is_retryable`
    // says yes to — it is the documented transport-retry case. The only thing
    // standing between this stall and a second request is the rule that the
    // attempt had already produced stream bytes.
    //
    // That rule is not fussiness. There is no resume token on this wire, a
    // re-sent request samples afresh so the two halves cannot be spliced, and
    // the tokens of the abandoned attempt were generated and billed either
    // way (research §7.3).
    let server = TestServer::start(Arc::new(|_nth, mut stream| {
        Box::pin(async move {
            write(&mut stream, SSE_HEAD).await;
            write(
                &mut stream,
                b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n",
            )
            .await;
            // Then the socket stalls, mid-answer, forever.
            tokio::time::sleep(Duration::from_secs(60)).await;
        })
    }))
    .await;

    let options = ChatOptions {
        read_timeout: Duration::from_millis(200),
        max_attempts: 3,
        retry_backoff: Duration::from_millis(10),
        ..ChatOptions::default()
    };
    let (outcome, _, failures) = run(&server, &options).await;

    let outcome = outcome.expect("the partial record survives the stall");
    assert_eq!(outcome.content, "partial");
    assert_eq!(outcome.finish_reason, FinishReason::Failed);
    assert!(
        matches!(
            failures.lock().expect("lock").first(),
            Some(BotsError::Timeout { .. })
        ),
        "the stall must be reported as a silence timeout"
    );
    assert_eq!(
        server.connection_count(),
        1,
        "a partially-streamed completion must never be sent again"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_that_produced_no_bytes_is_retried_once() {
    // The other half of the same rule: 503 before a single stream byte is the
    // documented retryable case, so the second attempt must happen.
    let server = TestServer::start(Arc::new(|nth, mut stream| {
        Box::pin(async move {
            if nth == 0 {
                write(&mut stream, &status_response("503 Service Unavailable", "{\"error\":\"warming up\"}")).await;
            } else {
                write(
                    &mut stream,
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
                      data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"second try\"},\"finish_reason\":\"stop\"}]}\n\n\
                      data: [DONE]\n\n",
                )
                .await;
            }
        })
    }))
    .await;

    let options = ChatOptions {
        read_timeout: Duration::from_millis(5_000),
        max_attempts: 2,
        retry_backoff: Duration::from_millis(10),
        ..ChatOptions::default()
    };
    let (outcome, _, _) = run(&server, &options).await;

    assert_eq!(outcome.expect("the retry succeeded").content, "second try");
    assert_eq!(server.connection_count(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unauthorized_is_a_typed_error_carrying_no_credential() {
    let server = TestServer::start(responds_with(
        b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 41\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"invalid api key\"}}\r\n",
    ))
    .await;

    let (outcome, _, _) = run(&server, &options(5_000)).await;
    let err = outcome.expect_err("401 is a failure");

    match &err {
        BotsError::Status {
            status, endpoint, ..
        } => {
            assert_eq!(*status, 401);
            assert_eq!(endpoint, "POST /v1/chat/completions");
        }
        other => panic!("expected a typed status error, got {other:?}"),
    }

    // The whole point of the sensitive header and the URL-free endpoint name.
    let text = err.to_string();
    assert!(
        !text.contains(TOKEN),
        "the error leaked the credential: {text}"
    );
    assert!(
        !text.to_ascii_lowercase().contains("authorization"),
        "the error named the credential header: {text}"
    );
    assert!(
        !text.contains("http://"),
        "the error leaked the endpoint URL: {text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn not_found_is_a_distinct_typed_error() {
    let server = TestServer::start(responds_with(
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot here",
    ))
    .await;

    let (outcome, _, _) = run(&server, &options(5_000)).await;
    match outcome.expect_err("404 is a failure") {
        BotsError::Status { status, .. } => assert_eq!(status, 404),
        other => panic!("expected a typed status error, got {other:?}"),
    }
    assert_eq!(
        server.connection_count(),
        1,
        "a 404 is keeper's own wrong URL; retrying it just repeats it"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_error_is_a_distinct_typed_error() {
    let server = TestServer::start(responds_with(
        b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 5\r\nConnection: close\r\n\r\nboom!",
    ))
    .await;

    let (outcome, _, _) = run(&server, &options(5_000)).await;
    match outcome.expect_err("500 is a failure") {
        BotsError::Status { status, detail, .. } => {
            assert_eq!(status, 500);
            assert_eq!(detail, "boom!");
        }
        other => panic!("expected a typed status error, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn done_with_no_usage_frame_is_a_reply_with_no_numbers() {
    // An endpoint that omits `usage` is a fact about the endpoint. It must not
    // become a zero.
    let server = TestServer::start(responds_with(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
          data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"terse\"}}]}\n\n\
          data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
          data: [DONE]\n\n",
    ))
    .await;

    let (outcome, events, _) = run(&server, &options(5_000)).await;
    let outcome = outcome.expect("a completion");

    assert_eq!(outcome.content, "terse");
    assert_eq!(outcome.finish_reason, FinishReason::Stop);
    assert_eq!(outcome.usage, None);
    assert!(!events.iter().any(|event| event == "usage"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_stream_keeps_what_had_already_arrived() {
    let server = TestServer::start(Arc::new(|_nth, mut stream| {
        Box::pin(async move {
            write(&mut stream, SSE_HEAD).await;
            write(
                &mut stream,
                b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"begun\"}}]}\n\n",
            )
            .await;
            // Then nothing at all, until the client gives up or cancels.
            tokio::time::sleep(Duration::from_secs(60)).await;
        })
    }))
    .await;

    let client = client(Duration::from_secs(30)).expect("client");
    let endpoint = endpoint(&server);
    let (handle, cancel) = cancellation();
    let options = ChatOptions {
        read_timeout: Duration::from_secs(30),
        max_attempts: 1,
        ..ChatOptions::default()
    };

    // Press Stop shortly after the first delta lands — while the socket is
    // silent, which is the case a between-chunks flag would sleep through.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        handle.cancel();
    });

    let mut sink = |_event: ChatEvent| {};
    let started = std::time::Instant::now();
    let outcome = stream_chat(&client, &endpoint, &request(), &options, cancel, &mut sink)
        .await
        .expect("a cancelled turn is a record, not an error");

    assert_eq!(outcome.finish_reason, FinishReason::Cancelled);
    assert_eq!(outcome.content, "begun");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "Stop must interrupt a silent socket, not wait out the read timeout"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_in_band_error_frame_fails_the_turn_rather_than_completing_it() {
    let server = TestServer::start(responds_with(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
          data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n\
          data: {\"error\":{\"code\":\"server_error\",\"message\":\"upstream died\"}}\n\n\
          data: [DONE]\n\n",
    ))
    .await;

    let (outcome, _, failures) = run(&server, &options(5_000)).await;
    let outcome = outcome.expect("the partial record survives");

    assert_eq!(outcome.finish_reason, FinishReason::Failed);
    assert_eq!(outcome.content, "partial");
    let failures = failures.lock().expect("lock");
    assert!(
        matches!(failures.first(), Some(BotsError::Provider { .. })),
        "an error frame followed by [DONE] must not commit a failed turn as a success"
    );
}
