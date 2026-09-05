//! Bots — the vocabulary keeper uses to talk to a model (Epic 61, Story 61.1).
//!
//! **A bot is (provider, model-or-profile, identity).** That single definition
//! is what makes one surface serve two back ends: a Hermes bot is a *profile*,
//! addressed by the `/p/{profile}` URL prefix every api-server route is
//! registered under a second time (research §2.2, §2.6), and an Ollama bot is a
//! *model tag*, which its `/v1` layer takes in the request body and not in the
//! path (research §3.1). The divergence is therefore data — which field of an
//! [`Endpoint`] is populated — and never a second code path, which is the
//! epic's first decision.
//!
//! Everything decided about a provider is decided in this module tree
//! (AD-55/AD-56): the base-URL grammar ([`url`]), the persistence
//! ([`store`]), the request-URL assembly ([`Endpoint::url`]) and where a
//! credential lives ([`provider_token_key`]). The `keeper` shell reads a row,
//! asks the secret port for the token, builds an [`Endpoint`] and calls — it
//! makes no decision of its own.
//!
//! **The credential never travels with the row.** A provider's token goes to
//! the [`crate::platform::Platform`] keychain port keyed by provider id, and
//! the base-URL grammar refuses userinfo outright, so there is no shape in
//! which a token reaches `keeper.db`, a screen, an error string or the egress
//! disclosure (FR-370, AD-147).

pub mod audit;
pub mod chat;
pub mod commands;
pub mod context_files;
pub mod deliverable;
pub mod discover;
pub mod error;
pub mod follow;
pub mod grant;
pub mod http;
pub mod identity;
pub mod quirks;
pub mod remote;
pub mod session;
pub mod sse;
pub mod store;
pub mod tools;
pub mod url;
pub mod voice_target;

pub use url::{parse_base_url, parse_bot_target, BaseUrl, BaseUrlError, BotTargetError};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::CoreError;
use crate::platform::Platform;

/// Which of the two OpenAI-compatible back ends a provider is (Story 61.1,
/// FR-369).
///
/// A closed set of two, deliberately. A third kind (the owner named `omp`) is
/// one row and one match arm on this foundation, and inventing an abstraction
/// for it today — with no endpoint to read — is the mistake the epic refuses
/// (DW-214). The kind is what decides how a *bot* is addressed, so every place
/// that branches on it is a place where the two wire dialects genuinely differ.
///
/// Serializes to `"hermes" | "ollama"` — the frontend wire contract, and the
/// same string the `bot_providers.kind` column stores, so there is one spelling
/// of each kind across the product.
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
}

impl ProviderKind {
    /// The string stored in `bot_providers.kind` and sent over IPC. Kept as one
    /// function so the column, the wire and any log line can never disagree.
    pub fn as_registry_str(&self) -> &'static str {
        match self {
            ProviderKind::Hermes => "hermes",
            ProviderKind::Ollama => "ollama",
        }
    }

    /// Parse a stored `kind` back, or `None` for a value this build does not
    /// know.
    ///
    /// `None` rather than a default: a row written by a newer build names a kind
    /// this one cannot speak, and answering "Hermes" for it would send a Hermes
    /// request to something else. The reader surfaces the row as unreadable
    /// instead — the same posture [`crate::tasks`] takes for an unknown task
    /// row.
    pub fn from_registry_str(value: &str) -> Option<Self> {
        match value {
            "hermes" => Some(ProviderKind::Hermes),
            "ollama" => Some(ProviderKind::Ollama),
            _ => None,
        }
    }
}

/// A configured provider: one endpoint, one credential, side by side with
/// others of the same kind (Story 61.1, FR-369).
///
/// The multi-tenant shape is `SyncProfile`'s, which is the tree's only
/// precedent for "several endpoints of the same kind, each with its own
/// credential" (research §8.1) — so two Ollama servers, or a work Hermes and a
/// home Hermes, are two rows and never a mode switch.
///
/// **No credential and no health here.** The token lives behind the secret port
/// ([`provider_token_key`]); the health snapshot is a mutable observation about
/// this row rather than part of its identity, so it is read alongside it as
/// [`store::ProviderRow`]. What remains is exactly what the user typed and
/// cannot be re-derived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    /// Opaque ULID, minted once. The credential's keychain key and every bot's
    /// `provider_id` are built from it, so it must survive a rename of anything
    /// user-visible.
    pub id: String,
    /// Which back end this is.
    pub kind: ProviderKind,
    /// The user's display name. Free text — it names the tenant ("work
    /// Hermes"), not the endpoint.
    pub name: String,
    /// The normalized base URL ([`BaseUrl::normalized`]), never the raw typed
    /// string: the store holds only URLs that passed the grammar.
    pub base_url: String,
    /// When the row was written, `i64` ms since the Unix epoch (UTC).
    pub created_ms: i64,
}

/// What keeper last learned about a provider by talking to it (Story 61.1).
///
/// Serializes camelCase; stored as the `bot_providers.health_state` scalar.
/// Kept as a scalar column rather than a JSON snapshot because AD-139 bars a
/// blob in a row, and because a state the SQL can filter on is a state the
/// surface can list without reading every row into memory.
///
/// [`BotHealthState::Unknown`] is the default and the honest answer before a
/// probe: the epic's rule is that a capability keeper could not read is
/// `unknown` and never `false`, and a provider keeper has not called yet is the
/// same shape of fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BotHealthState {
    /// Never probed, or probed and the answer could not be classified.
    Unknown,
    /// The endpoint answered as itself.
    Reachable,
    /// The endpoint could not be reached at all (connection refused, DNS, TLS).
    Unreachable,
    /// The endpoint answered `401`/`403`: it is there, and the credential is
    /// not the one it wants. On a Hermes profile prefix this is its own fact —
    /// a named prefix requires that profile's own key and rejects the default
    /// one (research §9.2).
    Unauthorized,
    /// keeper holds a row for this provider but the secret port has no token
    /// for it — a keychain entry deleted, or a database restored without one.
    ///
    /// A distinct state because the alternative is failing at send time, and a
    /// person who finds out their provider has no credential *while* waiting
    /// for an answer has been told too late (FR-370).
    SecretMissing,
}

impl BotHealthState {
    /// The string stored in `bot_providers.health_state` and sent over IPC.
    pub fn as_registry_str(&self) -> &'static str {
        match self {
            BotHealthState::Unknown => "unknown",
            BotHealthState::Reachable => "reachable",
            BotHealthState::Unreachable => "unreachable",
            BotHealthState::Unauthorized => "unauthorized",
            BotHealthState::SecretMissing => "secretMissing",
        }
    }

    /// Parse a stored `health_state`, defaulting to [`BotHealthState::Unknown`]
    /// for an absent or unrecognized value.
    ///
    /// Total on purpose, unlike [`ProviderKind::from_registry_str`]: a health
    /// state keeper cannot read is precisely "keeper does not know", so the
    /// default is the truth rather than a fallback. A `kind` has no such
    /// harmless default, which is why the two differ.
    pub fn from_registry_str(value: &str) -> Self {
        match value {
            "reachable" => BotHealthState::Reachable,
            "unreachable" => BotHealthState::Unreachable,
            "unauthorized" => BotHealthState::Unauthorized,
            "secretMissing" => BotHealthState::SecretMissing,
            _ => BotHealthState::Unknown,
        }
    }
}

/// A provider's health snapshot, as three scalars (Story 61.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderHealth {
    /// The verdict.
    pub state: BotHealthState,
    /// When the verdict was reached, `i64` ms since the Unix epoch, or `None`
    /// when no probe has ever run. Never defaulted to now-or-zero: a timestamp
    /// keeper did not measure is the kind of number this app does not print.
    pub checked_ms: Option<i64>,
    /// The endpoint's own words, already stripped of anything credential-shaped
    /// by the caller that produced it. `None` when there is nothing to add
    /// beyond the state.
    pub detail: Option<String>,
}

impl ProviderHealth {
    /// The snapshot a row starts life with: nothing measured yet.
    pub fn unknown() -> Self {
        ProviderHealth {
            state: BotHealthState::Unknown,
            checked_ms: None,
            detail: None,
        }
    }
}

/// A bot's chosen identity: a shape, a colour token and a mark (Story 61.1,
/// FR-383).
///
/// Every field is optional here and stays optional in the row, because Story
/// 61.7 is what gives this a picker and a bounded, contrast-checked palette.
/// Until then a bot has no identity rather than a made-up one — an assigned
/// colour would have to be un-assigned later, and `DESIGN.md:172` requires a
/// colour to be paired with a shape, which is a pairing only the story that
/// ships both can guarantee.
///
/// Stored as three nullable scalar columns, never a blob (AD-139).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BotIdentity {
    /// A shape from the closed set Story 61.7 defines.
    pub shape: Option<String>,
    /// A design-token name from the bounded palette Story 61.7 defines — a
    /// token, never a hex value, so the check in `scripts/check-design.mjs`
    /// remains the single authority on what passes AA in both themes.
    pub colour: Option<String>,
    /// A short grapheme or the name of an existing `space-icons.ts` icon.
    pub mark: Option<String>,
}

impl BotIdentity {
    /// Whether the user has chosen anything at all. Cheap, and it keeps the
    /// "has an identity" question from being three `is_some()` calls at every
    /// call site.
    pub fn is_empty(&self) -> bool {
        self.shape.is_none() && self.colour.is_none() && self.mark.is_none()
    }
}

/// A pinned bot: which provider, which target, in which hand-set order (Story
/// 61.1, FR-383).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bot {
    /// Opaque ULID, minted once.
    pub id: String,
    /// The owning [`Provider::id`].
    pub provider_id: String,
    /// The kind-specific target: a Hermes **profile name** (which becomes the
    /// `/p/{profile}` prefix) or an Ollama **model tag** (which becomes the
    /// request body's `model`). One column for both because it is one concept —
    /// the thing on the far side that this bot *is* — and two nullable columns
    /// would let a row name neither or both.
    ///
    /// Always a value that passed [`parse_bot_target`]: for Hermes it lands in
    /// a URL path, so a `/` or a `..` here would retarget the request.
    pub target: String,
    /// The user's display name, free text.
    pub name: String,
    /// Position in the hand-set pin order, ascending. Rewritten wholesale by
    /// [`store::reorder_bots`] for `registry::reorder_pins`' reason: N
    /// independent writes leave a half-rewritten sequence when one fails.
    pub pin_order: i64,
    /// The chosen shape/colour/mark, all optional until Story 61.7.
    pub identity: BotIdentity,
    /// When the row was written, `i64` ms since the Unix epoch (UTC).
    pub created_ms: i64,
}

/// Everything one request needs, assembled from a [`Provider`], a chosen bot
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
    /// Assemble an endpoint for `provider`, optionally addressing `bot`, with
    /// the `token` the caller read from the secret port.
    ///
    /// The token is a parameter rather than something read here because this
    /// crate reaches the OS only through [`Platform`] (AD-24) and an
    /// `Endpoint` is built on the request path, where a keychain round trip
    /// per call would be a dialog per call on some platforms. The shell reads
    /// it once — see [`resolve_token`] — and hands it in.
    pub fn new(provider: &Provider, bot: Option<&str>, token: Option<String>) -> Self {
        Endpoint {
            kind: provider.kind,
            base_url: provider.base_url.clone(),
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
    /// second client, no body field, no header. Ollama has no such concept: its
    /// bot is the `model` in the request body, so `bot` is deliberately ignored
    /// for routing there rather than silently prefixed onto a path its server
    /// would 404.
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

/// The secret-port key holding a provider's default credential (Story 61.1,
/// FR-370, AD-147).
///
/// Namespaced by provider id, in the `<concern>/<id>` shape the existing keys
/// use (`session/<ulid>`, `store_passphrase/<ulid>`,
/// `recovery_key/<account>`), so removing one provider deletes exactly one
/// secret and a key can never be mistaken for another concern's.
pub fn provider_token_key(provider_id: &str) -> String {
    format!("bot_provider_token/{provider_id}")
}

/// The secret-port key holding the credential for one *bot* of a provider
/// (Story 61.1, FR-370, AD-147).
///
/// **Why a bot can need its own key.** On Hermes a request to `/p/<profile>/…`
/// must present that profile's own `API_SERVER_KEY` from
/// `~/.hermes/profiles/<profile>/.env`; since the July-2026 hardening the
/// gateway's default key is *rejected* on a named prefix, and a profile with no
/// key of its own fails closed (research §9.2). So the credential on Hermes is
/// per (provider, bot), and a design with one key per provider would be able to
/// reach the default profile only.
pub fn bot_token_key(provider_id: &str, bot: &str) -> String {
    format!("bot_token/{provider_id}/{bot}")
}

/// Read the token to send for `(provider_id, bot)`: the bot's own key when it
/// has one, else the provider's (Story 61.1, FR-370).
///
/// The fallback order is the one the far side demands. A Hermes profile that
/// has its own key must use it — the provider's default key would come back
/// `401` (research §9.2) — while a provider whose bots share one gateway key
/// stores it once. An Ollama provider has no per-bot notion at all, so the bot
/// key is simply never written and the fallback is the only path.
///
/// `Ok(None)` is a real answer, not a failure: see [`Endpoint::token`]. The
/// caller that needs to *distinguish* "no credential needed" from "the
/// credential went missing" persists that as
/// [`BotHealthState::SecretMissing`] against the row, which is what stops the
/// discovery happening at send time.
pub fn resolve_token(
    platform: &dyn Platform,
    provider_id: &str,
    bot: Option<&str>,
) -> Result<Option<String>, CoreError> {
    if let Some(bot) = bot {
        if let Some(token) = platform.keychain_get(&bot_token_key(provider_id, bot))? {
            return Ok(Some(token));
        }
    }
    platform.keychain_get(&provider_token_key(provider_id))
}

/// Store a provider's default credential behind the secret port (Story 61.1,
/// FR-370).
///
/// The only writer of that key, so "where is the token" has one answer. Never
/// logged, and the value is not echoed in any error this function can return.
pub fn save_provider_token(
    platform: &dyn Platform,
    provider_id: &str,
    token: &str,
) -> Result<(), CoreError> {
    platform.keychain_set(&provider_token_key(provider_id), token)
}

/// Store the credential for one bot of a provider (Story 61.1, FR-370) — the
/// Hermes named-profile case [`bot_token_key`] explains.
pub fn save_bot_token(
    platform: &dyn Platform,
    provider_id: &str,
    bot: &str,
    token: &str,
) -> Result<(), CoreError> {
    platform.keychain_set(&bot_token_key(provider_id, bot), token)
}

/// Delete a provider's default credential (Story 61.1).
///
/// Idempotent, like every other keychain delete in the tree: removing a secret
/// that was never written is not an error, so this is safe on the rollback path
/// of a half-finished provider add.
pub fn delete_provider_token(platform: &dyn Platform, provider_id: &str) -> Result<(), CoreError> {
    platform.keychain_delete(&provider_token_key(provider_id))
}

/// Delete one bot's credential (Story 61.1). Idempotent, as above.
pub fn delete_bot_token(
    platform: &dyn Platform,
    provider_id: &str,
    bot: &str,
) -> Result<(), CoreError> {
    platform.keychain_delete(&bot_token_key(provider_id, bot))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A provider fixture. `base_url` is what the grammar would have stored.
    fn provider(kind: ProviderKind, base_url: &str) -> Provider {
        Provider {
            id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
            kind,
            name: "work".to_owned(),
            base_url: base_url.to_owned(),
            created_ms: 1_700_000_000_000,
        }
    }

    /// The `/p/{bot}` prefix is the whole of bot mode on the wire, so it is
    /// asserted on the exact route Story 61.3 probes a profile with.
    #[test]
    fn a_hermes_bot_is_addressed_by_the_profile_prefix() {
        let endpoint = Endpoint::new(
            &provider(ProviderKind::Hermes, "http://127.0.0.1:8642"),
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
        let endpoint = Endpoint::new(
            &provider(ProviderKind::Hermes, "http://127.0.0.1:8642"),
            None,
            None,
        );
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
            &provider(ProviderKind::Ollama, "http://localhost:11434"),
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
        let trailing = Endpoint::new(
            &provider(ProviderKind::Ollama, "http://localhost:11434/"),
            None,
            None,
        );
        assert_eq!(
            trailing.url("/v1/models"),
            "http://localhost:11434/v1/models"
        );

        let prefixed = Endpoint::new(
            &provider(ProviderKind::Ollama, "https://gw.example.org/ollama"),
            None,
            None,
        );
        assert_eq!(
            prefixed.url("/v1/models"),
            "https://gw.example.org/ollama/v1/models"
        );

        let prefixed_hermes = Endpoint::new(
            &provider(ProviderKind::Hermes, "https://gw.example.org/hermes/"),
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

    /// The keychain keys are namespaced per provider and per bot, so removing
    /// one provider (or one bot) can delete exactly one secret.
    #[test]
    fn secret_keys_are_namespaced_by_provider_and_bot() {
        assert_eq!(
            provider_token_key("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            "bot_provider_token/01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
        assert_eq!(
            bot_token_key("01ARZ3NDEKTSV4RRFFQ69G5FAV", "grokbot"),
            "bot_token/01ARZ3NDEKTSV4RRFFQ69G5FAV/grokbot"
        );
    }

    /// Both `kind` spellings round-trip, and an unknown one is refused rather
    /// than defaulted — a row from a newer build must not be read as Hermes.
    #[test]
    fn a_provider_kind_round_trips_and_an_unknown_kind_is_refused() {
        for kind in [ProviderKind::Hermes, ProviderKind::Ollama] {
            assert_eq!(
                ProviderKind::from_registry_str(kind.as_registry_str()),
                Some(kind)
            );
        }
        assert_eq!(ProviderKind::from_registry_str("omp"), None);
        assert_eq!(ProviderKind::from_registry_str(""), None);
    }

    /// Health round-trips, and anything unreadable reads back as `Unknown` —
    /// never as a state keeper did not observe.
    #[test]
    fn health_round_trips_and_defaults_to_unknown() {
        for state in [
            BotHealthState::Unknown,
            BotHealthState::Reachable,
            BotHealthState::Unreachable,
            BotHealthState::Unauthorized,
            BotHealthState::SecretMissing,
        ] {
            assert_eq!(
                BotHealthState::from_registry_str(state.as_registry_str()),
                state
            );
        }
        assert_eq!(
            BotHealthState::from_registry_str("degraded"),
            BotHealthState::Unknown
        );
        assert_eq!(ProviderHealth::unknown().state, BotHealthState::Unknown);
        assert_eq!(ProviderHealth::unknown().checked_ms, None);
    }

    /// The wire tags the frontend switches on, asserted exactly — the ts-rs
    /// binding and the renderer must not drift.
    #[test]
    fn the_wire_tags_are_the_frontend_contract() {
        assert_eq!(
            serde_json::to_string(&ProviderKind::Hermes).expect("serialize hermes"),
            "\"hermes\""
        );
        assert_eq!(
            serde_json::to_string(&ProviderKind::Ollama).expect("serialize ollama"),
            "\"ollama\""
        );
        assert_eq!(
            serde_json::to_string(&BotHealthState::SecretMissing).expect("serialize state"),
            "\"secretMissing\""
        );
    }
}
