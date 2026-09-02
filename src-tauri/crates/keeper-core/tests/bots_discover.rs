//! Discovery against a real socket (Story 61.3, FR-375, FR-376, FR-377).
//!
//! Every assertion here goes through `reqwest`, a TCP connection and a real
//! HTTP response, because the risk in this story lives entirely in the impure
//! shell: a payload shape that changed, a status keeper misclassified, a body
//! that is not JSON, a socket that refuses. A test that proved the tri-state
//! through a pure function would have proved nothing about the endpoint — so
//! the tri-state is exercised here by *serving* an old Ollama's `/api/tags`,
//! byte for byte as the Ollama docs capture it.
//!
//! The server is a raw [`TcpListener`] on a thread, which is the pattern
//! `keeper-sync/src/http.rs` and `keeper-syncd/tests/update_install.rs`
//! already use, and it adds no dependency to the crate. It records every
//! request line it answers, which is how the "one round trip, not N+1" claim
//! is checked rather than asserted.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use keeper_core::bots::discover::{self, BotRoster};
use keeper_core::bots::{Endpoint, ProviderKind};
use keeper_core::vm::{BotPresence, BotReach};

/// The credential every test sends, so its absence from an error string is a
/// claim a test can make. Not a real token shape for any product.
const TOKEN: &str = "bots-discover-test-credential";

/// One route: an exact path, the status to answer with, and the body.
#[derive(Clone)]
struct Route {
    path: &'static str,
    status: u16,
    body: &'static str,
}

fn route(path: &'static str, body: &'static str) -> Route {
    Route {
        path,
        status: 200,
        body,
    }
}

fn failing(path: &'static str, status: u16) -> Route {
    Route {
        path,
        status,
        body: "{\"error\":\"served by the test stub\"}",
    }
}

/// A tiny HTTP server that answers exactly the routes it was given and records
/// what it was asked.
struct Stub {
    port: u16,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Stub {
    fn start(routes: Vec<Route>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("read the bound port").port();
        let routes = Arc::new(routes);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let routes = Arc::clone(&routes);
                let recorder = Arc::clone(&recorder);
                std::thread::spawn(move || serve(stream, &routes, &recorder));
            }
        });
        Self { port, seen }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn endpoint(&self, kind: ProviderKind) -> Endpoint {
        Endpoint {
            kind,
            base_url: self.base_url(),
            bot: None,
            token: Some(TOKEN.to_owned()),
        }
    }

    /// Every request line answered so far, as `"GET /api/tags"`.
    fn requests(&self) -> Vec<String> {
        self.seen.lock().expect("the recorder lock").clone()
    }
}

fn serve(mut stream: TcpStream, routes: &[Route], seen: &Mutex<Vec<String>>) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone the socket"));
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_owned();
    let path = parts.next().unwrap_or("/").to_owned();
    // Drain the headers so the client sees a well-formed exchange, and note
    // that nothing here echoes them back: a stub that reflected the
    // `Authorization` header would make the "no token in an error" test lie.
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) if line.trim().is_empty() => break,
            Ok(_) => {
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            Err(_) => return,
        }
    }
    if content_length > 0 {
        let mut body = vec![0u8; content_length];
        use std::io::Read as _;
        let _ = reader.read_exact(&mut body);
    }
    seen.lock()
        .expect("the recorder lock")
        .push(format!("{method} {path}"));

    let answer = routes.iter().find(|route| route.path == path);
    let (status, body) = match answer {
        Some(route) => (route.status, route.body),
        None => (404, "{\"error\":\"no such route on the stub\"}"),
    };
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn client() -> reqwest::Client {
    discover::discovery_client().expect("the discovery client builds")
}

/// The `/api/tags` payload from the Ollama docs' own capture (research R2
/// §4.1), with the `capabilities` array the Go type serves and the docs example
/// omits.
const TAGS_WITH_CAPABILITIES: &str = r#"{"models":[{"name":"deepseek-r1:latest","model":"deepseek-r1:latest",
  "modified_at":"2025-05-10T08:06:48.639712648-07:00","size":4683075271,
  "digest":"0a8c266910232fd3291e71e5ba1e058cc5af9d411192cf88b6d30e92b6e73163",
  "details":{"parent_model":"","format":"gguf","family":"qwen2","families":["qwen2"],
             "parameter_size":"7.6B","quantization_level":"Q4_K_M"},
  "capabilities":["completion","tools","thinking"]}]}"#;

/// The identical payload as an Ollama that predates the field serves it: no
/// `capabilities` key at all. Verbatim from research R2 §4.1.
const TAGS_WITHOUT_CAPABILITIES: &str = r#"{"models":[{"name":"deepseek-r1:latest","model":"deepseek-r1:latest",
  "modified_at":"2025-05-10T08:06:48.639712648-07:00","size":4683075271,
  "digest":"0a8c266910232fd3291e71e5ba1e058cc5af9d411192cf88b6d30e92b6e73163",
  "details":{"parent_model":"","format":"gguf","family":"qwen2","families":["qwen2"],
             "parameter_size":"7.6B","quantization_level":"Q4_K_M"}}]}"#;

#[tokio::test]
async fn discover_ollama_tags_carry_capabilities_in_one_round_trip() {
    let stub = Stub::start(vec![
        route("/api/tags", TAGS_WITH_CAPABILITIES),
        // Present so a regression that reaches for it would succeed and be
        // caught by the request log below, instead of failing for the wrong
        // reason.
        route(
            "/api/show",
            "{\"capabilities\":[\"completion\",\"vision\"]}",
        ),
    ]);
    let endpoint = stub.endpoint(ProviderKind::Ollama);

    let models = discover::models(&client(), &endpoint)
        .await
        .expect("the tag list parses");

    assert_eq!(models.len(), 1, "one model was served");
    let model = &models[0];
    assert_eq!(model.id, "deepseek-r1:latest");
    assert_eq!(model.family.as_deref(), Some("qwen2"));
    assert_eq!(model.parameter_size.as_deref(), Some("7.6B"));
    assert_eq!(model.quantization.as_deref(), Some("Q4_K_M"));
    assert_eq!(model.size_bytes, Some(4_683_075_271));
    assert_eq!(model.tools, Some(true), "the array said tools");
    assert_eq!(model.reasoning, Some(true), "the array said thinking");
    assert_eq!(
        model.vision,
        Some(false),
        "a present array that omits vision is a stated no, not unknown"
    );
    assert_eq!(
        model.capabilities,
        vec![
            "completion".to_owned(),
            "tools".to_owned(),
            "thinking".to_owned()
        ],
        "the endpoint's own vocabulary is kept verbatim"
    );

    assert_eq!(
        stub.requests(),
        vec!["GET /api/tags".to_owned()],
        "capabilities come from /api/tags, so discovery is one request and never one-plus-N"
    );
}

#[tokio::test]
async fn discover_ollama_without_capabilities_is_unknown_never_false() {
    let stub = Stub::start(vec![route("/api/tags", TAGS_WITHOUT_CAPABILITIES)]);
    let endpoint = stub.endpoint(ProviderKind::Ollama);

    let models = discover::models(&client(), &endpoint)
        .await
        .expect("the tag list parses");

    let model = &models[0];
    assert_eq!(
        model.vision, None,
        "an Ollama that predates the capabilities array knows nothing about vision, \
         and nothing is not no"
    );
    assert_eq!(model.tools, None, "same for tools");
    assert_eq!(model.reasoning, None, "same for reasoning");
    assert!(
        model.capabilities.is_empty(),
        "no vocabulary was served, so none is reported"
    );
    // The row itself still parsed: unknown capabilities must not cost the user
    // the model.
    assert_eq!(model.id, "deepseek-r1:latest");
    assert_eq!(model.parameter_size.as_deref(), Some("7.6B"));
}

#[tokio::test]
async fn discover_hermes_merges_models_with_model_options() {
    let stub = Stub::start(vec![
        route(
            "/v1/models",
            r#"{"object":"list","data":[{"id":"hermes-agent","owned_by":"hermes","root":"hermes-agent"},
                {"id":"unenriched-alias","owned_by":"hermes","root":"unenriched-alias"}]}"#,
        ),
        route(
            "/api/model/options",
            r#"{"model":"hermes-agent","provider":"nous","providers":[{"slug":"nous","name":"Nous",
                "models":[{"id":"hermes-agent","capabilities":{"supports_tools":true,
                    "supports_vision":false,"supports_reasoning":true,"context_window":131072,
                    "max_output_tokens":8192,"model_family":"hermes"}},
                  {"id":"routed-only","capabilities":{"supports_tools":false,
                    "context_window":8192}}]}]}"#,
        ),
    ]);
    let endpoint = stub.endpoint(ProviderKind::Hermes);

    let models = discover::models(&client(), &endpoint)
        .await
        .expect("both payloads parse");

    let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["hermes-agent", "unenriched-alias", "routed-only"],
        "the /v1/models order is kept, and a model only the options payload knows is appended"
    );

    let enriched = &models[0];
    assert_eq!(enriched.tools, Some(true));
    assert_eq!(
        enriched.vision,
        Some(false),
        "a stated false stays false — this is the arm the tri-state must not lose"
    );
    assert_eq!(enriched.reasoning, Some(true));
    assert_eq!(enriched.context_window, Some(131_072));
    assert_eq!(enriched.max_output_tokens, Some(8192));
    assert_eq!(enriched.family.as_deref(), Some("hermes"));

    let unenriched = &models[1];
    assert_eq!(
        (unenriched.vision, unenriched.tools, unenriched.reasoning),
        (None, None, None),
        "an alias the options payload does not mention is unknown on all three"
    );
    assert_eq!(unenriched.context_window, None);

    let partial = &models[2];
    assert_eq!(partial.tools, Some(false), "the one key it stated");
    assert_eq!(
        partial.vision, None,
        "the keys it did not state stay unknown, per key and not per model"
    );
    assert_eq!(partial.context_window, Some(8192));
}

#[tokio::test]
async fn discover_hermes_models_survive_options_being_unavailable() {
    let stub = Stub::start(vec![
        route(
            "/v1/models",
            r#"{"object":"list","data":[{"id":"hermes-agent"}]}"#,
        ),
        failing("/api/model/options", 404),
    ]);
    let endpoint = stub.endpoint(ProviderKind::Hermes);

    let models = discover::models(&client(), &endpoint)
        .await
        .expect("the roster still returns");

    assert_eq!(models.len(), 1, "an older Hermes still lists its model");
    assert_eq!(
        (models[0].vision, models[0].tools, models[0].reasoning),
        (None, None, None),
        "capabilities keeper could not read are unknown, not false"
    );
}

#[tokio::test]
async fn discover_bot_probe_404_means_the_bot_does_not_exist() {
    let stub = Stub::start(vec![failing("/p/ghost/v1/models", 404)]);
    let endpoint = stub.endpoint(ProviderKind::Hermes);

    let probe = discover::probe_bot(&client(), &endpoint, "ghost").await;

    assert_eq!(probe.reach, BotReach::Online, "the gateway answered");
    assert_eq!(probe.status, Some(404));
    assert_eq!(probe.presence, Some(BotPresence::Absent));
    let reason = probe.reason.expect("a 404 carries its own sentence");
    assert!(
        reason.contains("ghost"),
        "the sentence names the bot: {reason}"
    );
    assert!(
        !reason.contains(TOKEN),
        "no sentence carries the credential: {reason}"
    );
    assert_eq!(
        stub.requests(),
        vec!["GET /p/ghost/v1/models".to_owned()],
        "the profile prefix is how a bot is addressed on the wire"
    );
}

#[tokio::test]
async fn discover_bot_probe_401_is_a_different_answer_from_404() {
    let stub = Stub::start(vec![failing("/p/locked/v1/models", 401)]);
    let endpoint = stub.endpoint(ProviderKind::Hermes);

    let probe = discover::probe_bot(&client(), &endpoint, "locked").await;

    assert_eq!(probe.reach, BotReach::Online);
    assert_eq!(probe.status, Some(401));
    assert_eq!(
        probe.presence,
        Some(BotPresence::Unknown),
        "a refused credential says nothing about whether the bot exists — a Hermes profile \
         served under its own key answers exactly this way"
    );
    let reason = probe.reason.expect("a 401 carries its own sentence");
    assert!(
        reason.contains("key"),
        "the sentence sends the user to the credential: {reason}"
    );
    assert!(!reason.contains(TOKEN), "and never prints it: {reason}");
}

#[tokio::test]
async fn discover_bot_probe_cannot_tell_when_nothing_answers() {
    // A port that was bound and released: the kernel refuses the connection
    // instantly, so nothing is resolved and no timeout is waited out.
    let closed = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = closed.local_addr().expect("read the bound port").port();
    drop(closed);

    let endpoint = Endpoint {
        kind: ProviderKind::Hermes,
        base_url: format!("http://127.0.0.1:{port}"),
        bot: None,
        token: Some(TOKEN.to_owned()),
    };

    let probe = discover::probe_bot(&client(), &endpoint, "ghost").await;

    assert_eq!(
        probe.reach,
        BotReach::Offline,
        "the same word the app already uses for a remote it cannot reach"
    );
    assert_eq!(probe.status, None, "there was no answer to have a status");
    assert_eq!(
        probe.presence,
        Some(BotPresence::Unknown),
        "cannot tell — never Absent, which would send the user to retype a name that was right"
    );
    let reason = probe.reason.expect("an unreachable endpoint says so");
    assert!(
        !reason.contains(TOKEN),
        "no credential in the reason: {reason}"
    );
    assert!(
        !reason.contains(&format!("127.0.0.1:{port}")),
        "and no URL either, because a base URL may carry userinfo: {reason}"
    );
}

#[tokio::test]
async fn discover_malformed_json_names_the_endpoint_and_not_the_token() {
    let stub = Stub::start(vec![route("/api/tags", "{\"models\": [ this is not json")]);
    let endpoint = stub.endpoint(ProviderKind::Ollama);

    let err = discover::models(&client(), &endpoint)
        .await
        .expect_err("a body that is not JSON is a typed failure, not an empty list");

    let text = err.to_string();
    assert!(
        text.contains("GET /api/tags"),
        "the failure names the route it came from: {text}"
    );
    assert!(!text.contains(TOKEN), "and carries no credential: {text}");
    assert!(
        !text.contains("127.0.0.1"),
        "and no host, since the base URL is the user's: {text}"
    );
}

#[tokio::test]
async fn discover_health_reads_the_version_each_kind_reports() {
    let ollama = Stub::start(vec![route("/api/version", "{\"version\":\"0.33.2\"}")]);
    let probe = discover::health(&client(), &ollama.endpoint(ProviderKind::Ollama)).await;
    assert_eq!(probe.reach, BotReach::Online);
    assert_eq!(probe.status, Some(200));
    assert_eq!(probe.version.as_deref(), Some("0.33.2"));
    assert!(
        probe.round_trip_ms.is_some_and(|ms| ms >= 0),
        "the round trip is measured, not estimated"
    );
    assert_eq!(probe.reason, None, "a healthy endpoint needs no sentence");
    assert_eq!(
        ollama.requests(),
        vec!["GET /api/version".to_owned()],
        "the cheap public route, per kind"
    );

    let hermes = Stub::start(vec![route(
        "/health",
        "{\"status\":\"ok\",\"version\":\"0.21.0\"}",
    )]);
    let probe = discover::health(&client(), &hermes.endpoint(ProviderKind::Hermes)).await;
    assert_eq!(probe.version.as_deref(), Some("0.21.0"));
    assert_eq!(hermes.requests(), vec!["GET /health".to_owned()]);
}

#[tokio::test]
async fn discover_health_of_a_dead_endpoint_is_offline_and_unreachable() {
    let closed = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = closed.local_addr().expect("read the bound port").port();
    drop(closed);

    let endpoint = Endpoint {
        kind: ProviderKind::Ollama,
        base_url: format!("http://127.0.0.1:{port}"),
        bot: None,
        token: None,
    };
    let probe = discover::health(&client(), &endpoint).await;

    assert_eq!(probe.reach, BotReach::Offline);
    assert_eq!(probe.version, None);
    assert_eq!(probe.presence, None, "no bot was named, so none is judged");
    assert_eq!(
        discover::health_state(&probe),
        keeper_core::bots::BotHealthState::Unreachable,
        "the verdict the provider row stores is derived here, not in the shell"
    );
}

#[tokio::test]
async fn discover_health_of_a_refusing_endpoint_is_online_and_unauthorized() {
    let stub = Stub::start(vec![failing("/health", 401)]);
    let probe = discover::health(&client(), &stub.endpoint(ProviderKind::Hermes)).await;

    assert_eq!(
        probe.reach,
        BotReach::Online,
        "an answer keeper did not like is still an answer"
    );
    assert_eq!(
        discover::health_state(&probe),
        keeper_core::bots::BotHealthState::Unauthorized
    );
}

#[tokio::test]
async fn discover_ollama_bot_is_a_model_tag() {
    let stub = Stub::start(vec![route("/api/tags", TAGS_WITH_CAPABILITIES)]);
    let endpoint = stub.endpoint(ProviderKind::Ollama);
    let client = client();

    let present = discover::probe_bot(&client, &endpoint, "deepseek-r1:latest").await;
    assert_eq!(present.presence, Some(BotPresence::Exists));
    assert_eq!(present.reason, None);

    let missing = discover::probe_bot(&client, &endpoint, "not-pulled:latest").await;
    assert_eq!(
        missing.presence,
        Some(BotPresence::Absent),
        "the tag list answered, and it does not contain that tag"
    );
    assert!(
        missing
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("not-pulled:latest")),
        "the sentence names the tag"
    );
}

#[tokio::test]
async fn discover_enumerate_bots_lists_ollama_tags_and_refuses_hermes_with_a_reason() {
    let stub = Stub::start(vec![route("/api/tags", TAGS_WITH_CAPABILITIES)]);
    let client = client();

    let ollama = discover::enumerate_bots(&client, &stub.endpoint(ProviderKind::Ollama))
        .await
        .expect("the tag list parses");
    assert_eq!(
        ollama,
        BotRoster::Enumerated(vec!["deepseek-r1:latest".to_owned()]),
        "an Ollama bot is a model tag, and those it does list"
    );

    let hermes = discover::enumerate_bots(&client, &stub.endpoint(ProviderKind::Hermes))
        .await
        .expect("a refusal is an outcome, not a failure");
    assert_eq!(
        hermes,
        BotRoster::Unavailable {
            reason: discover::NO_BOT_ROSTER
        },
        "the bearer API has no roster route, so keeper says so in one sentence written once"
    );
    assert_eq!(
        stub.requests(),
        vec!["GET /api/tags".to_owned()],
        "the Hermes refusal sends no request at all — keeper does not probe a route it has \
         read the source of and knows is absent"
    );
}
