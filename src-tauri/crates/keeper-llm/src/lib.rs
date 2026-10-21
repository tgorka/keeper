//! keeper-llm — talking to an OpenAI-compatible model: the provider kinds and
//! their quirks, the base-URL grammar, the HTTP client, the SSE framer, the
//! streamed chat completion and the tool loop that drives one to an answer.
//!
//! An open crate with no keeper dependency, so keeper's app and an agent
//! runtime built elsewhere speak to a model through one implementation.
//! `cargo tree -p keeper-llm` names no `keeper-*` crate, no matrix-sdk, no
//! SQLite, no gitoxide and no tauri (`bun run check:llm-lean`). keeper-core
//! re-exports every module here at its old `keeper_core::bots::*` path.

pub mod chat;
pub mod error;
pub mod http;
pub mod quirks;
pub mod sse;
pub mod tools;
pub mod url;

pub use crate::url::{parse_base_url, parse_bot_target, BaseUrl, BaseUrlError, BotTargetError};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Which OpenAI-compatible back end a provider is (Story 61.1, FR-369;
/// AD-369).
///
/// A closed set of three, deliberately. AD-146 closed it at two and named its
/// own revisit trigger: a third kind with a real endpoint to read. CLIProxyAPI
/// met it, so [`ProviderKind::OpenAi`] is one row and one match arm on this
/// foundation; a kind designed against no endpoint (the owner named `omp`)
/// stays out (DW-214). The kind is what decides how a *bot* is addressed, so
/// every place that branches on it is a place where the wire dialects
/// genuinely differ.
///
/// Serializes to `"hermes" | "ollama" | "openai"` — the frontend wire
/// contract, and the same string the `bot_providers.kind` column stores, so
/// there is one spelling of each kind across the product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ProviderKind {
    /// A Hermes agent gateway's api-server (research §2.2): bearer-authenticated,
    /// loopback by default, with every route mirrored at `/p/{profile}`.
    Hermes,
    /// An Ollama server (research §3.1), local or remote. Its `/v1` layer takes
    /// no credential of its own — a token, when present, was added by a proxy in
    /// front of it (research §3.5).
    Ollama,
    /// Any OpenAI-compatible endpoint (CLIProxyAPI, research §11.1):
    /// bearer-authenticated, the model in the body, its models listed at
    /// `/v1/models` with no capabilities.
    #[serde(rename = "openai")]
    OpenAi,
}

impl ProviderKind {
    /// The string stored in `bot_providers.kind` and sent over IPC. Kept as one
    /// function so the column, the wire and any log line can never disagree.
    pub fn as_registry_str(&self) -> &'static str {
        match self {
            ProviderKind::Hermes => "hermes",
            ProviderKind::Ollama => "ollama",
            ProviderKind::OpenAi => "openai",
        }
    }

    /// Parse a stored `kind` back, or `None` for a value this build does not
    /// know.
    ///
    /// `None` rather than a default: a row written by a newer build names a kind
    /// this one cannot speak, and answering "Hermes" for it would send a Hermes
    /// request to something else. The reader surfaces the row as unreadable
    /// instead — the same posture `keeper_core::tasks` takes for an unknown task
    /// row.
    pub fn from_registry_str(value: &str) -> Option<Self> {
        match value {
            "hermes" => Some(ProviderKind::Hermes),
            "ollama" => Some(ProviderKind::Ollama),
            "openai" => Some(ProviderKind::OpenAi),
            _ => None,
        }
    }
}

/// Everything one request needs: a provider's kind and base URL, a chosen bot
/// and the provider's secret (Story 61.1).
///
/// **Why this type exists at all.** The alternative is passing a base URL, a
/// kind and an optional profile to every function in `bots::http` and
/// `bots::discover` and hoping each one joins them the same way. `Endpoint` is
/// the one place the join happens ([`Endpoint::url`]), so a route the app calls
/// is a route this module composed — the same reason `keeper-sync` gives its
/// path types a single constructor.
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// Which dialect the caller is speaking.
    pub kind: ProviderKind,
    /// The provider's normalized base URL.
    pub base_url: String,
    /// The bot to address, when one is chosen. For Hermes this becomes the
    /// `/p/{bot}` path prefix; for Ollama it is carried for the request *body*
    /// and takes no part in routing.
    pub bot: Option<String>,
    /// The bearer token, when one is held. `None` is legitimate: a loopback
    /// Ollama has no credential at all (research §3.5), and a Hermes gateway
    /// with no `API_SERVER_KEY` configured allows everything (research §8.1) —
    /// so an absent token is a fact about the endpoint, not a missing field.
    ///
    /// The value is never logged and never formatted into a URL. The header it
    /// becomes is marked sensitive by `bots::http`, so it is redacted from
    /// error text (Story 61.2).
    pub token: Option<String>,
}

impl Endpoint {
    /// Assemble an endpoint of `kind` at `base_url` (normalized by the
    /// grammar), optionally addressing `bot`, with the `token` the caller read
    /// from the secret port.
    ///
    /// The token is a parameter rather than something read here because
    /// keeper reaches the OS only through its platform port (AD-24) and an
    /// `Endpoint` is built on the request path, where a keychain round trip
    /// per call would be a dialog per call on some platforms. The shell reads
    /// it once — see `keeper_core::bots::resolve_token` — and hands it in.
    pub fn new(
        kind: ProviderKind,
        base_url: &str,
        bot: Option<&str>,
        token: Option<String>,
    ) -> Self {
        Endpoint {
            kind,
            base_url: base_url.to_owned(),
            bot: bot.map(str::to_owned),
            token,
        }
    }

    /// Join `path` (which starts with `/`) onto the base URL, inserting the
    /// Hermes profile prefix `/p/{bot}` when this is a Hermes endpoint
    /// addressing a bot.
    ///
    /// **The prefix is the whole of "bot mode" on the wire.** Hermes registers
    /// every api-server route a second time at `/p/{profile}<path>` (research
    /// §2.2), so addressing a bot is a URL decision and nothing else — no
    /// second client, no body field, no header. Ollama and an OpenAI-compatible
    /// endpoint have no such concept: their bot is the `model` in the request
    /// body, so `bot` is deliberately ignored for routing there rather than
    /// silently prefixed onto a path their server would 404.
    ///
    /// The base URL's trailing slash was already dropped by the grammar, but it
    /// is trimmed again here: this function is total over any base URL a caller
    /// hands it, including one restored from an older row, and a doubled slash
    /// in a path is a real routing difference to some proxies.
    pub fn url(&self, path: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        let prefix = match (self.kind, self.bot.as_deref()) {
            (ProviderKind::Hermes, Some(bot)) => Some(bot),
            _ => None,
        };
        let mut out = String::with_capacity(
            base.len() + prefix.map_or(0, |bot| bot.len() + 3) + path.len() + 1,
        );
        out.push_str(base);
        if let Some(bot) = prefix {
            out.push_str("/p/");
            out.push_str(bot);
        }
        if !path.starts_with('/') {
            out.push('/');
        }
        out.push_str(path);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `/p/{bot}` prefix is the whole of bot mode on the wire, so it is
    /// asserted on the exact route Story 61.3 probes a profile with.
    #[test]
    fn a_hermes_bot_is_addressed_by_the_profile_prefix() {
        let endpoint = Endpoint::new(
            ProviderKind::Hermes,
            "http://127.0.0.1:8642",
            Some("grokbot"),
            None,
        );
        assert_eq!(
            endpoint.url("/v1/models"),
            "http://127.0.0.1:8642/p/grokbot/v1/models"
        );
        assert_eq!(
            endpoint.url("/v1/chat/completions"),
            "http://127.0.0.1:8642/p/grokbot/v1/chat/completions"
        );
    }

    /// The same provider with no bot chosen addresses the gateway's own default
    /// profile — the prefix appears only when a bot does.
    #[test]
    fn a_hermes_endpoint_without_a_bot_has_no_prefix() {
        let endpoint = Endpoint::new(ProviderKind::Hermes, "http://127.0.0.1:8642", None, None);
        assert_eq!(
            endpoint.url("/v1/models"),
            "http://127.0.0.1:8642/v1/models"
        );
    }

    /// Ollama routes by body, not by path: a bot must not become a path prefix
    /// its server would 404.
    #[test]
    fn an_ollama_endpoint_ignores_the_bot_for_routing() {
        let endpoint = Endpoint::new(
            ProviderKind::Ollama,
            "http://localhost:11434",
            Some("llama3.1:8b"),
            None,
        );
        assert_eq!(
            endpoint.url("/v1/chat/completions"),
            "http://localhost:11434/v1/chat/completions"
        );
        assert_eq!(endpoint.url("/api/tags"), "http://localhost:11434/api/tags");
        assert_eq!(
            endpoint.bot.as_deref(),
            Some("llama3.1:8b"),
            "the bot is still carried — the request BODY needs it"
        );
    }

    /// A base URL with a trailing slash, and one with a path prefix, both join
    /// without doubling or dropping a separator. The row should never hold a
    /// trailing slash (the grammar strips it), so this pins the behaviour for a
    /// row restored from anywhere else.
    #[test]
    fn joining_never_doubles_or_drops_a_separator() {
        let trailing = Endpoint::new(ProviderKind::Ollama, "http://localhost:11434/", None, None);
        assert_eq!(
            trailing.url("/v1/models"),
            "http://localhost:11434/v1/models"
        );

        let prefixed = Endpoint::new(
            ProviderKind::Ollama,
            "https://gw.example.org/ollama",
            None,
            None,
        );
        assert_eq!(
            prefixed.url("/v1/models"),
            "https://gw.example.org/ollama/v1/models"
        );

        let prefixed_hermes = Endpoint::new(
            ProviderKind::Hermes,
            "https://gw.example.org/hermes/",
            Some("grokbot"),
            None,
        );
        assert_eq!(
            prefixed_hermes.url("/v1/models"),
            "https://gw.example.org/hermes/p/grokbot/v1/models"
        );

        // Total over a path that forgot its leading slash: one separator, not
        // zero and not two.
        assert_eq!(
            trailing.url("v1/models"),
            "http://localhost:11434/v1/models"
        );
    }

    /// Every `kind` spelling round-trips — through the column and through the
    /// IPC wire, which must be the same word — and an unknown one is refused
    /// rather than defaulted: a row from a newer build must not be read as
    /// Hermes, and `omp` stays closed (DW-214).
    #[test]
    fn a_provider_kind_round_trips_and_an_unknown_kind_is_refused() {
        for (kind, word) in [
            (ProviderKind::Hermes, "hermes"),
            (ProviderKind::Ollama, "ollama"),
            (ProviderKind::OpenAi, "openai"),
        ] {
            assert_eq!(kind.as_registry_str(), word);
            assert_eq!(ProviderKind::from_registry_str(word), Some(kind));
            let wire = serde_json::to_string(&kind).expect("a kind serializes");
            assert_eq!(wire, format!("\"{word}\""));
            let back: ProviderKind = serde_json::from_str(&wire).expect("a kind deserializes");
            assert_eq!(back, kind);
        }
        assert_eq!(ProviderKind::from_registry_str("omp"), None);
        assert_eq!(ProviderKind::from_registry_str("openAi"), None);
        assert_eq!(ProviderKind::from_registry_str(""), None);
    }
}
