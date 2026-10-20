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
pub mod commands;
pub mod context_files;
pub mod deliverable;
pub mod discover;
pub mod embed;
pub mod follow;
pub mod grant;
pub mod identity;
pub mod remote;
pub mod session;
pub mod store;
pub mod tools;
pub mod voice_target;

pub use keeper_llm::{chat, error, http, quirks, sse, url};
pub use keeper_llm::{
    parse_base_url, parse_bot_target, BaseUrl, BaseUrlError, BotTargetError, Endpoint, Provider,
    ProviderKind,
};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::CoreError;
use crate::platform::Platform;

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

/// The bearer token for `(provider_id, bot)`, honouring the provider's
/// credential source (Epic 82, AD-315).
///
/// `account` is the shell's in-memory view of the configured account. With
/// none, this is exactly [`resolve_token`]: no registry row is read, so an
/// install without an account pays nothing and cannot fail on the database.
/// A provider opted into *this* account
/// (`bots.provider_credential_source.<id> = account:<account id>`) sends the
/// account's access token — refreshed as needed, never stored under the
/// provider — and fails with the account's own error when it cannot give one,
/// so the caller can say "Sign in again" rather than "the token went missing".
/// A row bound to another account is not this account's to answer.
pub async fn resolve_credential(
    platform: &dyn Platform,
    http: &reqwest::Client,
    account: Option<&crate::org_account::descriptor::AccountDescriptor>,
    provider_id: &str,
    bot: Option<&str>,
) -> Result<Option<String>, crate::org_account::AccountError> {
    use crate::org_account::AccountError;
    let internal = |e: CoreError| AccountError::Internal(e.to_string());
    let Some(account) = account else {
        return resolve_token(platform, provider_id, bot).map_err(internal);
    };
    let data_dir = platform.data_dir().map_err(internal)?;
    let source = crate::registry::get_bots_provider_credential_source(
        &data_dir,
        provider_id,
        Some(&account.id),
    )
    .map_err(internal)?;
    if source.is_none() {
        return resolve_token(platform, provider_id, bot).map_err(internal);
    }
    crate::org_account::oidc::access_token(platform, http, account)
        .await
        .map(Some)
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
