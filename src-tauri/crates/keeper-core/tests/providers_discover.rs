//! Provider discovery against a real socket (Story 61.3, FR-375, FR-377).
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

use keeper_core::providers::discover;
use keeper_core::providers::{Endpoint, ProviderKind};
use keeper_core::vm::ProviderReach;

/// The credential every test sends, so its absence from an error string is a
/// claim a test can make. Not a real token shape for any product.
const TOKEN: &str = "providers-discover-test-credential";

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
    assert_eq!(probe.reach, ProviderReach::Online);
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

    assert_eq!(probe.reach, ProviderReach::Offline);
    assert_eq!(probe.version, None);
    assert_eq!(
        discover::health_state(&probe),
        keeper_core::providers::ProviderHealthState::Unreachable,
        "the verdict the provider row stores is derived here, not in the shell"
    );
}

#[tokio::test]
async fn discover_health_of_a_refusing_endpoint_is_online_and_unauthorized() {
    let stub = Stub::start(vec![failing("/health", 401)]);
    let probe = discover::health(&client(), &stub.endpoint(ProviderKind::Hermes)).await;

    assert_eq!(
        probe.reach,
        ProviderReach::Online,
        "an answer keeper did not like is still an answer"
    );
    assert_eq!(
        discover::health_state(&probe),
        keeper_core::providers::ProviderHealthState::Unauthorized
    );
}

/// A CLIProxyAPI `/v1/models` answer, in the shape it serves: OpenAI's list,
/// each row an id with `object` and `owned_by`, and no capability anywhere.
/// The duplicate and the empty id are there to be dropped.
const OPENAI_MODELS: &str = r#"{"object":"list","data":[
  {"id":"claude-opus-5-5","object":"model","created":1767225600,"owned_by":"anthropic"},
  {"id":"gpt-6-astra","object":"model","created":1767225600,"owned_by":"openai"},
  {"id":"gemini-3-pro","object":"model","created":1767225600,"owned_by":"google"},
  {"id":"gpt-6-astra","object":"model","created":1767225600,"owned_by":"openai"},
  {"id":"","object":"model","owned_by":"nobody"}]}"#;

/// The sentence an OpenAI-compatible endpoint's refusal of keeper's key
/// prints, at the status it answered.
fn openai_refusal(status: u16) -> String {
    format!(
        "The endpoint refused keeper's key ({status}). Check the key this provider was saved with."
    )
}

#[tokio::test]
async fn discover_openai_health_is_one_get_of_v1_models() {
    let stub = Stub::start(vec![
        route("/v1/models", OPENAI_MODELS),
        // Present so a regression that reached for another kind's health
        // route would succeed and be caught by the request log.
        route("/health", "{\"status\":\"ok\",\"version\":\"0.21.0\"}"),
        route("/api/version", "{\"version\":\"0.33.2\"}"),
    ]);
    let probe = discover::health(&client(), &stub.endpoint(ProviderKind::OpenAi)).await;

    assert_eq!(probe.reach, ProviderReach::Online);
    assert_eq!(probe.status, Some(200));
    assert_eq!(probe.version, None, "the dialect states no version");
    assert_eq!(probe.reason, None);
    assert_eq!(
        discover::health_state(&probe),
        keeper_core::providers::ProviderHealthState::Reachable
    );
    assert_eq!(stub.requests(), vec!["GET /v1/models".to_owned()]);
}

#[tokio::test]
async fn discover_openai_models_are_the_listed_ids_with_capabilities_unknown() {
    let stub = Stub::start(vec![
        route("/v1/models", OPENAI_MODELS),
        // Hermes' enrichment route, served with capabilities a regression
        // would wrongly merge in.
        route(
            "/api/model/options",
            r#"{"providers":[{"models":[{"id":"gpt-6-astra","capabilities":{"supports_tools":false}}]}]}"#,
        ),
    ]);
    let models = discover::models(&client(), &stub.endpoint(ProviderKind::OpenAi))
        .await
        .expect("the model list parses");

    let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["claude-opus-5-5", "gpt-6-astra", "gemini-3-pro"],
        "the endpoint's order, each id once, and no empty id"
    );
    for model in &models {
        assert_eq!(
            (model.vision, model.tools, model.reasoning, model.embedding),
            (None, None, None, None),
            "{}: /v1/models states no capability, so none is known",
            model.id
        );
        assert_eq!(model.context_window, None);
    }
    assert_eq!(stub.requests(), vec!["GET /v1/models".to_owned()]);
}

#[tokio::test]
async fn discover_openai_refusing_the_key_is_unauthorized_with_its_own_sentence() {
    let stub = Stub::start(vec![failing("/v1/models", 401)]);
    let client = client();

    let health = discover::health(&client, &stub.endpoint(ProviderKind::OpenAi)).await;
    assert_eq!(health.reach, ProviderReach::Online);
    assert_eq!(health.status, Some(401));
    assert_eq!(
        discover::health_state(&health),
        keeper_core::providers::ProviderHealthState::Unauthorized
    );
    assert_eq!(health.reason, Some(openai_refusal(401)));

    let err = discover::models(&client, &stub.endpoint(ProviderKind::OpenAi))
        .await
        .expect_err("a refused list is an error");
    assert!(!err.to_string().contains(TOKEN), "{err}");
    assert_eq!(
        stub.requests(),
        vec!["GET /v1/models".to_owned(); 2],
        "every refusal came from /v1/models, never a prefixed path"
    );
}
