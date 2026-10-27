//! A 429's `Retry-After` against a real socket (R58-01).
//!
//! CLIProxyAPI answers a model whose quota is spent with HTTP 429 and
//! `Retry-After` set to the seconds until the reset — `427327`, about 4.9 days,
//! on 2026-10-26. The client used to sleep exactly that long before its one
//! retry, so a turn on such a model hung for days. These tests stand up a
//! loopback server that answers from a script and count its connections: a
//! wait past `keeper_llm::chat::MAX_RETRY_AFTER` must fail at once, with no
//! second request, and a short one must still be honoured.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use keeper_llm::chat::{
    cancellation, stream_chat, ChatEvent, ChatMessage, ChatOptions, ChatOutcome, ChatRequest, Role,
};
use keeper_llm::error::BotsError;
use keeper_llm::http::client;
use keeper_llm::{Endpoint, ProviderKind};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Well under a second: a refusal that waits for anything fails this.
const AT_ONCE: Duration = Duration::from_millis(500);

/// CPA's body for a model in cooldown, as R58-01 captured it.
const COOLDOWN_BODY: &str = r#"{"error":{"message":"All credentials for model gpt-6.1-sol are cooling down via provider codex, reset_time 118h42m7s","type":"model_cooldown"}}"#;

const MODEL: &str = "gpt-6.1-sol";

/// A loopback HTTP/1.1 server answering connection `n` with `responses[n]`,
/// or with the last response once the script runs out.
struct Server {
    addr: SocketAddr,
    connections: Arc<AtomicU32>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    async fn start(responses: Vec<Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");
        let connections = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&connections);
        let responses = Arc::new(responses);
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let nth = counter.fetch_add(1, Ordering::SeqCst) as usize;
                let responses = Arc::clone(&responses);
                tokio::spawn(async move {
                    read_request(&mut stream).await;
                    let response = &responses[nth.min(responses.len() - 1)];
                    let _ = stream.write_all(response).await;
                    let _ = stream.flush().await;
                });
            }
        });
        Self {
            addr,
            connections,
            task,
        }
    }

    fn connections(&self) -> u32 {
        self.connections.load(Ordering::SeqCst)
    }
}

/// Consume the request head and body, so the client's write completes.
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
        let Some(head_end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&buffer[..head_end]).to_ascii_lowercase();
        let length = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if buffer.len() >= head_end + 4 + length {
            return;
        }
    }
}

/// A 429 carrying `Retry-After: value` and CPA's cooldown body.
fn too_many(retry_after: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nRetry-After: {retry_after}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{COOLDOWN_BODY}",
        COOLDOWN_BODY.len()
    )
    .into_bytes()
}

/// A clean one-word stream.
fn answers_ok() -> Vec<u8> {
    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
      data: {\"id\":\"c1\",\"model\":\"gpt-6.1-sol\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"}}]}\n\n\
      data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
      data: [DONE]\n\n"
        .to_vec()
}

/// One completion against `server` with the default options — two attempts,
/// the production retry policy — and a millisecond back-off.
async fn run(server: &Server) -> Result<ChatOutcome, BotsError> {
    let options = ChatOptions {
        read_timeout: Duration::from_secs(5),
        retry_backoff: Duration::from_millis(10),
        ..ChatOptions::default()
    };
    let client = client(options.read_timeout).expect("client");
    let endpoint = Endpoint {
        kind: ProviderKind::Hermes,
        base_url: format!("http://{}", server.addr),
        bot: None,
        token: None,
    };
    let request = ChatRequest {
        model: MODEL.to_owned(),
        messages: vec![ChatMessage::text(Role::User, "hello")],
        ..ChatRequest::default()
    };
    let (_handle, cancel) = cancellation();
    let mut sink = |_event: ChatEvent| {};
    stream_chat(&client, &endpoint, &request, &options, cancel, &mut sink).await
}

/// `run`, failed at once: within [`AT_ONCE`], with no second request.
async fn refused_at_once(retry_after: &str) -> (u64, Option<String>, String) {
    let server = Server::start(vec![too_many(retry_after), answers_ok()]).await;
    let result = tokio::time::timeout(AT_ONCE, run(&server))
        .await
        .unwrap_or_else(|_| panic!("Retry-After {retry_after} was waited for"));
    assert_eq!(
        server.connections(),
        1,
        "Retry-After {retry_after} was retried"
    );
    match result {
        Err(error @ BotsError::RetryAfterTooLong { .. }) => {
            let text = error.to_string();
            let BotsError::RetryAfterTooLong {
                model,
                status,
                wait_secs,
                until,
                ..
            } = error
            else {
                unreachable!()
            };
            assert_eq!(model, MODEL);
            assert_eq!(status, 429);
            assert!(!text.contains("http://"), "{text}");
            (wait_secs, until, text)
        }
        other => panic!("Retry-After {retry_after} ended as {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_days_long_retry_after_fails_at_once_with_the_model_and_when_it_ends() {
    let before = SystemTime::now();
    let (wait_secs, until, text) = refused_at_once("427327").await;

    assert_eq!(wait_secs, 427_327);
    let until = until.expect("seconds from now name an absolute time");
    let at = httpdate::parse_http_date(&until).expect("an HTTP date");
    let expected = before + Duration::from_secs(427_327);
    let off = at
        .duration_since(expected)
        .unwrap_or_else(|early| early.duration());
    assert!(
        off <= Duration::from_secs(2),
        "{until} is not now + 427327 s"
    );
    assert!(
        text.starts_with(&format!(
            "gpt-6.1-sol is out of quota until about {until} (Retry-After 4.9 days). Pick another model."
        )),
        "{text}"
    );
    // The endpoint's own sentence still reaches the person.
    assert!(text.contains("cooling down"), "{text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retry_after_one_second_past_the_bound_is_not_waited_for() {
    // A literal, not `MAX_RETRY_AFTER + 1` (30 s + 1): a bound raised to
    // hours or days must fail this, so the number cannot follow the constant.
    let (wait_secs, _, text) = refused_at_once("31").await;
    assert_eq!(wait_secs, 31);
    assert!(text.contains("(Retry-After 31 s)"), "{text}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retry_after_past_year_9999_is_refused_without_a_date_and_without_a_panic() {
    // Now plus 3·10¹¹ s is past 9999-12-31, which `httpdate` panics on
    // rather than prints; a panic here unwinds keeper-hq's turn task before it
    // reports the turn ended, and the session never takes another (R599-01).
    let (wait_secs, until, text) = refused_at_once("300000000000").await;
    assert_eq!(wait_secs, 300_000_000_000);
    assert_eq!(until, None);
    assert!(
        text.starts_with(
            "gpt-6.1-sol is out of quota for more than a year \
             (Retry-After more than a year). Pick another model."
        ),
        "{text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retry_after_too_large_for_u64_is_the_largest_and_refused_at_once() {
    // RFC 9111 §1.2.2: delta-seconds past what the recipient can represent
    // are the largest it can, never "no header" and a retry (R599-02).
    let (wait_secs, until, text) = refused_at_once(&"9".repeat(30)).await;
    assert_eq!(wait_secs, u64::MAX);
    assert_eq!(until, None);
    assert!(
        text.starts_with(
            "gpt-6.1-sol is out of quota for more than a year \
             (Retry-After more than a year). Pick another model."
        ),
        "{text}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_short_retry_after_is_waited_out_and_the_retry_answers() {
    let server = Server::start(vec![too_many("1"), answers_ok()]).await;
    let started = Instant::now();
    let outcome = tokio::time::timeout(Duration::from_secs(5), run(&server))
        .await
        .expect("a one-second wait ends")
        .expect("the retry answers");
    assert_eq!(outcome.content, "ok");
    assert_eq!(server.connections(), 2);
    assert!(
        started.elapsed() >= Duration::from_millis(950),
        "Retry-After 1 was not honoured: {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_http_date_retry_after_is_refused_when_far_and_honoured_when_near() {
    // Far: five days out, refused at once, naming the header's own date.
    let far = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(5 * 86_400));
    let (wait_secs, until, text) = refused_at_once(&far).await;
    assert_eq!(until.as_deref(), Some(far.as_str()));
    assert!(
        (5 * 86_400 - 2..=5 * 86_400).contains(&wait_secs),
        "{wait_secs}"
    );
    assert!(text.contains("(Retry-After 5.0 days)"), "{text}");

    // Near: two seconds out (one to two after the date's truncation), waited.
    let near = httpdate::fmt_http_date(SystemTime::now() + Duration::from_secs(2));
    let server = Server::start(vec![too_many(&near), answers_ok()]).await;
    let started = Instant::now();
    let outcome = tokio::time::timeout(Duration::from_secs(5), run(&server))
        .await
        .expect("a two-second wait ends")
        .expect("the retry answers");
    assert_eq!(outcome.content, "ok");
    assert_eq!(server.connections(), 2);
    assert!(
        started.elapsed() >= Duration::from_millis(500),
        "the near date was not waited for: {:?}",
        started.elapsed()
    );
}
