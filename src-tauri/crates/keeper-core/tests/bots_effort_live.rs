//! An agent's reasoning level against a real OpenAI-compatible endpoint
//! (story 100.2 #2, R382): CLIProxyAPI, through keeper's own client, sent
//! `reasoning_effort: "xhigh"`, streams reasoning text back; the same request
//! without a level sends no such field.
//!
//! Ignored by default: it spends the endpoint owner's subscription. The
//! endpoint is named by the environment, never by this file:
//!
//! - `KEEPER_OPENAI_SMOKE_BASE_URL` — the endpoint's base URL;
//! - `KEEPER_OPENAI_SMOKE_TOKEN_FILE` (optional) — a file holding its bearer
//!   key, else `~/.omp/cliproxyapi.token`;
//! - `KEEPER_OPENAI_SMOKE_MODEL` (optional) — else `claude-opus-5-5`.
//!
//! Without the URL or the key the test says so and passes without sending a
//! byte. The key is read at run time and never printed: only the lengths of
//! what came back are.

use std::path::PathBuf;

use keeper_core::bots::chat::{
    build_body, cancellation, stream_chat, ChatEvent, ChatMessage, ChatOptions, ChatRequest,
    Effort, Role,
};
use keeper_core::bots::{http, parse_base_url, Endpoint, ProviderKind};

fn token_file() -> Option<PathBuf> {
    std::env::var_os("KEEPER_OPENAI_SMOKE_TOKEN_FILE")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".omp/cliproxyapi.token"))
        })
}

#[tokio::test]
#[ignore = "live: needs KEEPER_OPENAI_SMOKE_BASE_URL and a CLIProxyAPI key (100.2 #2)"]
async fn an_openai_endpoint_reasons_at_the_level_keeper_sends() {
    let Ok(base) = std::env::var("KEEPER_OPENAI_SMOKE_BASE_URL") else {
        eprintln!("skipped: KEEPER_OPENAI_SMOKE_BASE_URL is not set");
        return;
    };
    let Some(token) = token_file()
        .and_then(|file| std::fs::read_to_string(file).ok())
        .map(|text| text.trim().to_owned())
        .filter(|token| !token.is_empty())
    else {
        eprintln!("skipped: no CLIProxyAPI key to read");
        return;
    };
    let model = std::env::var("KEEPER_OPENAI_SMOKE_MODEL")
        .ok()
        .filter(|model| !model.trim().is_empty())
        .unwrap_or_else(|| "claude-opus-5-5".to_owned());
    let endpoint = Endpoint {
        kind: ProviderKind::OpenAi,
        base_url: parse_base_url(&base)
            .expect("the base URL is one keeper accepts")
            .normalized,
        bot: Some(model.clone()),
        token: Some(token),
    };
    let client = http::client(http::READ_TIMEOUT).expect("the chat client builds");

    let ask = |effort: Option<Effort>| ChatRequest {
        model: model.clone(),
        messages: vec![ChatMessage::text(
            Role::User,
            "A bat and a ball cost 1.10 together; the bat costs 1.00 more than the ball. \
             What does the ball cost? Answer with the number only.",
        )],
        effort,
        ..ChatRequest::default()
    };

    let none = build_body(ProviderKind::OpenAi, &ask(None)).expect("body");
    assert!(none.get("reasoning_effort").is_none(), "no level, no field");
    let xhigh = build_body(ProviderKind::OpenAi, &ask(Some(Effort::Xhigh))).expect("body");
    assert_eq!(xhigh["reasoning_effort"], "xhigh");

    let (_handle, signal) = cancellation();
    let mut reasoning_events = 0usize;
    let mut sink = |event: ChatEvent| {
        if matches!(event, ChatEvent::ReasoningDelta(_)) {
            reasoning_events += 1;
        }
    };
    let outcome = stream_chat(
        &client,
        &endpoint,
        &ask(Some(Effort::Xhigh)),
        &ChatOptions::default(),
        signal,
        &mut sink,
    )
    .await
    .expect("the endpoint answers at xhigh");
    eprintln!(
        "xhigh on {model}: {} chars of answer, {} chars of reasoning in {reasoning_events} deltas, model {:?}, {} ms",
        outcome.content.chars().count(),
        outcome.reasoning.chars().count(),
        outcome.model,
        outcome.total_ms
    );
    assert!(
        !outcome.reasoning.trim().is_empty(),
        "xhigh streams reasoning text back"
    );
    assert!(!outcome.content.trim().is_empty(), "and an answer");
}
