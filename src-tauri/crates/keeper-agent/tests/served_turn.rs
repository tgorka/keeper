//! An agent's turn served with no room: the stream a host's sink hears, and
//! the agent's `[limits]` held, against an OpenAI-shaped streaming stub.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_agent::ports::{TurnEnd, TurnSink};
use keeper_agent::served::{self, PersonMessage, ServedTurn};
use keeper_core::agents::home::Limits;
use keeper_core::bots::chat::{self, ChatMessage, ChatRequest, Role, ToolSpec};
use keeper_core::bots::error::BotsError;
use keeper_core::bots::tools::{ToolCall, ToolHost, ToolOutcome};
use keeper_core::bots::{Endpoint, ProviderKind};
use keeper_core::vm::BotStreamEvent;
use serde_json::{json, Value};

/// Serves each POST the next queued list of SSE chunks; records each body.
fn stub(replies: Vec<Vec<Value>>) -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind stub");
    let base = format!("http://{}", listener.local_addr().expect("stub address"));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    let mut queue: VecDeque<Vec<Value>> = replies.into();
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            let Ok(mut socket) = socket else { continue };
            let mut reader = BufReader::new(socket.try_clone().expect("clone socket"));
            let mut length = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() || line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap_or(0);
                }
            }
            let mut body = vec![0; length];
            let _ = reader.read_exact(&mut body);
            seen.lock()
                .expect("requests")
                .push(serde_json::from_slice(&body).unwrap_or(Value::Null));
            let chunks = queue.pop_front().unwrap_or_default();
            let mut text: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
            text.push_str("data: [DONE]\n\n");
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                text.len()
            );
        }
    });
    (base, requests)
}

fn delta(text: &str) -> Value {
    json!({"model":"stub-model","choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]})
}

fn thought(text: &str) -> Value {
    json!({"model":"stub-model","choices":[{"index":0,"delta":{"reasoning_content":text},"finish_reason":null}]})
}

fn finish(reason: &str, prompt: u32, completion: u32) -> Value {
    json!({"id":"resp-1","model":"stub-model","choices":[{"index":0,"delta":{},"finish_reason":reason}],
        "usage":{"prompt_tokens":prompt,"completion_tokens":completion,"total_tokens":prompt + completion}})
}

fn call(id: &str) -> Value {
    json!({"model":"stub-model","choices":[{"index":0,"delta":{"tool_calls":[
        {"index":0,"id":id,"type":"function","function":{"name":"clock","arguments":"{}"}}
    ]},"finish_reason":"tool_calls"}]})
}

/// A host serving one tool outside the drive's verbs, counting its runs.
#[derive(Default)]
struct Clock(Mutex<usize>);

impl ToolHost for Clock {
    fn run(&self, _call: &ToolCall) -> Result<ToolOutcome, BotsError> {
        Err(BotsError::Tool {
            detail: "no drive here".to_owned(),
        })
    }
    fn run_named(&self, wire: &chat::ToolCall) -> Option<ToolOutcome> {
        (wire.name == "clock").then(|| {
            *self.0.lock().expect("runs") += 1;
            ToolOutcome::Answered {
                text: "12:00".to_owned(),
            }
        })
    }
}

#[derive(Default)]
struct Heard {
    events: Mutex<Vec<Value>>,
    ended: Mutex<Option<TurnEnd>>,
}

impl TurnSink for Heard {
    fn event(&self, event: BotStreamEvent) -> bool {
        self.events
            .lock()
            .expect("events")
            .push(serde_json::to_value(event).expect("event JSON"));
        true
    }
    fn ended(&self, end: TurnEnd) {
        *self.ended.lock().expect("ended") = Some(end);
    }
}

impl Heard {
    fn kinds(&self) -> Vec<String> {
        self.events
            .lock()
            .expect("events")
            .iter()
            .map(|e| e["kind"].as_str().unwrap_or_default().to_owned())
            .collect()
    }
    fn of(&self, kind: &str) -> Vec<Value> {
        self.events
            .lock()
            .expect("events")
            .iter()
            .filter(|e| e["kind"] == kind)
            .cloned()
            .collect()
    }
}

fn turn(base: &str, limits: Limits) -> ServedTurn {
    ServedTurn {
        session_id: "c1.1".to_owned(),
        title: "time".to_owned(),
        agent_id: "nixie".to_owned(),
        provider_id: "llm".to_owned(),
        turn_id: "m-1".to_owned(),
        person: PersonMessage {
            id: "m-1".to_owned(),
            seq: 0,
            text: "what time is it?".to_owned(),
            created_ms: 1,
        },
        endpoint: Endpoint {
            kind: ProviderKind::OpenAi,
            base_url: base.to_owned(),
            bot: None,
            token: None,
        },
        read_timeout: Duration::from_secs(10),
        request: ChatRequest {
            model: "stub-model".to_owned(),
            messages: vec![
                ChatMessage::text(Role::System, "You are Nixie."),
                ChatMessage::text(Role::User, "what time is it?"),
            ],
            tools: vec![ToolSpec {
                name: "clock".to_owned(),
                description: "The time.".to_owned(),
                parameters: json!({"type":"object","properties":{}}),
            }],
            ..ChatRequest::default()
        },
        limits,
        default_profile_id: String::new(),
    }
}

fn limits(rounds_per_turn: u32, tokens_per_turn: u64) -> Limits {
    Limits {
        rounds_per_turn,
        tokens_per_turn,
        tokens_per_delegation: 200_000,
        hop_limit: 3,
        rounds_per_exchange: 3,
        max_concurrent_sessions: 4,
    }
}

#[tokio::test]
async fn a_turn_with_a_tool_call_streams_in_order_and_closes_with_the_whole_turns_metadata() {
    let (base, requests) = stub(vec![
        vec![thought("look it up"), delta("Let me check."), call("c-1")],
        vec![delta("It is "), delta("noon."), finish("stop", 30, 5)],
    ]);
    let host = Clock::default();
    let heard = Heard::default();
    let (_cancel, signal) = chat::cancellation();

    let end = served::serve_turn(turn(&base, limits(8, 0)), &host, &heard, signal).await;

    assert_eq!(end, TurnEnd::Complete);
    assert_eq!(*heard.ended.lock().expect("ended"), Some(TurnEnd::Complete));
    assert_eq!(*host.0.lock().expect("runs"), 1, "the call ran once");
    assert_eq!(requests.lock().expect("requests").len(), 2);
    let kinds = heard.kinds();
    assert_eq!(kinds.first().map(String::as_str), Some("opened"));
    assert_eq!(kinds.last().map(String::as_str), Some("closed"));
    assert_eq!(
        heard.of("firstToken").len(),
        1,
        "once per answer, not per round"
    );
    let at = |kind: &str| kinds.iter().position(|k| k == kind).expect(kind);
    assert!(at("toolCall") < at("toolResult"));
    assert_eq!(heard.of("reasoning")[0]["text"], "look it up");
    let streamed: String = heard
        .of("delta")
        .iter()
        .map(|d| d["text"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(streamed, "Let me check.\n\nIt is noon.");

    let opened = &heard.of("opened")[0];
    assert_eq!(opened["subscriptionId"], "m-1");
    assert_eq!(opened["user"]["content"], "what time is it?");
    assert_eq!(opened["assistant"]["partial"], true);
    let closed = &heard.of("closed")[0];
    assert_eq!(closed["reason"], Value::Null);
    let message = &closed["message"];
    assert_eq!(message["content"], streamed);
    assert_eq!(message["model"], "stub-model");
    assert_eq!(message["finishReason"], "stop");
    assert_eq!(message["toolCallCount"], 1);
    assert_eq!(message["partial"], false);
    assert_eq!(message["promptTokens"], 30);
    assert!(message["ttftMs"].is_number() && message["durationMs"].is_number());
}

#[tokio::test]
async fn the_agents_rounds_per_turn_bound_the_loop() {
    // One round may call tools; the next is sent with none on offer.
    let (base, requests) = stub(vec![
        vec![call("c-1")],
        vec![delta("Done."), finish("stop", 1, 1)],
    ]);
    let host = Clock::default();
    let heard = Heard::default();
    let (_cancel, signal) = chat::cancellation();

    served::serve_turn(turn(&base, limits(1, 0)), &host, &heard, signal).await;

    let requests = requests.lock().expect("requests");
    assert_eq!(requests.len(), 2);
    assert!(requests[0]["tools"].is_array());
    assert!(requests[1].get("tools").is_none_or(Value::is_null));
}

#[tokio::test]
async fn a_round_past_the_agents_token_budget_is_not_sent() {
    let (base, requests) = stub(vec![
        vec![
            delta("Checking."),
            call("c-1"),
            finish("tool_calls", 90, 20),
        ],
        vec![delta("never sent"), finish("stop", 1, 1)],
    ]);
    let host = Clock::default();
    let heard = Heard::default();
    let (_cancel, signal) = chat::cancellation();

    let end = served::serve_turn(turn(&base, limits(8, 100)), &host, &heard, signal).await;

    assert_eq!(requests.lock().expect("requests").len(), 1);
    let closed = &heard.of("closed")[0];
    assert_eq!(closed["reason"], keeper_core::agents::helper::TURN_SPENT);
    assert_eq!(closed["message"]["partial"], true);
    assert_eq!(closed["message"]["content"], "Checking.");
    assert_eq!(
        end,
        TurnEnd::Failed(keeper_core::agents::helper::TURN_SPENT.to_owned())
    );
}
