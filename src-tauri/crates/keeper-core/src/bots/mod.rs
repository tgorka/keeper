//! The model-runtime half that still lives in keeper (Epic 61; Story 108.3a).
//!
//! The ⌘9 bots surface is gone (Story 108.3a, FR-865): its pane, its IPC, its
//! settings section, its voice target and its scheduled task. What remains in
//! this module tree is what `keeper-agent`'s runtime half and
//! `keeper_core::agents`' runtime-decision modules still import — the turn
//! loop's sessions, grants, audit, tools, context files and deliverables —
//! and it leaves keeper with them (108.3b moves the runtime to keeper-hq,
//! 108.3c deletes it here, R-HQ-67). Nothing in the shell or the frontend
//! calls it any more.
//!
//! The provider half — the registry, the grammar, discovery and embeddings —
//! is [`crate::providers`], which notes search keeps. It is re-exported here
//! under its old paths for that runtime alone, so the code that leaves with
//! 108.3c is not rewritten on its way out.

pub mod audit;
pub mod context_files;
pub mod deliverable;
pub mod grant;
pub mod identity;
pub mod remote;
pub mod session;
pub mod store;
pub mod tools;

pub use crate::providers::{discover, embed};
pub use crate::providers::{
    delete_provider_token, provider_token_key, save_provider_token, Provider, ProviderHealth,
    ProviderHealthState,
};
pub use keeper_llm::{chat, error, http, quirks, sse, url};
pub use keeper_llm::{
    parse_base_url, parse_bot_target, BaseUrl, BaseUrlError, BotTargetError, Endpoint, ProviderKind,
};

use crate::error::CoreError;
use crate::platform::Platform;

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
/// [`ProviderHealthState::SecretMissing`] against the row, which is what stops the
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
    crate::providers::resolve_credential_with(platform, http, account, provider_id, || {
        resolve_token(platform, provider_id, bot)
    })
    .await
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

    /// A bot's key is namespaced per provider and per bot, so removing one bot
    /// deletes exactly one secret.
    #[test]
    fn a_bot_secret_key_is_namespaced_by_provider_and_bot() {
        assert_eq!(
            bot_token_key("01ARZ3NDEKTSV4RRFFQ69G5FAV", "grokbot"),
            "bot_token/01ARZ3NDEKTSV4RRFFQ69G5FAV/grokbot"
        );
    }
}
