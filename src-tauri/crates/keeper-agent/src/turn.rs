//! Opening a turn (FR-372, FR-373): the rows, the replay, the grants, the
//! request — everything that happens before the first byte goes to a model.
//!
//! **No decisions live here.** What goes on offer, which drive files the model
//! is told about, the continuity header, the title and the replay rule are
//! `keeper_core::bots`'; this module gathers the facts they need and stores
//! what they decide.
//!
//! # The stream contract
//!
//! [`open_turn`] persists the assistant row **before** the request goes out,
//! marked partial, and [`crate::drive`] rewrites it as deltas land. So the
//! record on disk is never more than one flush behind the record on screen,
//! and a stream that dies — a dropped socket, a pressed Stop, a killed
//! process — leaves a row marked partial rather than leaving nothing.
//!
//! # Where the turn came from
//!
//! [`TurnOrigin`] is asked of the caller at the moment arming reaches it, not
//! at entry: a spoken turn is the voice turn's own answer about whether it is
//! waiting for a send, and that answer is read after the endpoint, the
//! identity probe and the row writes, exactly where it always was.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use keeper_core::bots::chat::{ChatMessage, ChatRequest, Role};
use keeper_core::bots::context_files::ContextBundle;
use keeper_core::bots::deliverable;
use keeper_core::bots::remote::{self, CapabilityCache, SessionCapabilities};
use keeper_core::bots::tools::{self, ToolOffer};
use keeper_core::bots::{discover, http, session, store, Bot, Endpoint};
use keeper_core::error::CoreError;
use keeper_core::org_account::descriptor::AccountDescriptor;
use keeper_core::org_account::AccountError;
use keeper_core::platform::Platform;
use keeper_core::vm::{
    BotChatSendReq, BotMessageVm, BotModelVm, BotRetryReq, BotSessionVm, BotStreamEvent,
};
use keeper_core::voice::speech;
use keeper_sync::SyncProfile;

use crate::host::{self, ArmedDrive, TurnHost};
use crate::ports::{ApprovalPort, ProfileSource, VaultWriter};

/// Why a turn could not be opened. The shell maps each arm onto the error
/// code the same failure always carried.
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// A store or platform failure.
    #[error(transparent)]
    Core(#[from] CoreError),
    /// The credential could not be had.
    #[error(transparent)]
    Account(#[from] AccountError),
    /// An id that names nothing.
    #[error("no such {what}: {id}")]
    NoSuch {
        /// What kind of row.
        what: &'static str,
        /// The id.
        id: String,
    },
    /// A request this device cannot make.
    #[error("{0}")]
    Unsupported(String),
    /// A request keeper refuses, with the sentence.
    #[error("{0}")]
    Refused(String),
}

/// Where a turn came from, which decides what the request carries beside the
/// conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOrigin {
    /// Typed into the pane.
    Typed,
    /// Heard by the voice turn: answered in `language`, aloud (AD-182).
    Spoken {
        /// The listening locale in force.
        language: String,
    },
    /// A scheduled bot task (AD-224): no conversation, no person.
    Task,
}

/// The configured account, for a provider set to "Use my account" (AD-315).
#[derive(Clone)]
pub struct AccountCredential {
    /// The account.
    pub descriptor: AccountDescriptor,
    /// Its HTTP client, or the sentence for why there is none.
    pub http: Result<reqwest::Client, String>,
}

/// The drive half of a host: absent where there is no drive.
#[derive(Clone)]
pub struct DrivePorts {
    /// Which profiles a tool call may name.
    pub profiles: Arc<dyn ProfileSource>,
    /// How a write lands in a notes vault; `None` routes every write as
    /// unmanaged.
    pub vault: Option<Arc<dyn VaultWriter>>,
    /// Who approves; `None` refuses every ask with
    /// [`crate::host::UNATTENDED_REFUSAL`].
    pub approval: Option<Arc<dyn ApprovalPort>>,
}

/// Everything a turn needs from the process it runs in.
#[derive(Clone)]
pub struct TurnEnv {
    /// The secret and data-directory port.
    pub platform: Arc<dyn Platform>,
    /// The configured account, if any. `None` reads the keychain exactly as
    /// before an account existed: no registry row, no HTTP client (NFR-92).
    pub account: Option<AccountCredential>,
    /// The drive, or `None` on a build that has none (the phone).
    pub drive: Option<DrivePorts>,
}

impl TurnEnv {
    /// An environment with no account and no drive.
    pub fn new(platform: Arc<dyn Platform>) -> Self {
        Self {
            platform,
            account: None,
            drive: None,
        }
    }
}

/// Now, in ms since the Unix epoch (UTC).
pub fn now_ms() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_millis()).unwrap_or(i64::MAX),
        // Before 1970. Not reachable on a machine whose clock is sane, and a
        // saturating floor is a better record than a panic.
        Err(_) => 0,
    }
}

/// A fresh opaque id, in the shape every other keeper record uses.
pub fn new_id() -> String {
    ulid::Ulid::new().to_string()
}

fn provider_of(dir: &Path, provider_id: &str) -> Result<store::ProviderRow, AgentError> {
    store::get_provider(dir, provider_id)?.ok_or_else(|| AgentError::NoSuch {
        what: "provider",
        id: provider_id.to_owned(),
    })
}

fn bot_of(dir: &Path, bot_id: &str) -> Result<Bot, AgentError> {
    store::get_bot(dir, bot_id)?.ok_or_else(|| AgentError::NoSuch {
        what: "bot",
        id: bot_id.to_owned(),
    })
}

/// A bot provider's bearer token: the account's access token when the
/// provider is set to "Use my account", else its keychain item.
async fn credential(
    env: &TurnEnv,
    provider_id: &str,
    bot: Option<&str>,
) -> Result<Option<String>, AccountError> {
    let Some(account) = &env.account else {
        return keeper_core::bots::resolve_token(env.platform.as_ref(), provider_id, bot)
            .map_err(|error| AccountError::Internal(error.to_string()));
    };
    let http = account
        .http
        .as_ref()
        .map_err(|error| AccountError::Internal(error.clone()))?;
    keeper_core::bots::resolve_credential(
        env.platform.as_ref(),
        http,
        Some(&account.descriptor),
        provider_id,
        bot,
    )
    .await
}

/// Assemble the endpoint for one provider, optionally addressing one bot.
///
/// The token comes from `keeper_core::bots::resolve_credential`, which knows
/// the bot-then-provider fallback order and the provider's choice of the
/// account instead; the join of base URL, kind and profile prefix is
/// `Endpoint::url`'s. Neither is re-derived here.
pub async fn endpoint_of(
    env: &TurnEnv,
    row: &store::ProviderRow,
    bot: Option<&str>,
) -> Result<Endpoint, AgentError> {
    let token = credential(env, &row.provider.id, bot).await?;
    Ok(Endpoint::new(&row.provider, bot, token))
}

/// The silence budget for one provider: its override, or the policy default.
pub fn read_timeout_of(row: &store::ProviderRow) -> Duration {
    match row.read_timeout_ms {
        Some(ms) if ms > 0 => Duration::from_millis(ms.unsigned_abs()),
        _ => http::READ_TIMEOUT,
    }
}

/// The per-provider capability cache (Epic 63, AD-176).
///
/// One probe of `GET /v1/capabilities` per provider per process, remembered
/// here and forgotten when the provider is edited, removed or re-tested. The
/// key and the forgetting rule are `keeper_core::bots::remote::CapabilityCache`'s;
/// this holds the instance and the lock.
pub fn capabilities() -> std::sync::MutexGuard<'static, CapabilityCache> {
    static CACHE: std::sync::LazyLock<std::sync::Mutex<CapabilityCache>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(CapabilityCache::default()));
    CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What this provider's endpoint can do about sessions, probing once.
///
/// The probe is at the gateway root, without a profile prefix, because the
/// capabilities are the gateway's. An endpoint that answers nothing is
/// remembered as [`SessionCapabilities::NONE`].
pub async fn session_caps(env: &TurnEnv, row: &store::ProviderRow) -> SessionCapabilities {
    let provider = &row.provider;
    if let Some(caps) = capabilities().get(&provider.id, &provider.base_url) {
        return caps;
    }
    let caps = match (
        endpoint_of(env, row, None).await,
        discover::discovery_client(),
    ) {
        (Ok(endpoint), Ok(client)) => remote::probe_capabilities(&client, &endpoint).await,
        _ => SessionCapabilities::NONE,
    };
    capabilities().remember(&provider.id, &provider.base_url, caps);
    caps
}

/// The cached answer for a provider id, or none — for labelling rows without
/// a round trip.
pub fn cached_caps(dir: &Path, provider_id: &str) -> SessionCapabilities {
    store::get_provider(dir, provider_id)
        .ok()
        .flatten()
        .and_then(|row| capabilities().get(provider_id, &row.provider.base_url))
        .unwrap_or(SessionCapabilities::NONE)
}

/// One turn, resolved before its task is spawned.
pub struct Turn {
    /// Where `keeper.db` lives.
    pub dir: PathBuf,
    /// Where the request goes.
    pub endpoint: Endpoint,
    /// The request as the model will see it.
    pub request: ChatRequest,
    /// The provider's silence budget.
    pub read_timeout: Duration,
    /// The conversation.
    pub session_id: String,
    /// The provider.
    pub provider_id: String,
    /// The bot.
    pub bot_id: String,
    /// The assistant row this turn fills.
    pub assistant_id: String,
    /// How to build the host once the task exists. The grants are **not**
    /// here: the host re-reads them per call (FR-386), and a copy on this
    /// struct would be an unrevocable grant.
    pub drive: Box<dyn TurnHost>,
    /// The profiles a tool call may name; empty where there is no drive.
    pub profiles: Vec<SyncProfile>,
    /// The profile an unqualified tool path is relative to, or empty.
    pub default_profile_id: String,
    /// Where the turn came from.
    pub origin: TurnOrigin,
}

impl Turn {
    /// The ids the audit rows and the approval sheet name.
    pub fn host_ids(&self) -> host::HostIds {
        host::HostIds {
            data_dir: self.dir.clone(),
            provider_id: self.provider_id.clone(),
            bot_id: self.bot_id.clone(),
            session_id: self.session_id.clone(),
            message_id: Some(self.assistant_id.clone()),
        }
    }
}

/// A turn whose rows exist, ready to stream.
pub struct OpenedTurn {
    /// The turn.
    pub turn: Turn,
    /// The `Opened` event, sent before anything else.
    pub opened: BotStreamEvent,
    /// The bundle the model was told about, sent right after `Opened`.
    pub context: Option<ContextBundle>,
    /// The id `drive::stop` cancels.
    pub subscription_id: String,
    /// The bot's display name, for the voice turn's "Waiting for nixie".
    pub bot_name: String,
}

/// What [`arm_turn`] resolved.
pub struct Armed {
    /// The request as the model will see it.
    pub request: ChatRequest,
    /// The bundle the pane is told about.
    pub context: Option<ContextBundle>,
    /// The host the task will run over.
    pub drive: Box<dyn TurnHost>,
    /// The profiles a tool call may name.
    pub profiles: Vec<SyncProfile>,
    /// The profile an unqualified path means.
    pub default_profile_id: String,
    /// Where the turn came from, as `origin_of` answered.
    pub origin: TurnOrigin,
}

/// Everything about one turn that `keeper-core` decides from the live grants,
/// gathered here and decided there.
///
/// Three reads and three decisions. The reads: the live grants for this
/// `(provider, bot)`, the drive's profiles, and — only where a grant exists
/// and the provider is one keeper runs tools for — whether the model states
/// it can use tools. The decisions, all `keeper-core`'s: [`tools::offer_tools`]
/// for what goes in `tools`, `context_files::context_targets` for which drive
/// files the model is told about, and [`tools::default_profile_id`] for the
/// profile an unqualified path means.
///
/// **The context bundle is built only when tools are offered**: it is the
/// rules for the files the model is about to touch, and a model that cannot
/// touch them has no call for their rules.
///
/// The system message: a typed or spoken turn joins the context prompt and
/// the spoken turn's answer instruction with `ChatMessage::instructions`; a
/// task sends the context prompt as it is, untrimmed, because that is what a
/// scheduled run has always sent.
///
/// An unreadable grant table reads as no grants: no tools, no context, and
/// the turn still runs as plain prose.
pub async fn arm_turn(
    env: &TurnEnv,
    dir: &Path,
    row: &store::ProviderRow,
    bot: &Bot,
    model: &str,
    messages: Vec<ChatMessage>,
    origin_of: &(dyn Fn(&Path) -> TurnOrigin + Sync),
) -> Armed {
    let grants = store::list_grants_for_bot(dir, &bot.provider_id, Some(&bot.id))
        .map(|listing| listing.live)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "bots: could not read the grants for this turn");
            Vec::new()
        });
    let kind = row.provider.kind;
    // The probe is a network round trip, spent only where its answer can
    // change the decision: `offer_tools` withholds for Hermes and for no
    // grant whatever the capability says.
    let tools_supported = if grants.is_empty() || !discover::probes_model_capabilities(kind) {
        None
    } else {
        discovered_model(env, dir, bot, model)
            .await
            .and_then(|found| found.tools)
    };
    let offer = tools::offer_tools(kind, tools_supported, &grants);
    if let ToolOffer::Withheld { reason } = &offer {
        tracing::debug!(reason, "bots: no tools offered this turn");
    }

    let drive = match &env.drive {
        Some(ports) => host::arm_drive(ports, &grants, offer.is_offered()),
        None => ArmedDrive::none(),
    };
    let profile_ids: Vec<&str> = drive
        .profiles
        .iter()
        .map(|profile| profile.id.as_str())
        .collect();
    let default_profile_id = tools::default_profile_id(&grants, &profile_ids).unwrap_or_default();

    let origin = origin_of(dir);
    let context_prompt = drive
        .context
        .as_ref()
        .and_then(ContextBundle::system_prompt);
    let mut prompted = Vec::with_capacity(messages.len() + 1);
    match &origin {
        TurnOrigin::Task => {
            if let Some(context) = context_prompt {
                prompted.push(ChatMessage::text(Role::System, context));
            }
        }
        TurnOrigin::Typed | TurnOrigin::Spoken { .. } => {
            // Epic 64, AD-182: a question the voice turn heard is answered
            // aloud, in the language the person is speaking.
            let instruction = match &origin {
                TurnOrigin::Spoken { language } => {
                    tracing::info!(%language, "bots: a spoken turn; asking for the answer in the listening language");
                    Some(speech::answer_instruction(language))
                }
                _ => None,
            };
            let parts = [context_prompt, instruction].into_iter().flatten();
            if let Some(system) = ChatMessage::instructions(parts) {
                prompted.push(system);
            }
        }
    }
    prompted.extend(messages);

    let request = ChatRequest {
        model: model.to_owned(),
        messages: prompted,
        tools: offer.specs(),
        ..ChatRequest::default()
    };
    Armed {
        request,
        context: drive.context,
        drive: drive.host,
        profiles: drive.profiles,
        default_profile_id,
        origin,
    }
}

/// Open a turn: the conversation (created when `req.session_id` is absent),
/// the user's message and the empty, partial assistant row, all stored before
/// anything is sent.
///
/// The whole conversation is replayed to the model from keeper's store, in
/// order. On a Hermes that honours the continuity header the turn **also**
/// names its session (AD-176): the id the row holds, or one minted here and
/// written to the row before the request goes out.
pub async fn open_turn(
    env: &TurnEnv,
    req: BotChatSendReq,
    origin_of: &(dyn Fn(&Path) -> TurnOrigin + Sync),
) -> Result<OpenedTurn, AgentError> {
    let dir = env.platform.data_dir()?;
    let bot = bot_of(&dir, &req.bot_id)?;
    let row = provider_of(&dir, &bot.provider_id)?;
    let endpoint = endpoint_of(env, &row, Some(&bot.target)).await?;
    let now = now_ms();

    // The conversation, created on the first message so a titled conversation
    // can never exist with nothing in it.
    let mut session_row = match req.session_id.as_deref() {
        Some(id) => session::get_session(&dir, id)?.ok_or_else(|| AgentError::NoSuch {
            what: "conversation",
            id: id.to_owned(),
        })?,
        None => {
            let created = session::BotSession {
                id: new_id(),
                bot_id: bot.id.clone(),
                provider_id: bot.provider_id.clone(),
                title: session::mint_title(&req.text),
                created_ms: now,
                updated_ms: now,
                archived: false,
                remote_session_id: None,
                remote_last_active_ms: None,
                remote_source: None,
            };
            session::insert_session(&dir, &created)?;
            created
        }
    };
    let continuity = adopt_identity(env, &dir, &row, &mut session_row).await?;

    let user = store_message(&dir, &session_row.id, "user", &req.text, None, false, now)?;
    let assistant = store_message(
        &dir,
        &session_row.id,
        "assistant",
        "",
        Some((&req.model, &bot.provider_id)),
        true,
        now,
    )?;
    session::touch_session(&dir, &session_row.id, now)?;

    let history = session::list_messages(&dir, &session_row.id)?;
    // Story 61.12: the pasted images of this turn become `data:` content parts
    // on the user message, here and nowhere else.
    let messages = attach_staged_images(&dir, replay(&history, &assistant.id), &req.attachment_ids);
    let mut armed = arm_turn(env, &dir, &row, &bot, &req.model, messages, origin_of).await;
    armed.request.session_id = continuity;
    let turn = Turn {
        dir,
        endpoint,
        request: armed.request,
        read_timeout: read_timeout_of(&row),
        session_id: session_row.id.clone(),
        provider_id: bot.provider_id.clone(),
        bot_id: bot.id.clone(),
        assistant_id: assistant.id.clone(),
        drive: armed.drive,
        profiles: armed.profiles,
        default_profile_id: armed.default_profile_id,
        origin: armed.origin,
    };

    let subscription_id = new_id();
    let opened = BotStreamEvent::Opened {
        subscription_id: subscription_id.clone(),
        session: Box::new(BotSessionVm::compose(&session_row)),
        user: Box::new(BotMessageVm::compose(&user)),
        assistant: Box::new(BotMessageVm::compose(&assistant)),
    };
    Ok(OpenedTurn {
        turn,
        opened,
        context: armed.context,
        subscription_id,
        bot_name: bot.name,
    })
}

/// Re-open the question one assistant row failed to answer (FR-372).
///
/// The failed row and everything after it are **replaced**: two answers to
/// one question is a record nobody can read, and a re-sent request samples
/// afresh. A row read from the gateway's transcript rather than written here
/// (AD-181) cannot be retried from this device.
pub async fn open_retry(
    env: &TurnEnv,
    req: BotRetryReq,
    origin_of: &(dyn Fn(&Path) -> TurnOrigin + Sync),
) -> Result<OpenedTurn, AgentError> {
    let dir = env.platform.data_dir()?;
    let mut session_row =
        session::get_session(&dir, &req.session_id)?.ok_or_else(|| AgentError::NoSuch {
            what: "conversation",
            id: req.session_id.clone(),
        })?;
    let bot = bot_of(&dir, &session_row.bot_id)?;
    let row = provider_of(&dir, &bot.provider_id)?;
    let endpoint = endpoint_of(env, &row, Some(&bot.target)).await?;
    let continuity = adopt_identity(env, &dir, &row, &mut session_row).await?;

    let history = session::list_messages(&dir, &req.session_id)?;
    let Some(doomed) = history.iter().find(|message| message.id == req.message_id) else {
        if continuity.is_some() {
            return Err(AgentError::Unsupported(
                "that answer was read from the gateway, not written here; retry it \
                 from the device that asked, or ask again"
                    .to_owned(),
            ));
        }
        return Err(AgentError::NoSuch {
            what: "message",
            id: req.message_id,
        });
    };
    if doomed.role != "assistant" {
        return Err(AgentError::Refused(
            "only an answer can be retried; a question is not re-asked by keeper".to_owned(),
        ));
    }
    // Drop the failed answer and everything after it: replaying a later turn
    // over a re-sampled earlier one would build a conversation that never
    // happened.
    for message in history.iter().filter(|m| m.seq >= doomed.seq) {
        session::delete_message(&dir, &message.id)?;
    }

    let now = now_ms();
    let assistant = store_message(
        &dir,
        &req.session_id,
        "assistant",
        "",
        Some((&req.model, &bot.provider_id)),
        true,
        now,
    )?;
    session::touch_session(&dir, &req.session_id, now)?;

    let replayed = session::list_messages(&dir, &req.session_id)?;
    let mut armed = arm_turn(
        env,
        &dir,
        &row,
        &bot,
        &req.model,
        replay(&replayed, &assistant.id),
        origin_of,
    )
    .await;
    armed.request.session_id = continuity;
    let turn = Turn {
        dir,
        endpoint,
        request: armed.request,
        read_timeout: read_timeout_of(&row),
        session_id: req.session_id.clone(),
        provider_id: bot.provider_id.clone(),
        bot_id: bot.id.clone(),
        assistant_id: assistant.id.clone(),
        drive: armed.drive,
        profiles: armed.profiles,
        default_profile_id: armed.default_profile_id,
        origin: armed.origin,
    };

    let subscription_id = new_id();
    let opened = BotStreamEvent::Opened {
        subscription_id: subscription_id.clone(),
        session: Box::new(BotSessionVm::compose(&session_row)),
        // The question is unchanged, so the row the pane already holds is
        // echoed rather than re-minted: a Retry that re-emitted a new user
        // message would double the question on screen.
        user: Box::new(
            replayed
                .iter()
                .rfind(|m| m.role == "user")
                .map(BotMessageVm::compose)
                .unwrap_or_else(|| BotMessageVm::compose(&assistant)),
        ),
        assistant: Box::new(BotMessageVm::compose(&assistant)),
    };
    Ok(OpenedTurn {
        turn,
        opened,
        context: armed.context,
        subscription_id,
        bot_name: bot.name,
    })
}

/// The session id this turn sends, written to the row first (AD-176).
///
/// `keeper_core::bots::remote::continuity_id` decides. A fresh id is persisted
/// **before** it is returned, and `row.remote_session_id` is updated in place
/// so the `Opened` event carries the identity the request is about to use.
pub async fn adopt_identity(
    env: &TurnEnv,
    dir: &Path,
    provider: &store::ProviderRow,
    row: &mut session::BotSession,
) -> Result<Option<String>, AgentError> {
    let caps = session_caps(env, provider).await;
    let continuity = remote::continuity_id(caps, row.remote_session_id.as_deref(), new_id);
    if continuity.is_some() && continuity != row.remote_session_id {
        session::set_session_remote_id(dir, &row.id, continuity.as_deref())?;
        row.remote_session_id = continuity.clone();
    }
    Ok(continuity)
}

/// Store one message and return it as written, with the `seq` the store
/// assigned.
pub fn store_message(
    dir: &Path,
    session_id: &str,
    role: &str,
    content: &str,
    model: Option<(&str, &str)>,
    partial: bool,
    now: i64,
) -> Result<session::BotMessage, AgentError> {
    let mut message = session::BotMessage {
        id: new_id(),
        session_id: session_id.to_owned(),
        seq: 0,
        role: role.to_owned(),
        content: content.to_owned(),
        model: model.map(|(model, _)| model.to_owned()),
        provider_id: model.map(|(_, provider)| provider.to_owned()),
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        ttft_ms: None,
        duration_ms: None,
        finish_reason: None,
        request_id: None,
        tool_call_count: 0,
        partial,
        created_ms: now,
    };
    message.seq = session::append_message(dir, &message)?;
    Ok(message)
}

/// The conversation as the model is told it, excluding the empty assistant row
/// this turn is about to fill.
///
/// A partial row from an earlier failed turn is replayed as what it is — the
/// model saw those tokens, and hiding them would make the next turn answer a
/// question it has already half-answered.
pub fn replay(history: &[session::BotMessage], exclude_id: &str) -> Vec<ChatMessage> {
    history
        .iter()
        .filter(|message| message.id != exclude_id && !message.content.is_empty())
        .map(|message| {
            let role = match message.role.as_str() {
                "system" => Role::System,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                // Anything else — including a role a newer keeper wrote — is
                // replayed as the person's turn rather than dropped.
                _ => Role::User,
            };
            ChatMessage::text(role, message.content.clone())
        })
        .collect()
}

/// This model as the endpoint describes it right now, or `None` when keeper
/// could not ask or the endpoint does not list it.
///
/// One read behind both the vision check and the turn's tool-capability
/// check, so the tri-state every capability carries — `None` is "did not
/// say", never `false` — is produced by one route (FR-377).
pub async fn discovered_model(
    env: &TurnEnv,
    dir: &Path,
    bot: &Bot,
    model: &str,
) -> Option<BotModelVm> {
    let row = provider_of(dir, &bot.provider_id).ok()?;
    let endpoint = endpoint_of(env, &row, Some(&bot.target)).await.ok()?;
    let client = http::client(read_timeout_of(&row)).ok()?;
    let models = discover::models(&client, &endpoint).await.ok()?;
    models.into_iter().find(|candidate| candidate.id == model)
}

/// Fold the staged images of this message into its user turn (FR-392).
///
/// The one place a pasted image becomes a `data:` URI. A staged image that
/// cannot be read back is skipped rather than fatal, and each image is
/// discarded once its bytes are in the request.
pub fn attach_staged_images(
    dir: &Path,
    mut messages: Vec<ChatMessage>,
    attachment_ids: &[String],
) -> Vec<ChatMessage> {
    if attachment_ids.is_empty() {
        return messages;
    }
    let Some(last) = messages.iter_mut().rev().find(|m| m.role == Role::User) else {
        return messages;
    };
    for id in attachment_ids {
        match deliverable::read_staged(dir, id) {
            Ok(bytes) => {
                let mime =
                    deliverable::staged_mime(dir, id).unwrap_or_else(|| "image/png".to_owned());
                last.content
                    .push(deliverable::image_content_part(&mime, &bytes));
                deliverable::discard_staged(dir, id);
            }
            Err(error) => {
                tracing::info!(%error, "bots: a staged image could not be read back");
            }
        }
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::ProfileSource;

    /// The replay excludes the row this turn is about to fill, and keeps a
    /// partial answer from an earlier turn.
    #[test]
    fn the_replay_excludes_the_row_being_filled() {
        let message = |id: &str, role: &str, content: &str, seq: i64| session::BotMessage {
            id: id.to_owned(),
            session_id: "s1".to_owned(),
            seq,
            role: role.to_owned(),
            content: content.to_owned(),
            model: None,
            provider_id: None,
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            ttft_ms: None,
            duration_ms: None,
            finish_reason: None,
            request_id: None,
            tool_call_count: 0,
            partial: false,
            created_ms: 0,
        };
        let history = vec![
            message("m1", "user", "hello", 0),
            message("m2", "assistant", "half an ans", 1),
            message("m3", "user", "again", 2),
            message("m4", "assistant", "", 3),
        ];
        let replayed = replay(&history, "m4");
        assert_eq!(replayed.len(), 3, "the empty row being filled is excluded");
        assert_eq!(replayed[1].role, Role::Assistant);
    }

    struct DataDir(PathBuf);

    impl Platform for DataDir {
        fn data_dir(&self) -> Result<PathBuf, CoreError> {
            Ok(self.0.clone())
        }
        fn keychain_set(&self, _: &str, _: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn keychain_get(&self, _: &str) -> Result<Option<String>, CoreError> {
            Ok(None)
        }
        fn keychain_delete(&self, _: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn open_url(&self, _: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn notify(
            &self,
            _: &str,
            _: &str,
            _: &keeper_core::vm::NotifyTarget,
        ) -> Result<(), CoreError> {
            Ok(())
        }
        fn sidecar_path(&self, _: &str) -> Result<PathBuf, CoreError> {
            Err(CoreError::Unsupported("no sidecar".to_owned()))
        }
        fn exclude_from_backup(&self, _: &Path) -> Result<(), CoreError> {
            Ok(())
        }
        fn set_badge_count(&self, _: Option<u32>) -> Result<(), CoreError> {
            Ok(())
        }
    }

    struct Folder(PathBuf);

    impl ProfileSource for Folder {
        fn profiles(&self) -> Vec<SyncProfile> {
            vec![SyncProfile::new(
                "folder",
                "Folder",
                self.0.clone(),
                "unused",
            )]
        }
    }

    fn system_text(armed: &Armed) -> Option<String> {
        let first = armed.request.messages.first()?;
        (first.role == Role::System).then(|| match first.content.first() {
            Some(keeper_core::bots::chat::ContentPart::Text(text)) => text.clone(),
            _ => String::new(),
        })
    }

    /// AD-182: a spoken turn's system message is the drive's context, then the
    /// answer-in-this-language sentence; a typed one carries the context
    /// alone; a task carries the context untrimmed, as a scheduled run always
    /// has.
    #[tokio::test]
    async fn a_spoken_origin_adds_the_answer_instruction_after_the_context_and_a_typed_one_adds_nothing(
    ) {
        let data = tempfile::tempdir().expect("data dir");
        let drive = tempfile::tempdir().expect("drive");
        std::fs::write(drive.path().join("AGENTS.md"), "Answer briefly.\n").expect("context");
        let dir = data.path();
        store::insert_provider(
            dir,
            &keeper_core::bots::Provider {
                id: "provider".to_owned(),
                kind: keeper_core::bots::ProviderKind::Ollama,
                name: "Nowhere".to_owned(),
                base_url: "http://127.0.0.1:9".to_owned(),
                created_ms: 1,
            },
        )
        .expect("provider");
        let bot = Bot {
            id: "bot".to_owned(),
            provider_id: "provider".to_owned(),
            target: "model".to_owned(),
            name: "Bot".to_owned(),
            pin_order: 0,
            identity: Default::default(),
            created_ms: 1,
        };
        store::insert_bot(dir, &bot).expect("bot");
        store::save_grant(
            dir,
            &keeper_core::bots::grant::Grant {
                id: "grant".to_owned(),
                provider_id: "provider".to_owned(),
                bot_id: Some("bot".to_owned()),
                scope: keeper_core::bots::grant::GrantScope::Profile {
                    profile_id: "folder".to_owned(),
                },
                mode: keeper_core::bots::grant::GrantMode::Read,
                created_ms: 1,
            },
        )
        .expect("grant");
        let row = provider_of(dir, "provider").expect("row");
        let env = TurnEnv {
            platform: Arc::new(DataDir(dir.to_owned())),
            account: None,
            drive: Some(DrivePorts {
                profiles: Arc::new(Folder(drive.path().to_owned())),
                vault: None,
                approval: None,
            }),
        };
        let question = || vec![ChatMessage::text(Role::User, "hello")];
        let arm = |origin: TurnOrigin| {
            let (env, row, bot) = (&env, &row, &bot);
            async move {
                arm_turn(env, dir, row, bot, "model", question(), &move |_| {
                    origin.clone()
                })
                .await
            }
        };

        let typed = arm(TurnOrigin::Typed).await;
        let context = typed
            .context
            .as_ref()
            .and_then(ContextBundle::system_prompt)
            .expect("the drive's AGENTS.md is the context");
        assert_eq!(system_text(&typed).as_deref(), Some(context.trim()));
        assert_eq!(typed.request.messages.len(), 2);

        let spoken = arm(TurnOrigin::Spoken {
            language: "pl-PL".to_owned(),
        })
        .await;
        assert_eq!(
            system_text(&spoken),
            Some(format!(
                "{}\n\n{}",
                context.trim(),
                speech::answer_instruction("pl-PL").trim()
            ))
        );

        let task = arm(TurnOrigin::Task).await;
        assert_eq!(system_text(&task), Some(context.clone()));
        assert_ne!(
            context,
            context.trim(),
            "the fixture's context ends in a newline"
        );
    }
}
