//! What can this endpoint actually do — models, capabilities, health, and the
//! one question that has no answer (Story 61.3, FR-375, FR-376, FR-377).
//!
//! Four questions, and this module is honest about which of them it can answer:
//!
//! 1. **Is it there, and what is it?** [`health`] asks one cheap, public route
//!    per kind — Ollama's `GET /api/version`, Hermes' `GET /health`, both of
//!    which carry a `version` string (R2 §3; R1 §10.3).
//! 2. **Which models can I send?** [`models`] reads the endpoint that is
//!    strictly better per kind: `GET /api/tags` for Ollama, because it carries
//!    the whole `capabilities` array for every local model in one round trip
//!    (R2 §4.1), and `GET /v1/models` merged with `GET /api/model/options` for
//!    Hermes, because the first is the roster and only the second knows what a
//!    model supports (R1 §2.9, §4.3).
//! 3. **Does this bot exist?** [`probe_bot`] verifies a *named* bot, and
//!    distinguishes "no such bot" from "keeper's credential was refused" from
//!    "nothing answered" — three different sentences for a user about to
//!    retype a name that was right all along.
//! 4. **Which bots exist?** It cannot say. See [`NO_BOT_ROSTER`] and
//!    [`enumerate_bots`].
//!
//! # The tri-state, which is the load-bearing rule here
//!
//! `vision`, `tools` and `reasoning` on [`BotModelVm`] are `Option<bool>`, and
//! `None` means *the endpoint did not say*. It is never flattened to `false`,
//! at any point, in any branch: an Ollama older than the `capabilities` array
//! (added to `/api/show` in v0.6.4; `omitempty` on both routes, so absence and
//! emptiness are the same fact — R2 §11) tells keeper nothing, and a Hermes
//! whose `/api/model/options` is unreachable tells keeper nothing either. The
//! surface offers an unknown capability with a warning and hides only a
//! capability the endpoint refused (AD-27).
//!
//! # Bounded bodies, silence, and secrets
//!
//! No body is read with `Response::text`: every read goes through
//! [`bounded_body`], which stops at [`MAX_DISCOVERY_BODY`]. The client's only
//! deadline is a ceiling on **silence** and never on total duration — the
//! reasoning is written out in `keeper-sync/src/http.rs:24-40`, and because
//! `keeper-core` may not depend on `keeper-sync` (AD-40) it is restated at
//! [`DISCOVERY_READ_TIMEOUT`] rather than copied by accident. The
//! `AUTHORIZATION` header is marked sensitive so `reqwest` redacts it from
//! error text, and no sentence this module produces carries a token or a URL
//! with userinfo.

use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::bots::error::BotsError;
use crate::bots::http;
use crate::bots::{BotHealthState, Endpoint, ProviderKind};
use crate::vm::{BotModelVm, BotPresence, BotProbeVm, BotReach};

/// How long a discovery request may deliver **nothing** before it is a failure.
///
/// A ceiling on silence, not on duration. The distinction is not a style
/// choice: a whole-request deadline is the wrong question for a network client,
/// because any budget generous enough for a slow-but-alive peer is far too
/// large to catch a socket whose peer has gone. `keeper-sync/src/http.rs:24-40`
/// records the observed failure that settled this — nine transfers sat in
/// `running` for sixteen hours with a live TCP connection and no bytes — and
/// `reqwest`'s `read_timeout` is the knob that answers it, since it applies to
/// each read and resets after every successful one.
///
/// That module is the source of this policy; it is restated here instead of
/// imported because `keeper-core` may not depend on `keeper-sync` (AD-40,
/// `check:core-sync-free`).
///
/// Thirty seconds rather than sixty: unlike an LFS batch, every route in this
/// module answers from memory or from a small SQLite read, so a half-minute of
/// silence is already a broken endpoint rather than a thinking one.
pub const DISCOVERY_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// The most JSON keeper will read from a discovery route.
///
/// A discovery answer is a roster, not a payload: Ollama's `/api/tags` is a few
/// hundred bytes per model and Hermes' `/api/model/options` a few kilobytes per
/// provider. A megabyte is far above any honest answer and far below anything
/// that could hurt the app, and the bound exists because
/// `Response::text`/`Response::json` on an unbounded body is a promise to
/// allocate whatever the far end sends.
pub const MAX_DISCOVERY_BODY: usize = 1024 * 1024;

/// Why keeper does not list the bots on a Hermes endpoint — written once, so
/// the pane, the empty state and the docs quote the same words.
///
/// This is not a gap in the implementation. The bearer-authenticated API server
/// has no profile roster at all: its complete route table (R1 §2.2, read from
/// `_http_route_table()`) contains no such route, and the two doors that do list
/// profiles need credentials keeper is not given — the dashboard's
/// `GET /api/profiles` behind an `X-Hermes-Session-Token`, and the TUI gateway's
/// `profiles.list` over a JSON-RPC websocket keeper does not speak (R1 §4.2).
/// So keeper ships verification rather than enumeration: a bot is named by the
/// person who has one, and [`probe_bot`] confirms it is really there.
pub const NO_BOT_ROSTER: &str = "keeper cannot list the bots on a Hermes endpoint. \
     The key you gave it opens the chat API, and that API has no route that names \
     the profiles behind it — a roster needs a door keeper is not handed. Type a \
     bot's name and keeper will check it exists before you rely on it.";

/// The HTTP client every discovery route shares.
///
/// Built by [`http::client`] — the one place a client is constructed in this
/// module tree — with silence, not duration, as its bound. Callers hold one
/// client for a whole discovery pass so connections are reused.
pub fn discovery_client() -> Result<reqwest::Client, BotsError> {
    http::client(DISCOVERY_READ_TIMEOUT)
}

/// The cheap, public route that says whether an endpoint is there and what
/// version it is, per kind.
///
/// Ollama: `GET /api/version` — unauthenticated, present since the first
/// releases, `{"version":"x.y.z"}` (R2 §3). Hermes: `GET /health` — public, and
/// its `version` comes from `hermes_cli.__version__` specifically so a client
/// can read the gateway version without scraping (R1 §10.3).
fn health_route(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Ollama => "/api/version",
        ProviderKind::Hermes => "/health",
    }
}

/// Ask an endpoint whether it is there, and what it is.
///
/// Never an `Err`: an endpoint that refused keeper's credential, or that never
/// answered, is a *fact about the endpoint* the surface has to print, so it
/// comes back as a [`BotProbeVm`] with [`BotReach::Offline`] or a status and a
/// reason. The only thing this cannot report is a version the endpoint did not
/// state, and `None` there is not a fault.
pub async fn health(client: &reqwest::Client, endpoint: &Endpoint) -> BotProbeVm {
    let path = health_route(endpoint.kind);
    let label = format!("GET {path}");
    let started = Instant::now();
    let request = match http::authorize(client.get(endpoint.url(path)), endpoint.token.as_deref()) {
        Ok(request) => request,
        Err(err) => return unreachable_probe(None, err.to_string()),
    };
    match request.send().await {
        Err(err) => unreachable_probe(None, BotsError::transport(err).to_string()),
        Ok(response) => {
            let round_trip_ms = elapsed_ms(started);
            let status = response.status();
            let (version, reason) = if status.is_success() {
                (version_of(response, &label).await, None)
            } else {
                (
                    None,
                    Some(status_sentence(status.as_u16(), path, endpoint.kind)),
                )
            };
            BotProbeVm {
                reach: BotReach::Online,
                status: Some(status.as_u16()),
                version,
                round_trip_ms: Some(round_trip_ms),
                bot: None,
                presence: None,
                reason,
            }
        }
    }
}

/// The verdict a provider row stores, derived from a probe.
///
/// Lives here rather than in the shell because it is a decision (AD-55/AD-56):
/// which HTTP facts mean "reachable", which mean "the credential is wrong", and
/// which mean keeper genuinely does not know. `401`/`403` is its own state
/// because it is its own sentence — the endpoint is there, and on a Hermes
/// profile prefix it may want that profile's own key (R1 §9.2) — while any
/// other unexpected status leaves the verdict [`BotHealthState::Unknown`]: a
/// `502` from a reverse proxy says nothing about the endpoint behind it, and
/// recording that as `Unreachable` would be a claim keeper cannot support.
///
/// [`BotHealthState::SecretMissing`] is never produced here: it is a fact about
/// keeper's own keychain that Story 61.1 establishes before a request is built,
/// not something an endpoint can tell you.
pub fn health_state(probe: &BotProbeVm) -> BotHealthState {
    match (probe.reach, probe.status) {
        (BotReach::Offline, _) => BotHealthState::Unreachable,
        (BotReach::Online, Some(401 | 403)) => BotHealthState::Unauthorized,
        (BotReach::Online, Some(status)) if (200..300).contains(&status) => {
            BotHealthState::Reachable
        }
        (BotReach::Online, _) => BotHealthState::Unknown,
    }
}

/// Every model this endpoint will accept as the `model` of a chat request.
///
/// Per kind, from the route that actually knows the answer — see the module
/// docs. Capabilities are whatever the endpoint stated and `None` wherever it
/// stated nothing.
pub async fn models(
    client: &reqwest::Client,
    endpoint: &Endpoint,
) -> Result<Vec<BotModelVm>, BotsError> {
    match endpoint.kind {
        ProviderKind::Ollama => ollama_models(client, endpoint).await,
        ProviderKind::Hermes => hermes_models(client, endpoint).await,
    }
}

/// Whether a named bot is really there.
///
/// **Hermes**: every api-server route is registered a second time under
/// `/p/{profile}`, so a bearer client can *address* any bot by URL prefix
/// (R1 §2.2) — keeper asks that prefix for its own model list and reads the
/// answer. `404` is "no such profile on this gateway"; `401`/`403` is "that
/// prefix wants its own key" and emphatically **not** absence, because a
/// profile served by a different `API_SERVER_KEY` answers exactly that way
/// (R1 §9.2); anything else, or nothing at all, is [`BotPresence::Unknown`].
///
/// **Ollama**: a bot is a model tag, so the probe is a lookup in the tag list.
///
/// The prefixed URL is built by [`Endpoint::url`] from a copy of the caller's
/// endpoint carrying `bot`, never by string arithmetic here — one place in the
/// tree knows how a Hermes profile prefix is spelled.
pub async fn probe_bot(client: &reqwest::Client, endpoint: &Endpoint, bot: &str) -> BotProbeVm {
    match endpoint.kind {
        ProviderKind::Hermes => probe_hermes_profile(client, endpoint, bot).await,
        ProviderKind::Ollama => probe_ollama_tag(client, endpoint, bot).await,
    }
}

/// The answer to "which bots exist here?", including the answer that is a
/// refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BotRoster {
    /// The endpoint can be asked, and these are its bots. Ollama only: its
    /// bots are its model tags, which `/api/tags` lists in full.
    Enumerated(Vec<String>),
    /// The endpoint cannot be asked, and this is the sentence that says why.
    /// Always [`NO_BOT_ROSTER`] today; typed as an outcome rather than an error
    /// because nothing failed — keeper was never given the door.
    Unavailable {
        /// Ready to print, in the house voice, identical everywhere.
        reason: &'static str,
    },
}

/// List the bots at an endpoint, or say why that cannot be done.
///
/// For Ollama this is real enumeration. For Hermes it is deliberately **not
/// implemented**, and the reason is [`NO_BOT_ROSTER`] — the bearer API exposes
/// no roster (R1 §4.2). No request is sent in that case: keeper does not probe
/// a route it has read the source of and knows is absent.
pub async fn enumerate_bots(
    client: &reqwest::Client,
    endpoint: &Endpoint,
) -> Result<BotRoster, BotsError> {
    match endpoint.kind {
        ProviderKind::Hermes => Ok(BotRoster::Unavailable {
            reason: NO_BOT_ROSTER,
        }),
        ProviderKind::Ollama => Ok(BotRoster::Enumerated(
            models(client, endpoint)
                .await?
                .into_iter()
                .map(|model| model.id)
                .collect(),
        )),
    }
}

/// The N+1 fallback, for one Ollama model, on demand only.
///
/// [`models`] never calls this: `/api/tags` already carries `capabilities` for
/// every local model in a single round trip (R2 §4.1), and asking `/api/show`
/// per model would turn one request into one-plus-N against a daemon that may
/// be loading a model at the time. It exists for the one case the tri-state
/// leaves open — a build old enough that `/api/tags` served no capabilities at
/// all — where a surface may ask about a single model the user actually chose.
///
/// Hermes has no equivalent and needs none: its enrichment already arrives from
/// `/api/model/options` inside [`models`].
pub async fn ollama_model_capabilities(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    model: &str,
) -> Result<BotModelVm, BotsError> {
    const PATH: &str = "/api/show";
    let label = "POST /api/show";
    let request = http::authorize(client.post(endpoint.url(PATH)), endpoint.token.as_deref())?
        .json(&ShowRequest { model });
    let response = request.send().await.map_err(BotsError::transport)?;
    let response = require_success(response, label).await?;
    let shown: ShowBody = json_body(response, label).await?;
    let (vision, tools, reasoning, capabilities) = capability_flags(shown.capabilities.as_deref());
    Ok(BotModelVm {
        id: model.to_owned(),
        family: shown.details.as_ref().and_then(|d| d.family.clone()),
        parameter_size: shown
            .details
            .as_ref()
            .and_then(|d| d.parameter_size.clone()),
        quantization: shown
            .details
            .as_ref()
            .and_then(|d| d.quantization_level.clone()),
        size_bytes: None,
        context_window: None,
        max_output_tokens: None,
        vision,
        tools,
        reasoning,
        capabilities,
    })
}

// ---------------------------------------------------------------------------
// Ollama
// ---------------------------------------------------------------------------

/// `GET /api/tags` — one request for the whole local roster *and* its
/// capabilities (R2 §4.1). Note what this function does not do: it does not
/// loop over the rows calling `/api/show`.
async fn ollama_models(
    client: &reqwest::Client,
    endpoint: &Endpoint,
) -> Result<Vec<BotModelVm>, BotsError> {
    const PATH: &str = "/api/tags";
    let rows = ollama_tags(client, endpoint, PATH).await?;
    Ok(rows.into_iter().filter_map(tag_to_vm).collect())
}

/// The shared `/api/tags` fetch, so the bot probe and the model list read the
/// same route through the same parser.
async fn ollama_tags(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    path: &'static str,
) -> Result<Vec<TagRow>, BotsError> {
    let label = format!("GET {path}");
    let request = http::authorize(client.get(endpoint.url(path)), endpoint.token.as_deref())?;
    let response = request.send().await.map_err(BotsError::transport)?;
    let response = require_success(response, &label).await?;
    let body: TagsBody = json_body(response, &label).await?;
    Ok(body.models)
}

/// A `/api/tags` row, mapped to keeper's own shape. Rows with no usable id are
/// dropped rather than invented: `model` and `name` are the same tag on every
/// build that serves both, and a row carrying neither names nothing keeper
/// could send.
fn tag_to_vm(row: TagRow) -> Option<BotModelVm> {
    let id = row.model.or(row.name)?;
    if id.is_empty() {
        return None;
    }
    let (vision, tools, reasoning, capabilities) = capability_flags(row.capabilities.as_deref());
    Some(BotModelVm {
        id,
        family: row.details.as_ref().and_then(|d| d.family.clone()),
        parameter_size: row.details.as_ref().and_then(|d| d.parameter_size.clone()),
        quantization: row
            .details
            .as_ref()
            .and_then(|d| d.quantization_level.clone()),
        size_bytes: row.size,
        // Three different numbers answer "what is the context window" on Ollama
        // — the model's trained maximum, the window the server will allocate
        // for the machine's VRAM, and the window a loaded instance is actually
        // running with (R2 §4.4). Discovery reads none of them, because
        // printing one as *the* window would be a number keeper did not
        // measure.
        context_window: None,
        max_output_tokens: None,
        vision,
        tools,
        reasoning,
        capabilities,
    })
}

/// The whole tri-state rule, in one place.
///
/// `None` in, `None` out — and an **empty** list is the same answer as an
/// absent one, because `capabilities` carries `omitempty` on both `/api/tags`
/// and `/api/show`, so a server that has capabilities to report never sends an
/// empty array (R2 §4.1, §11.3). A list that is present and non-empty is the
/// endpoint speaking, so each flag becomes a definite `Some`.
///
/// The vocabulary is open-ended by design — `completion`, `tools`, `insert`,
/// `vision`, `embedding`, `thinking`, `image`, `audio` today (R2 §4.3) — so
/// unknown strings are kept verbatim in the returned list and never dropped.
fn capability_flags(
    capabilities: Option<&[String]>,
) -> (Option<bool>, Option<bool>, Option<bool>, Vec<String>) {
    let Some(list) = capabilities.filter(|list| !list.is_empty()) else {
        return (None, None, None, Vec::new());
    };
    let has = |name: &str| list.iter().any(|item| item == name);
    (
        Some(has("vision")),
        Some(has("tools")),
        Some(has("thinking")),
        list.to_vec(),
    )
}

/// The probe for an Ollama "bot", which is a model tag.
async fn probe_ollama_tag(client: &reqwest::Client, endpoint: &Endpoint, bot: &str) -> BotProbeVm {
    const PATH: &str = "/api/tags";
    let started = Instant::now();
    match ollama_tags(client, endpoint, PATH).await {
        Ok(rows) => {
            let present = rows
                .into_iter()
                .filter_map(tag_to_vm)
                .any(|model| model.id == bot);
            BotProbeVm {
                reach: BotReach::Online,
                status: Some(200),
                version: None,
                round_trip_ms: Some(elapsed_ms(started)),
                bot: Some(bot.to_owned()),
                presence: Some(if present {
                    BotPresence::Exists
                } else {
                    BotPresence::Absent
                }),
                reason: if present {
                    None
                } else {
                    Some(format!(
                        "This Ollama has no model tagged \"{bot}\". Pull it there, or pick one \
                         of the models it already has."
                    ))
                },
            }
        }
        Err(err) => probe_from_error(err, bot, PATH, endpoint.kind, elapsed_ms(started)),
    }
}

// ---------------------------------------------------------------------------
// Hermes
// ---------------------------------------------------------------------------

/// `GET /v1/models` merged with `GET /api/model/options`.
///
/// The two routes answer different halves of one question and neither is
/// sufficient. `/v1/models` is the roster keeper may send as `model`: the
/// active profile's alias plus any `model_routes` aliases, and it is
/// *documented* as carrying no capability enrichment at all (R1 §2.9).
/// `/api/model/options` is the enrichment: authenticated providers, their
/// curated model lists and per-model `supports_tools` / `supports_vision` /
/// `supports_reasoning` / `context_window` (R1 §4.3).
///
/// The merge: the `/v1/models` order is kept, because those ids are the ones
/// this endpoint answers to unprompted; each is enriched by id from the options
/// payload. Models the options payload knows and `/v1/models` does not are
/// appended after them, because Hermes honours a `model` when a `provider`
/// accompanies it (R1 §2.4) — they are reachable, just not the default.
///
/// If the options call fails or answers a non-success status — an older Hermes,
/// a key without that scope — the roster still returns, with every capability
/// `None`. That is the tri-state doing its job: keeper could not read them, so
/// they are unknown, not absent.
///
/// One thing `supports_tools` does *not* mean here: Hermes executes its own
/// tools server-side and never hands a client a pending tool call (R1 §2.7), so
/// this flag describes the model behind Hermes, not whether keeper's own drive
/// tools can be driven through it. That distinction belongs to the chat loop,
/// and discovery does not pre-empt it by rewriting what the endpoint said.
async fn hermes_models(
    client: &reqwest::Client,
    endpoint: &Endpoint,
) -> Result<Vec<BotModelVm>, BotsError> {
    const MODELS: &str = "/v1/models";
    const OPTIONS: &str = "/api/model/options";

    let request = http::authorize(client.get(endpoint.url(MODELS)), endpoint.token.as_deref())?;
    let response = request.send().await.map_err(BotsError::transport)?;
    let response = require_success(response, "GET /v1/models").await?;
    let roster: HermesModelsBody = json_body(response, "GET /v1/models").await?;

    let enrichment = hermes_model_options(client, endpoint, OPTIONS)
        .await
        .unwrap_or_default();

    let mut out: Vec<BotModelVm> = Vec::with_capacity(roster.data.len() + enrichment.len());
    for row in roster.data {
        let id = row.id;
        if id.is_empty() || out.iter().any(|seen| seen.id == id) {
            continue;
        }
        let hint = enrichment.iter().find(|entry| entry.id == id);
        out.push(hermes_vm(id, hint));
    }
    for entry in &enrichment {
        if entry.id.is_empty() || out.iter().any(|seen| seen.id == entry.id) {
            continue;
        }
        out.push(hermes_vm(entry.id.clone(), Some(entry)));
    }
    Ok(out)
}

/// Build one Hermes model row. Absent enrichment means every flag is `None` —
/// the endpoint did not say, so keeper does not either.
fn hermes_vm(id: String, hint: Option<&HermesHint>) -> BotModelVm {
    BotModelVm {
        id,
        family: hint.and_then(|h| h.family.clone()),
        parameter_size: None,
        quantization: None,
        size_bytes: None,
        context_window: hint.and_then(|h| h.context_window),
        max_output_tokens: hint.and_then(|h| h.max_output_tokens),
        vision: hint.and_then(|h| h.vision),
        tools: hint.and_then(|h| h.tools),
        reasoning: hint.and_then(|h| h.reasoning),
        capabilities: Vec::new(),
    }
}

/// Best-effort capability enrichment. An `Err` here is not a failure of
/// discovery: the caller degrades to unknown capabilities rather than to no
/// models.
async fn hermes_model_options(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    path: &'static str,
) -> Result<Vec<HermesHint>, BotsError> {
    let request = http::authorize(client.get(endpoint.url(path)), endpoint.token.as_deref())?;
    let label = format!("GET {path}");
    let response = request.send().await.map_err(BotsError::transport)?;
    let response = require_success(response, &label).await?;
    let body: HermesOptionsBody = json_body(response, "GET /api/model/options").await?;
    let mut hints = Vec::new();
    let rows = body
        .providers
        .into_iter()
        .flat_map(|provider| provider.models)
        .chain(body.models);
    for row in rows {
        if let Some(hint) = row.into_hint() {
            hints.push(hint);
        }
    }
    Ok(hints)
}

/// Ask a Hermes profile prefix for its own model list.
async fn probe_hermes_profile(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    bot: &str,
) -> BotProbeVm {
    const PATH: &str = "/v1/models";
    let scoped = Endpoint {
        kind: endpoint.kind,
        base_url: endpoint.base_url.clone(),
        bot: Some(bot.to_owned()),
        token: endpoint.token.clone(),
    };
    let started = Instant::now();
    let request = match http::authorize(client.get(scoped.url(PATH)), scoped.token.as_deref()) {
        Ok(request) => request,
        Err(err) => return probe_from_error(err, bot, PATH, scoped.kind, elapsed_ms(started)),
    };
    match request.send().await {
        Err(err) => probe_from_error(
            BotsError::transport(err),
            bot,
            PATH,
            scoped.kind,
            elapsed_ms(started),
        ),
        Ok(response) => {
            let round_trip_ms = elapsed_ms(started);
            let status = response.status().as_u16();
            let (presence, reason) = hermes_presence(status, bot);
            BotProbeVm {
                reach: BotReach::Online,
                status: Some(status),
                version: None,
                round_trip_ms: Some(round_trip_ms),
                bot: Some(bot.to_owned()),
                presence: Some(presence),
                reason,
            }
        }
    }
}

/// The three-way discrimination this story exists for, kept as one pure
/// decision so nothing downstream re-derives it.
///
/// `404` is absence — the gateway serves no such profile. `401`/`403` is not:
/// a profile served under its own `API_SERVER_KEY` refuses the default key with
/// exactly that status (R1 §9.2), so the bot may well exist and keeper simply
/// was not allowed to look. Every other status is unknown too, because a `500`
/// or a `502` says something about the gateway and nothing about the bot.
fn hermes_presence(status: u16, bot: &str) -> (BotPresence, Option<String>) {
    match status {
        200..=299 => (BotPresence::Exists, None),
        404 => (
            BotPresence::Absent,
            Some(format!(
                "This Hermes gateway serves no bot named \"{bot}\". Check the spelling, or ask \
                 whoever runs it which profiles it has."
            )),
        ),
        401 | 403 => (
            BotPresence::Unknown,
            Some(format!(
                "\"{bot}\" refused keeper's key ({status}), so keeper cannot say whether it \
                 exists. A Hermes profile can be served under its own key — if this bot has one, \
                 add it as its own provider."
            )),
        ),
        other => (
            BotPresence::Unknown,
            Some(format!(
                "The gateway answered {other} when keeper asked about \"{bot}\", which says \
                 something about the gateway and nothing about the bot. Try again in a moment."
            )),
        ),
    }
}

// ---------------------------------------------------------------------------
// Shared HTTP plumbing
// ---------------------------------------------------------------------------

/// Turn a non-success status into a typed error, so no parser is ever handed an
/// error page.
///
/// The body excerpt goes through [`BotsError::status`], which bounds it — an
/// endpoint that answers 500 with a megabyte of HTML must not put a megabyte
/// anywhere — and `label` is the method and path only, never the base URL and
/// never a credential.
async fn require_success(
    response: reqwest::Response,
    label: &str,
) -> Result<reqwest::Response, BotsError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = bounded_body(response, label).await.unwrap_or_default();
    let detail = match String::from_utf8_lossy(&body).trim() {
        "" => "no body".to_owned(),
        text => text.to_owned(),
    };
    Err(BotsError::status(status.as_u16(), label, &detail))
}

/// Read a body in bounded chunks.
///
/// Deliberately not `Response::text` or `Response::json`: both consume whatever
/// the far end chooses to send, and a discovery route answering with a gigabyte
/// is a hostile or broken endpoint, not a reason to allocate a gigabyte.
async fn bounded_body(mut response: reqwest::Response, label: &str) -> Result<Vec<u8>, BotsError> {
    let mut body = Vec::new();
    loop {
        match response.chunk().await.map_err(BotsError::transport)? {
            None => return Ok(body),
            Some(chunk) => {
                if body.len() + chunk.len() > MAX_DISCOVERY_BODY {
                    return Err(BotsError::Protocol {
                        detail: format!(
                            "{label} answered with more than {MAX_DISCOVERY_BODY} bytes, which no \
                             honest model list needs"
                        ),
                    });
                }
                body.extend_from_slice(&chunk);
            }
        }
    }
}

/// Parse a bounded body, naming the route in any failure and nothing else.
async fn json_body<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    label: &str,
) -> Result<T, BotsError> {
    let body = bounded_body(response, label).await?;
    serde_json::from_slice(&body).map_err(|err| BotsError::Parse {
        endpoint: label.to_owned(),
        detail: err.to_string(),
    })
}

/// The version string, where the endpoint states one. A body that will not
/// parse leaves the version `None` rather than failing the health check: an
/// endpoint that answered is reachable whatever it said.
async fn version_of(response: reqwest::Response, label: &str) -> Option<String> {
    let body: VersionBody = json_body(response, label).await.ok()?;
    body.version.filter(|version| !version.is_empty())
}

/// The one sentence per status class, shared by health, discovery and probes so
/// the app says the same thing about the same fact.
fn status_sentence(status: u16, path: &str, kind: ProviderKind) -> String {
    match status {
        401 | 403 => match kind {
            // Ollama's local API has no authentication middleware at all
            // (R2 §5.3), so a 401 from one is a proxy or a gateway in front of
            // it, and telling the user to fix their Ollama key would send them
            // looking for a setting that does not exist.
            ProviderKind::Ollama => format!(
                "Something in front of this Ollama refused the request ({status}). Ollama itself \
                 does not ask for a key, so this is a proxy or a tunnel."
            ),
            ProviderKind::Hermes => format!(
                "The endpoint refused keeper's key ({status}). Check the provider's key, or \
                 whether this profile is served under one of its own."
            ),
        },
        404 => format!(
            "The endpoint answered, but has nothing at {path} ({status}). Check the base URL — a \
             path in it, or a Hermes gateway where an Ollama was expected, both look like this."
        ),
        429 => format!("The endpoint is rate-limiting keeper ({status}). Try again in a moment."),
        500..=599 => format!(
            "The endpoint is there but failing ({status} at {path}). That is its problem to \
             report, not keeper's to guess at."
        ),
        other => format!("The endpoint answered {other} at {path}, which keeper cannot use."),
    }
}

/// Nothing answered.
///
/// A probe that named a bot comes back [`BotPresence::Unknown`] and never
/// `Absent`: keeper could not ask, which is a different sentence from "it is
/// not there" for a user about to retype a name that was right all along. A
/// provider probe named no bot, so it reports no presence at all.
fn unreachable_probe(bot: Option<String>, reason: String) -> BotProbeVm {
    BotProbeVm {
        reach: BotReach::Offline,
        status: None,
        version: None,
        round_trip_ms: None,
        presence: bot.as_ref().map(|_| BotPresence::Unknown),
        bot,
        reason: Some(reason),
    }
}

/// A failed request during a bot probe.
///
/// Offline when nothing answered; an answer keeper could not use otherwise.
/// [`BotPresence::Unknown`] either way — a transport failure and a `500` both
/// say nothing about the bot — with the house sentence for a status, and the
/// error's own credential-free sentence for everything else.
fn probe_from_error(
    err: BotsError,
    bot: &str,
    path: &str,
    kind: ProviderKind,
    round_trip_ms: i64,
) -> BotProbeVm {
    match &err {
        BotsError::Status { status, .. } => BotProbeVm {
            reach: BotReach::Online,
            status: Some(*status),
            version: None,
            round_trip_ms: Some(round_trip_ms),
            bot: Some(bot.to_owned()),
            presence: Some(BotPresence::Unknown),
            reason: Some(status_sentence(*status, path, kind)),
        },
        _ => unreachable_probe(Some(bot.to_owned()), err.to_string()),
    }
}

/// Milliseconds since `started`, saturating — a duration that overflows an
/// `i64` of milliseconds is not a measurement worth panicking over.
fn elapsed_ms(started: Instant) -> i64 {
    i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)
}

// ---------------------------------------------------------------------------
// Wire shapes
// ---------------------------------------------------------------------------

/// `{"version":"0.33.2"}` from Ollama's `/api/version`, and the same key on
/// Hermes' `/health` (R2 §3, R1 §10.3). Every other field is ignored.
#[derive(Debug, Deserialize)]
struct VersionBody {
    #[serde(default)]
    version: Option<String>,
}

/// `POST /api/show` request body. `model` is the current spelling; the
/// deprecated `name` field is not sent.
#[derive(Debug, serde::Serialize)]
struct ShowRequest<'a> {
    model: &'a str,
}

#[derive(Debug, Deserialize)]
struct ShowBody {
    #[serde(default)]
    details: Option<TagDetails>,
    #[serde(default)]
    capabilities: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct TagsBody {
    #[serde(default)]
    models: Vec<TagRow>,
}

/// One `/api/tags` row. `capabilities` is `Option` on purpose and is the whole
/// point of the tri-state: a build that predates the field simply does not send
/// it (R2 §4.1).
#[derive(Debug, Deserialize)]
struct TagRow {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    size: Option<i64>,
    #[serde(default)]
    details: Option<TagDetails>,
    #[serde(default)]
    capabilities: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct TagDetails {
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    parameter_size: Option<String>,
    #[serde(default)]
    quantization_level: Option<String>,
}

/// `{"object":"list","data":[{"id":"hermes-agent",…}]}` (R1 §2.9).
#[derive(Debug, Deserialize)]
struct HermesModelsBody {
    #[serde(default)]
    data: Vec<HermesModelRow>,
}

#[derive(Debug, Deserialize)]
struct HermesModelRow {
    #[serde(default)]
    id: String,
}

/// `/api/model/options`: `{"providers":[{"slug","name","models":[…]}],…}`
/// (R1 §4.3). A top-level `models` array is accepted as well, because the
/// payload is built by one shared inventory function whose shape the docs do
/// not fix, and reading a list that is there costs nothing.
#[derive(Debug, Default, Deserialize)]
struct HermesOptionsBody {
    #[serde(default)]
    providers: Vec<HermesOptionsProvider>,
    #[serde(default)]
    models: Vec<HermesOptionsModel>,
}

#[derive(Debug, Deserialize)]
struct HermesOptionsProvider {
    #[serde(default)]
    models: Vec<HermesOptionsModel>,
}

/// A model entry from `/api/model/options`.
///
/// The capability keys are read from a nested `capabilities` object *and* from
/// the entry itself, in that order of preference: the dashboard's own type
/// nests them (`ModelsAnalyticsModelEntry.capabilities`, R1 §4.3), and a
/// deployment's `model_overrides` may patch them in flatter shapes. Neither
/// spelling being present leaves every flag `None`.
#[derive(Debug, Deserialize)]
struct HermesOptionsModel {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    capabilities: Option<HermesCapabilities>,
    #[serde(default, flatten)]
    flat: HermesCapabilities,
}

#[derive(Debug, Default, Deserialize)]
struct HermesCapabilities {
    #[serde(default)]
    supports_tools: Option<bool>,
    #[serde(default)]
    supports_vision: Option<bool>,
    #[serde(default)]
    supports_reasoning: Option<bool>,
    #[serde(default)]
    context_window: Option<i64>,
    #[serde(default)]
    max_output_tokens: Option<i64>,
    #[serde(default)]
    model_family: Option<String>,
}

/// The flattened enrichment for one model id.
#[derive(Debug, Clone)]
struct HermesHint {
    id: String,
    family: Option<String>,
    context_window: Option<i64>,
    max_output_tokens: Option<i64>,
    vision: Option<bool>,
    tools: Option<bool>,
    reasoning: Option<bool>,
}

impl HermesOptionsModel {
    fn into_hint(self) -> Option<HermesHint> {
        let id = self.id.or(self.model).or(self.name)?;
        let nested = self.capabilities.unwrap_or_default();
        let flat = self.flat;
        Some(HermesHint {
            id,
            family: nested.model_family.or(flat.model_family),
            context_window: nested.context_window.or(flat.context_window),
            max_output_tokens: nested.max_output_tokens.or(flat.max_output_tokens),
            vision: nested.supports_vision.or(flat.supports_vision),
            tools: nested.supports_tools.or(flat.supports_tools),
            reasoning: nested.supports_reasoning.or(flat.supports_reasoning),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule the whole story turns on, at the one function that decides it:
    /// no capability list means unknown, and unknown is never `false`.
    #[test]
    fn an_absent_capability_list_is_unknown_not_false() {
        let (vision, tools, reasoning, list) = capability_flags(None);
        assert_eq!(vision, None, "absent list must not answer vision");
        assert_eq!(tools, None, "absent list must not answer tools");
        assert_eq!(reasoning, None, "absent list must not answer reasoning");
        assert!(list.is_empty());
    }

    /// `omitempty` means a server with capabilities never sends `[]`, so an
    /// empty array is the same silence as an absent key.
    #[test]
    fn an_empty_capability_list_is_also_unknown() {
        assert_eq!(capability_flags(Some(&[])).0, None);
    }

    /// A list that is present speaks for every flag, including the negatives.
    #[test]
    fn a_present_capability_list_answers_all_three() {
        let list = vec!["completion".to_owned(), "vision".to_owned()];
        let (vision, tools, reasoning, kept) = capability_flags(Some(&list));
        assert_eq!(vision, Some(true));
        assert_eq!(tools, Some(false), "stated absence is false, not unknown");
        assert_eq!(reasoning, Some(false));
        assert_eq!(kept, list, "unknown vocabulary is kept verbatim");
    }

    #[test]
    fn a_404_is_absence_and_a_401_is_not() {
        assert_eq!(hermes_presence(404, "ghost").0, BotPresence::Absent);
        assert_eq!(hermes_presence(401, "ghost").0, BotPresence::Unknown);
        assert_eq!(hermes_presence(403, "ghost").0, BotPresence::Unknown);
        assert_eq!(hermes_presence(500, "ghost").0, BotPresence::Unknown);
        assert_eq!(hermes_presence(200, "ghost").0, BotPresence::Exists);
    }
}
