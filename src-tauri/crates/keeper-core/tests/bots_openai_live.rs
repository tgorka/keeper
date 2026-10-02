//! The `openai` provider kind against a real OpenAI-compatible endpoint
//! (AD-369, ruling R13): CLIProxyAPI, whose risk is the protocol conversion
//! behind it — a tool call or a stream terminator that does not survive the
//! trip from the upstream dialect to OpenAI's.
//!
//! Ignored by default: it spends the endpoint owner's subscription, so it runs
//! only when asked, with the endpoint and its key named by the environment —
//! never by this file:
//!
//! - `KEEPER_OPENAI_SMOKE_BASE_URL` — the endpoint's base URL;
//! - `KEEPER_OPENAI_SMOKE_TOKEN_FILE` — a file holding its bearer key;
//! - `KEEPER_OPENAI_SMOKE_MODEL` (optional) — the model to chat with, else the
//!   first listed model keeper would chat with;
//! - `KEEPER_OPENAI_SMOKE_PROMPT_FILE` (optional) — a composed system prompt
//!   whose `prompt_tokens` to measure, else a representative one built here.
//!
//! Four completions are sent, the fewest that prove the claims: three for a
//! tool loop with `tool_choice: "required"` (one round that reads, one forced
//! round answered as exhausted, one toolless answer) and one raw stream that
//! shows the wire's own terminator and usage frame. Every other request is a
//! `GET /v1/models`.
//!
//! The key is searched for, as text, in everything the run captured: every
//! `tracing` event and span field from any crate, every error and every line
//! this test printed.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use keeper_core::bots::chat::{
    build_body, cancellation, ChatEvent, ChatMessage, ChatOptions, ChatRequest, Role, ToolChoice,
};
use keeper_core::bots::grant::GrantMode;
use keeper_core::bots::quirks::quirks;
use keeper_core::bots::tools::{
    run_tool_loop, tool_specs, ToolCall, ToolHost, ToolLoop, ToolLoopEvent, ToolLoopOptions,
    ToolName, ToolOutcome,
};
use keeper_core::bots::{
    discover, http, parse_base_url, voice_target, Bot, BotHealthState, Endpoint, ProviderKind,
};
use keeper_core::vm::{BotPresence, BotReach};

/// The file the model must read, and the phrase its answer must quote.
const SMOKE_PATH: &str = "notes/keeper-smoke.md";
const SMOKE_PHRASE: &str = "QUARTZ-HERON-4417";

/// A model id no endpoint lists.
const ABSENT_MODEL: &str = "keeper-no-such-model";

/// A key the endpoint must refuse. Shaped like nothing real.
const WRONG_TOKEN: &str = "keeper-smoke-wrong-key-0000";

// ---------------------------------------------------------------------------
// Capturing every tracing event, so the key can be searched for
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<String>>);

impl Captured {
    fn push(&self, text: &str) {
        self.0.lock().expect("the capture lock").push_str(text);
    }

    fn text(&self) -> String {
        self.0.lock().expect("the capture lock").clone()
    }
}

struct FieldWriter<'a>(&'a mut String);

impl tracing::field::Visit for FieldWriter<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, "{}={value:?} ", field.name());
    }
}

/// A subscriber that writes every field of every span and event, at every
/// level, from every crate, into [`Captured`].
struct Capture {
    out: Captured,
    next_id: AtomicU64,
}

impl Capture {
    fn write(&self, target: &str, record: impl FnOnce(&mut FieldWriter<'_>)) {
        let mut line = format!("{target}: ");
        record(&mut FieldWriter(&mut line));
        line.push('\n');
        self.out.push(&line);
    }
}

impl tracing::Subscriber for Capture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        self.write(span.metadata().target(), |w| span.record(w));
        tracing::span::Id::from_u64(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    fn record(&self, _: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        self.write("span", |w| values.record(w));
    }

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        self.write(event.metadata().target(), |w| event.record(w));
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// Everything the run says, printed and kept.
struct Transcript(Captured);

impl Transcript {
    fn say(&self, line: String) {
        println!("{line}");
        self.0.push(&line);
        self.0.push("\n");
    }
}

// ---------------------------------------------------------------------------
// The in-memory drive
// ---------------------------------------------------------------------------

/// One profile holding one file: a listing names it, a read returns it, and
/// anything else is refused in the host's own words.
struct OneFile;

impl ToolHost for OneFile {
    fn run(&self, call: &ToolCall) -> Result<ToolOutcome, keeper_core::bots::error::BotsError> {
        let body = format!("# Smoke\n\nThe tide table reads: {SMOKE_PHRASE}.\n");
        Ok(match call.name {
            ToolName::Read if call.target.subpath == SMOKE_PATH => ToolOutcome::Text {
                of_bytes: Some(body.len() as u64),
                body,
                truncated_at: None,
                okf: None,
            },
            ToolName::List => ToolOutcome::Entries {
                subpath: call.target.subpath.clone(),
                entries: vec![keeper_core::bots::tools::EntryLine {
                    subpath: SMOKE_PATH.to_owned(),
                    is_dir: false,
                    bytes: Some(body.len() as u64),
                    is_virtual: false,
                }],
                truncated_at: None,
                of_entries: 1,
            },
            _ => ToolOutcome::Refused {
                reason: format!("Only {SMOKE_PATH} exists here; read it with drive_read."),
            },
        })
    }
}

// ---------------------------------------------------------------------------
// The prompt whose size is measured
// ---------------------------------------------------------------------------

/// A system prompt the size 89.3's golden is (memory at its caps, three
/// skills), for when no composed prompt is handed in.
fn representative_prompt() -> String {
    let entry = |n: usize| format!("Fact {n}: the person prefers short, sourced answers.");
    let capped = |cap: usize| {
        let mut text = String::new();
        let mut n = 0;
        loop {
            let next = entry(n);
            if text.chars().count() + next.chars().count() + 3 > cap {
                return text;
            }
            if !text.is_empty() {
                text.push_str("\n§\n");
            }
            text.push_str(&next);
            n += 1;
        }
    };
    let skills = ["research", "summarise", "plan"]
        .map(|name| {
            format!("- {name}: how this agent does {name} work, step by step, citing files.")
        })
        .join("\n");
    format!(
        "You are Nixi, the person's assistant in their drive.\n\n\
         ## Who you are\nCalm, exact, brief. You cite the file a fact came from.\n\n\
         ## What you remember\n{}\n\n## What you know about the person\n{}\n\n\
         ## Skills\n{skills}\n\n## This session\nHost: smoke. Readers: the person only.",
        capped(2200),
        capped(1375),
    )
}

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"))
}

#[tokio::test]
#[ignore = "live: needs KEEPER_OPENAI_SMOKE_BASE_URL and KEEPER_OPENAI_SMOKE_TOKEN_FILE (CLIProxyAPI, ruling R13)"]
async fn an_openai_endpoint_lists_answers_and_runs_keepers_tools() {
    let captured = Captured::default();
    tracing::subscriber::set_global_default(Capture {
        out: captured.clone(),
        next_id: AtomicU64::new(1),
    })
    .expect("the only subscriber in this binary");
    let say = Transcript(captured.clone());

    let base_url = parse_base_url(&env("KEEPER_OPENAI_SMOKE_BASE_URL"))
        .expect("the base URL is one keeper accepts")
        .normalized;
    let token = std::fs::read_to_string(env("KEEPER_OPENAI_SMOKE_TOKEN_FILE"))
        .expect("the token file reads")
        .trim()
        .to_owned();
    assert!(!token.is_empty(), "the token file is empty");
    let endpoint = |bot: Option<&str>, token: &str| Endpoint {
        kind: ProviderKind::OpenAi,
        base_url: base_url.clone(),
        bot: bot.map(str::to_owned),
        token: Some(token.to_owned()),
    };
    let client = discover::discovery_client().expect("the discovery client builds");

    // Health is one GET of /v1/models.
    let health = discover::health(&client, &endpoint(None, &token)).await;
    say.say(format!(
        "health: reach={:?} status={:?} reason={:?}",
        health.reach, health.status, health.reason
    ));
    assert_eq!(health.reach, BotReach::Online);
    assert_eq!(
        discover::health_state(&health),
        BotHealthState::Reachable,
        "{:?}",
        health.reason
    );

    // The models, and the one this run chats with.
    let models = discover::models(&client, &endpoint(None, &token))
        .await
        .expect("the endpoint lists its models");
    assert!(!models.is_empty(), "the endpoint lists no model");
    assert!(
        models
            .iter()
            .all(|m| (m.vision, m.tools, m.reasoning) == (None, None, None)),
        "/v1/models states no capability, so none is known"
    );
    let model = match std::env::var("KEEPER_OPENAI_SMOKE_MODEL") {
        Ok(model) if !model.trim().is_empty() => model.trim().to_owned(),
        _ => {
            let bot = Bot {
                id: "smoke".to_owned(),
                provider_id: "smoke".to_owned(),
                target: "smoke".to_owned(),
                name: "Smoke".to_owned(),
                pin_order: 0,
                identity: Default::default(),
                created_ms: 0,
            };
            voice_target::model_for(&bot, ProviderKind::OpenAi, &[], &models)
                .expect("the endpoint lists a model keeper would chat with")
        }
    };
    assert!(
        models.iter().any(|m| m.id == model),
        "{model} is not among the {} listed models",
        models.len()
    );
    say.say(format!(
        "models: {} listed; chatting with {model}",
        models.len()
    ));

    // Presence is membership in the list.
    let present = discover::probe_bot(&client, &endpoint(Some(&model), &token), &model).await;
    assert_eq!(
        present.presence,
        Some(BotPresence::Exists),
        "{:?}",
        present.reason
    );
    let absent =
        discover::probe_bot(&client, &endpoint(Some(ABSENT_MODEL), &token), ABSENT_MODEL).await;
    assert_eq!(
        absent.presence,
        Some(BotPresence::Absent),
        "{:?}",
        absent.reason
    );
    say.say(format!(
        "probe: {model} exists; {ABSENT_MODEL} absent ({:?})",
        absent.reason
    ));

    // A tool call survives the conversion, and the answer quotes the file.
    let chat_client = http::client(http::READ_TIMEOUT).expect("the chat client builds");
    let chat_endpoint = endpoint(Some(&model), &token);
    let request = ChatRequest {
        model: model.clone(),
        messages: vec![
            ChatMessage::text(
                Role::System,
                "You can read the person's drive with the drive tools. Answer from what you read.",
            ),
            ChatMessage::text(
                Role::User,
                format!(
                    "Call drive_read with path \"{SMOKE_PATH}\", then tell me the code the tide \
                     table reads, exactly as written."
                ),
            ),
        ],
        tools: tool_specs(GrantMode::Read),
        tool_choice: Some(ToolChoice::Required),
        ..ChatRequest::default()
    };
    let mut usage_seen = false;
    let mut events = Vec::new();
    let (_handle, cancel) = cancellation();
    let outcome = {
        let mut sink = |event: ToolLoopEvent| {
            if let ToolLoopEvent::Chat(ChatEvent::Usage(_)) = &event {
                usage_seen = true;
            }
            events.push(format!("{event:?}"));
        };
        run_tool_loop(
            &ToolLoop {
                client: &chat_client,
                endpoint: &chat_endpoint,
                host: &OneFile,
                default_profile_id: "smoke",
            },
            &request,
            &ChatOptions {
                max_attempts: 1,
                ..ChatOptions::default()
            },
            &ToolLoopOptions {
                max_rounds: 2,
                ..ToolLoopOptions::default()
            },
            cancel,
            &mut sink,
        )
        .await
    };
    for event in &events {
        captured.push(event);
        captured.push("\n");
    }
    let outcome = outcome.unwrap_or_else(|error| {
        captured.push(&error.to_string());
        panic!("the tool loop failed: {error}")
    });
    let reads: Vec<_> = outcome
        .calls
        .iter()
        .filter(|call| call.name == Some(ToolName::Read) && call.refusal.is_none())
        .collect();
    say.say(format!(
        "tool loop: {} rounds, calls {:?}, answer {:?}",
        outcome.rounds,
        outcome
            .calls
            .iter()
            .map(|call| (call.requested_name.as_str(), call.refusal.is_some()))
            .collect::<Vec<_>>(),
        outcome.final_outcome.content
    ));
    assert!(!reads.is_empty(), "no drive_read ran: {:?}", outcome.calls);
    assert!(
        outcome.final_outcome.content.contains(SMOKE_PHRASE),
        "the answer does not quote the file: {:?}",
        outcome.final_outcome.content
    );

    // The wire's own terminator and usage frame, and the prompt's size.
    let (prompt, prompt_source) = match std::env::var("KEEPER_OPENAI_SMOKE_PROMPT_FILE") {
        Ok(path) if !path.trim().is_empty() => (
            std::fs::read_to_string(path.trim()).expect("the prompt file reads"),
            "composed",
        ),
        _ => (representative_prompt(), "representative"),
    };
    let body = build_body(
        ProviderKind::OpenAi,
        &ChatRequest {
            model: model.clone(),
            messages: vec![
                ChatMessage::text(Role::System, prompt.clone()),
                ChatMessage::text(Role::User, "Reply with the single word OK."),
            ],
            max_tokens: Some(16),
            ..ChatRequest::default()
        },
    )
    .expect("the body builds");
    let raw = http::authorize(
        chat_client.post(chat_endpoint.url("/v1/chat/completions")),
        Some(&token),
    )
    .expect("the request authorizes")
    .json(&body)
    .send()
    .await
    .expect("the endpoint answers the raw stream");
    let status = raw.status();
    let text = raw.text().await.expect("the raw stream reads");
    assert!(status.is_success(), "the raw stream answered {status}");
    let frames: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .collect();
    let done = frames.last() == Some(&"[DONE]");
    let prompt_tokens = frames
        .iter()
        .filter_map(|frame| serde_json::from_str::<serde_json::Value>(frame).ok())
        .find_map(|frame| frame["usage"]["prompt_tokens"].as_u64());
    say.say(format!(
        "wire: [DONE] {}; usage frame {}; tool-loop usage {}",
        if done { "observed" } else { "NOT observed" },
        if prompt_tokens.is_some() {
            "observed"
        } else {
            "NOT observed"
        },
        if usage_seen {
            "observed"
        } else {
            "NOT observed"
        },
    ));
    say.say(format!(
        "prompt: {prompt_source}, {} chars, prompt_tokens={prompt_tokens:?} (against \
         OLLAMA_CONTEXT_LENGTH=16384)",
        prompt.chars().count()
    ));
    let row = quirks(ProviderKind::OpenAi);
    say.say(format!(
        "quirks row: done_sentinel={:?} stream_usage={:?}",
        row.done_sentinel, row.stream_usage
    ));

    // A wrong key is refused, in the kind's own sentence.
    let refused = discover::health(&client, &endpoint(None, WRONG_TOKEN)).await;
    say.say(format!(
        "wrong key: status={:?} reason={:?}",
        refused.status, refused.reason
    ));
    assert_eq!(
        discover::health_state(&refused),
        BotHealthState::Unauthorized
    );
    let status = refused.status.unwrap_or_default();
    assert_eq!(
        refused.reason,
        Some(format!(
            "The endpoint refused keeper's key ({status}). Check the key this provider was saved \
             with."
        ))
    );

    // The key never appears anywhere the run wrote.
    let everything = captured.text();
    assert!(
        !everything.contains(&token),
        "the key appears in the captured output"
    );
    assert!(
        !everything.contains(WRONG_TOKEN),
        "the wrong key appears in the captured output"
    );
    println!(
        "captured {} bytes; the key is not among them",
        everything.len()
    );
}
