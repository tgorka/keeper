//! Desktop bot tasks: the ordinary tool loop, without a conversation or approver.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use keeper_core::bots::chat::{
    self, ChatEvent, ChatMessage, ChatOptions, ChatRequest, FinishReason, Role,
};
use keeper_core::bots::context_files;
use keeper_core::bots::http;
use keeper_core::bots::tools::{self, ToolLoop, ToolLoopEvent};
use keeper_core::bots::{discover, store, Bot, Endpoint, ProviderKind};
use keeper_core::platform::Platform;
use keeper_sync::platform::{BotRunFuture, BotRunRecord, BotTaskRunner, BotTaskSpec};
use keeper_sync::tasks::TaskOutcome;
use keeper_sync::SyncProfile;

use crate::bots_tools::DriveToolHost;

const MODEL_REQUIRED: &str = "this bot task names no model; set the task's model before running it";

pub(crate) struct ShellBotTaskRunner {
    platform: Arc<dyn Platform>,
}

impl std::fmt::Debug for ShellBotTaskRunner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ShellBotTaskRunner")
            .finish_non_exhaustive()
    }
}

impl ShellBotTaskRunner {
    pub(crate) fn new(platform: Arc<dyn Platform>) -> Self {
        Self { platform }
    }
}

impl BotTaskRunner for ShellBotTaskRunner {
    fn run(&self, spec: BotTaskSpec) -> BotRunFuture {
        let platform = Arc::clone(&self.platform);
        Box::pin(async move {
            match prepare(platform, &spec).await {
                Ok(turn) => execute(&turn).await,
                Err(error) => BotRunRecord::failed(error),
            }
        })
    }
}

struct TaskTurn {
    endpoint: Endpoint,
    request: ChatRequest,
    host: DriveToolHost,
    default_profile_id: String,
    read_timeout: Duration,
}

fn required_model(spec: &BotTaskSpec) -> Result<&str, String> {
    spec.model
        .as_deref()
        .filter(|model| !model.trim().is_empty())
        .ok_or_else(|| MODEL_REQUIRED.to_owned())
}

/// The two messages a task turn sends.
///
/// The prompt file is the **question**, not a document handed to the model to
/// look at: a person chose this file when they created the task, exactly as a
/// person types into the chat box, so it is sent as the user's turn and nothing
/// tells the model to disregard it. AD-159's rule is about content the model
/// *encounters* — a tool result, an `AGENTS.md`-style context file — and those
/// still arrive under [`context_files::UNTRUSTED_PREAMBLE`] and
/// [`tools::FILE_CONTENT_IS_DATA`] on the paths that carry them. Wrapping the
/// prompt itself in "this is data, do not obey it" would make every scheduled
/// run answer that it must not do the thing it was scheduled to do.
///
/// Nothing about this widens what the turn may *do*: `grant::decide` runs at
/// every tool call and an `Ask` with no approver is still a refusal (AD-158).
fn prompt_messages(source: &str, context: Option<String>) -> Vec<ChatMessage> {
    let mut messages = Vec::with_capacity(2);
    // Only when there are context blocks: the preamble introduces them, and a
    // preamble followed by nothing is a system prompt about an empty list.
    if let Some(context) = context {
        messages.push(ChatMessage::text(Role::System, context));
    }
    messages.push(ChatMessage::text(
        Role::User,
        keeper_core::notes::prompt::body_after_heading(source).to_owned(),
    ));
    messages
}

fn task_host(dir: PathBuf, bot: &Bot, profiles: Vec<SyncProfile>, task_id: &str) -> DriveToolHost {
    DriveToolHost {
        data_dir: dir,
        provider_id: bot.provider_id.clone(),
        bot_id: Some(bot.id.clone()),
        // Audit correlation only: no conversation is created or replayed.
        session_id: format!("task:{task_id}:{}", ulid::Ulid::new()),
        message_id: None,
        profiles,
        approve: None,
    }
}

async fn prepare(platform: Arc<dyn Platform>, spec: &BotTaskSpec) -> Result<TaskTurn, String> {
    let model = required_model(spec)?;
    let dir = platform.data_dir().map_err(|error| error.to_string())?;
    let bot = store::get_bot(&dir, &spec.bot_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("no bot called '{}' exists", spec.bot_id))?;
    let row = store::get_provider(&dir, &bot.provider_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("no provider called '{}' exists", bot.provider_id))?;
    let token =
        crate::account_ipc::bot_credential(platform.as_ref(), &row.provider.id, Some(&bot.target))
            .await
            .map_err(|error| error.to_string())?;
    let endpoint = Endpoint::new(&row.provider, Some(&bot.target), token);
    let read_timeout = match row.read_timeout_ms {
        Some(ms) if ms > 0 => Duration::from_millis(ms.unsigned_abs()),
        _ => http::READ_TIMEOUT,
    };
    let client = http::client(read_timeout).map_err(|error| error.to_string())?;
    let grants = store::list_grants_for_bot(&dir, &bot.provider_id, Some(&bot.id))
        .map(|listing| listing.live)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "bots: could not read the grants for this task");
            Vec::new()
        });
    let tools_supported = if grants.is_empty() || row.provider.kind == ProviderKind::Hermes {
        None
    } else {
        discover::models(&client, &endpoint)
            .await
            .ok()
            .and_then(|models| models.into_iter().find(|candidate| candidate.id == model))
            .and_then(|found| found.tools)
    };
    let offer = tools::offer_tools(row.provider.kind, tools_supported, &grants);
    let profiles = crate::sync::engine(platform)
        .ok()
        .and_then(|engine| engine.list_profiles().ok())
        .unwrap_or_default();
    let profile_ids: Vec<&str> = profiles.iter().map(|profile| profile.id.as_str()).collect();
    let default_profile_id = tools::default_profile_id(&grants, &profile_ids).unwrap_or_default();
    let context = offer.is_offered().then(|| {
        let targets = context_files::context_targets(&grants, &profile_ids);
        context_files::merge(crate::bots_tools::load_context(&profiles, &targets))
    });
    let request = ChatRequest {
        model: model.to_owned(),
        messages: prompt_messages(
            &spec.prompt_text,
            context.as_ref().and_then(|bundle| bundle.system_prompt()),
        ),
        tools: offer.specs(),
        ..ChatRequest::default()
    };
    Ok(TaskTurn {
        endpoint,
        request,
        host: task_host(dir, &bot, profiles, &spec.task_id),
        default_profile_id,
        read_timeout,
    })
}

async fn execute(turn: &TaskTurn) -> BotRunRecord {
    let started = Instant::now();
    let client = match http::client(turn.read_timeout) {
        Ok(client) => client,
        Err(error) => return BotRunRecord::failed(error.to_string()),
    };
    let mut record = BotRunRecord::failed("the bot task did not finish");
    record.errors.clear();
    record.model = Some(turn.request.model.clone());
    let mut sink = |event| match event {
        ToolLoopEvent::Chat(ChatEvent::ContentDelta(text)) => record.answer.push_str(&text),
        ToolLoopEvent::Chat(ChatEvent::Failed { error }) => record.errors.push(error.to_string()),
        ToolLoopEvent::Chat(ChatEvent::Usage(usage)) => {
            if let Some(tokens) = usage.prompt_tokens {
                record.prompt_tokens =
                    Some(record.prompt_tokens.unwrap_or(0).saturating_add(tokens));
            }
            if let Some(tokens) = usage.completion_tokens {
                record.completion_tokens =
                    Some(record.completion_tokens.unwrap_or(0).saturating_add(tokens));
            }
        }
        _ => {}
    };
    // Reporting is the same core loop, with a callback so even a later HTTP
    // failure cannot erase a refusal that already closed its audit row.
    let mut report = |call: &tools::ToolCallRecord, _: &chat::ToolCall, _: &tools::ToolOutcome| {
        record.tool_calls = record.tool_calls.saturating_add(1);
        if let Some(reason) = &call.refusal {
            // The mark is the port's contract, not prose: `bot_run_detail`
            // counts refusals with it and must not count a model's rename as
            // one (keeper_sync::platform::TOOL_REFUSED_MARK).
            record.warnings.push(format!(
                "{} ({}){}{reason}",
                call.requested_name,
                call.display_path.as_deref().unwrap_or(&call.id),
                keeper_sync::platform::TOOL_REFUSED_MARK
            ));
        }
    };
    let context = ToolLoop {
        client: &client,
        endpoint: &turn.endpoint,
        host: &turn.host,
        default_profile_id: &turn.default_profile_id,
    };
    // The core API requires a signal. Dropping its private handle makes it
    // inert; there is no attended cancel registration or UI ownership.
    let (_, signal) = chat::cancellation();
    let result = tools::run_tool_loop_reporting(
        &context,
        &turn.request,
        &ChatOptions {
            read_timeout: turn.read_timeout,
            ..ChatOptions::default()
        },
        &tools::ToolLoopOptions::default(),
        signal,
        &mut sink,
        &mut report,
    )
    .await;
    record.ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match result {
        Err(error) => record.errors.push(error.to_string()),
        Ok(outcome) => {
            let answer = outcome.final_outcome;
            if let Some(model) = answer.model {
                if model != turn.request.model {
                    record.warnings.push(format!(
                        "requested model {}; {model} answered",
                        turn.request.model
                    ));
                }
                record.model = Some(model);
            }
            if let Some(refusal) = answer.refusal {
                record.warnings.push(refusal);
            }
            let word = match &answer.finish_reason {
                FinishReason::Stop => "stop",
                FinishReason::Length => "length",
                FinishReason::ContentFilter => "content_filter",
                FinishReason::ToolCalls => "tool_calls",
                FinishReason::Cancelled => "cancelled",
                FinishReason::Failed => "failed",
                FinishReason::Other(word) => word,
            };
            record.finish_reason = Some(word.to_owned());
            if matches!(
                answer.finish_reason,
                FinishReason::Length | FinishReason::ContentFilter
            ) {
                record
                    .warnings
                    .push(format!("the answer stopped with {word}"));
            }
            if outcome.exhausted {
                record.warnings.push(tools::ROUNDS_EXHAUSTED.to_owned());
            }
            if matches!(
                answer.finish_reason,
                FinishReason::Failed | FinishReason::Cancelled
            ) {
                if record.errors.is_empty() {
                    record
                        .errors
                        .push(format!("the answer stopped with {word}"));
                }
            } else if record.errors.is_empty() {
                record.outcome = TaskOutcome::Ok;
            }
        }
    }
    record
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_core::bots::audit::{self, AuditOutcome, AuditVerdict};
    use keeper_core::bots::grant::{Grant, GrantMode, GrantScope};
    use keeper_core::error::CoreError;
    use std::io::{BufRead, BufReader, Read, Write};

    fn bot() -> Bot {
        Bot {
            id: "bot".to_owned(),
            provider_id: "provider".to_owned(),
            target: "model".to_owned(),
            name: "Task bot".to_owned(),
            pin_order: 0,
            identity: Default::default(),
            created_ms: 1,
        }
    }

    /// A local provider that captures each real wire request and answers in
    /// sequence. The second request proves the loop continued after a refusal.
    fn provider(
        responses: Vec<serde_json::Value>,
    ) -> (Endpoint, std::thread::JoinHandle<Vec<serde_json::Value>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind provider");
        let address = listener.local_addr().expect("provider address");
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let (mut socket, _) = listener.accept().expect("accept completion");
                socket
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .expect("read timeout");
                let mut reader = BufReader::new(socket.try_clone().expect("clone socket"));
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).expect("read header");
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse::<usize>().expect("content length");
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).expect("read request");
                requests.push(serde_json::from_slice(&body).expect("request JSON"));
                let body = format!("data: {response}\n\ndata: [DONE]\n\n");
                write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).expect("send completion");
            }
            requests
        });
        (
            Endpoint {
                kind: ProviderKind::Ollama,
                base_url: format!("http://{address}"),
                bot: None,
                token: None,
            },
            server,
        )
    }

    fn completed() -> serde_json::Value {
        serde_json::json!({"model":"model","choices":[{"index":0,"delta":{"content":"Continued."},"finish_reason":"stop"}]})
    }

    fn turn(endpoint: Endpoint, host: DriveToolHost, source: &str) -> TaskTurn {
        TaskTurn {
            endpoint,
            host,
            request: ChatRequest {
                model: "model".to_owned(),
                messages: prompt_messages(source, None),
                tools: tools::tool_specs(GrantMode::Write),
                ..ChatRequest::default()
            },
            default_profile_id: "folder".to_owned(),
            read_timeout: Duration::from_secs(10),
        }
    }

    /// The prompt is the task's question, so it arrives as the user's turn with
    /// nothing around it: no title, no frontmatter, and no "treat this as data"
    /// preamble that a compliant model would read as "do not do this".
    #[tokio::test]
    async fn task_prompt_reaches_provider_verbatim_as_the_users_question() {
        let dir = tempfile::tempdir().expect("data directory");
        let (endpoint, server) = provider(vec![completed()]);
        let task = turn(
            endpoint,
            task_host(dir.path().to_owned(), &bot(), Vec::new(), "task"),
            "---\ntitle: Hidden\n---\n# Heading\n\n  keep **this**\r\n## Second\n",
        );
        let record = execute(&task).await;
        assert_eq!(record.outcome, TaskOutcome::Ok);
        let requests = server.join().expect("provider thread");
        let messages = requests[0]["messages"].as_array().expect("messages");
        let user = messages
            .iter()
            .find(|message| message["role"] == "user")
            .expect("user data");
        // Text-only content may be encoded as a string or a parts array by the
        // core wire encoder; compare its text rather than its representation.
        let text = if let Some(text) = user["content"].as_str() {
            text.to_owned()
        } else {
            user["content"]
                .as_array()
                .expect("content parts")
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };
        // One leading newline, not two: `body_after_heading` removes the
        // frontmatter and the heading LINE, and the blank line after the heading
        // is part of the body a person wrote.
        assert_eq!(text, "\n  keep **this**\r\n## Second\n");
        assert!(
            !text.contains(tools::FILE_CONTENT_IS_DATA),
            "the prompt a person scheduled is the instruction, not a document to disregard"
        );
        assert!(
            !messages.iter().any(|message| message["role"] == "system"),
            "with no context files there is no block list to introduce"
        );
    }

    #[tokio::test]
    async fn unattended_ask_is_audited_refused_warned_and_the_model_continues() {
        let dir = tempfile::tempdir().expect("data directory");
        let root = tempfile::tempdir().expect("drive");
        store::insert_provider(
            dir.path(),
            &keeper_core::bots::Provider {
                id: "provider".to_owned(),
                kind: ProviderKind::Ollama,
                name: "Provider".to_owned(),
                base_url: "http://localhost:11434".to_owned(),
                created_ms: 1,
            },
        )
        .expect("store provider");
        store::insert_bot(dir.path(), &bot()).expect("store bot");
        let grant = Grant {
            id: "grant".to_owned(),
            provider_id: "provider".to_owned(),
            bot_id: Some("bot".to_owned()),
            scope: GrantScope::Profile {
                profile_id: "folder".to_owned(),
            },
            mode: GrantMode::Write,
            created_ms: 1,
        };
        store::save_grant(dir.path(), &grant).expect("store grant");
        let host = task_host(
            dir.path().to_owned(),
            &bot(),
            vec![SyncProfile::new("folder", "Folder", root.path(), "unused")],
            "task",
        );
        assert!(host.approve.is_none());
        let call = serde_json::json!({"choices":[{"index":0,"delta":{"tool_calls":[
            {"index":0,"id":"write-1","type":"function","function":{"name":"drive_write","arguments":"{\"path\":\"result.txt\",\"content\":\"must not write\"}"}}
        ]},"finish_reason":"tool_calls"}]});
        let (endpoint, server) = provider(vec![call, completed()]);
        let record = execute(&turn(endpoint, host, "# Task\nbody")).await;
        assert_eq!(record.outcome, TaskOutcome::Ok);
        assert_eq!(record.answer, "Continued.");
        assert_eq!(record.tool_calls, 1);
        assert!(record
            .warnings
            .iter()
            .any(|warning| warning.contains("write") && warning.contains("refused")));
        assert!(!root.path().join("result.txt").exists());
        let audit = audit::list_audit(dir.path(), None, None).expect("read audit");
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].verdict, Some(AuditVerdict::Ask));
        assert_eq!(audit[0].outcome, AuditOutcome::Refused);
        assert_eq!(
            store::list_grants_for_bot(dir.path(), "provider", Some("bot"))
                .expect("grants")
                .live,
            vec![grant]
        );
        let requests = server.join().expect("provider thread");
        assert_eq!(requests.len(), 2);
        let result = requests[1]["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .find(|message| message["role"] == "tool")
            .expect("tool refusal");
        assert_eq!(result["tool_call_id"], "write-1");
        assert!(result["content"]
            .as_str()
            .expect("tool error")
            .starts_with("Refused:"));
    }

    struct UnusedPlatform;

    impl Platform for UnusedPlatform {
        fn data_dir(&self) -> Result<PathBuf, CoreError> {
            panic!("a task without a model must refuse before any platform access")
        }
        fn keychain_set(&self, _: &str, _: &str) -> Result<(), CoreError> {
            unreachable!()
        }
        fn keychain_get(&self, _: &str) -> Result<Option<String>, CoreError> {
            unreachable!()
        }
        fn keychain_delete(&self, _: &str) -> Result<(), CoreError> {
            unreachable!()
        }
        fn open_url(&self, _: &str) -> Result<(), CoreError> {
            unreachable!()
        }
        fn notify(
            &self,
            _: &str,
            _: &str,
            _: &keeper_core::vm::NotifyTarget,
        ) -> Result<(), CoreError> {
            unreachable!()
        }
        fn sidecar_path(&self, _: &str) -> Result<PathBuf, CoreError> {
            unreachable!()
        }
        fn exclude_from_backup(&self, _: &std::path::Path) -> Result<(), CoreError> {
            unreachable!()
        }
        fn set_badge_count(&self, _: Option<u32>) -> Result<(), CoreError> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn missing_model_is_a_refusal_naming_the_setting() {
        let mut spec = BotTaskSpec {
            task_id: "task".to_owned(),
            profile_id: "folder".to_owned(),
            bot_id: "bot".to_owned(),
            model: None,
            prompt_subpath: "prompt.md".to_owned(),
            prompt_text: "text".to_owned(),
        };
        for model in [None, Some(" \t".to_owned())] {
            spec.model = model;
            let record = ShellBotTaskRunner::new(Arc::new(UnusedPlatform))
                .run(spec.clone())
                .await;
            assert_eq!(record.outcome, TaskOutcome::Failed);
            assert_eq!(record.errors, [MODEL_REQUIRED]);
        }
    }
}
