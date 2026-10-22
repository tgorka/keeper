//! Model providers — the endpoints keeper calls for notes search's embeddings
//! (Story 61.1; carved out of the retired ⌘9 surface by Story 108.3a, R-HQ-12).
//!
//! A provider is one endpoint and one credential, side by side with others of
//! the same kind: two Ollama servers, or a work Hermes and a home Hermes, are
//! two rows and never a mode switch. Everything decided about a provider is
//! decided in this module tree (AD-55/AD-56): the base-URL grammar
//! ([`url`]), the persistence ([`store`]), the request-URL assembly
//! ([`Endpoint::url`]), what the endpoint can do ([`discover`]), the embedding
//! call ([`embed`]) and where a credential lives ([`provider_token_key`]). The
//! `keeper` shell reads a row, asks the secret port for the token, builds an
//! [`Endpoint`] and calls — it makes no decision of its own.
//!
//! **The credential never travels with the row.** A provider's token goes to
//! the [`crate::platform::Platform`] keychain port keyed by provider id, and
//! the base-URL grammar refuses userinfo outright, so there is no shape in
//! which a token reaches `keeper.db`, a screen, an error string or the egress
//! disclosure (FR-370, AD-147).
//!
//! The stored names — the `bot_providers` table, the `bot_provider_token/`
//! keychain prefix, the `bots.provider_credential_source.` settings family and
//! the account's `bots.toml` — keep their spelling for every device still on
//! an older build (DW-E108-6).

pub mod discover;
pub mod embed;
pub mod store;

pub use keeper_llm::{error, http, quirks, url};
pub use keeper_llm::{parse_base_url, BaseUrl, BaseUrlError, Endpoint, ProviderKind};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::CoreError;
use crate::platform::Platform;

/// A configured provider: one endpoint, one credential (Story 61.1, FR-369).
///
/// **No credential and no health here.** The token lives behind the secret port
/// ([`provider_token_key`]); the health snapshot is a mutable observation about
/// this row rather than part of its identity, so it is read alongside it as
/// [`store::ProviderRow`]. What remains is exactly what the person typed and
/// cannot be re-derived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    /// Opaque ULID, minted once. The credential's keychain key is built from
    /// it, so it must survive a rename of anything user-visible.
    pub id: String,
    /// Which back end this is.
    pub kind: ProviderKind,
    /// The person's display name. Free text — it names the tenant ("work
    /// Hermes"), not the endpoint.
    pub name: String,
    /// The normalized base URL ([`BaseUrl::normalized`]), never the raw typed
    /// string: the store holds only URLs that passed the grammar.
    pub base_url: String,
    /// When the row was written, `i64` ms since the Unix epoch (UTC).
    pub created_ms: i64,
}

impl Provider {
    /// The endpoint this provider is reached at, optionally addressing one
    /// `target` behind it (a Hermes profile), with the `token` the caller read
    /// from the secret port. The model client reads only the kind and the base
    /// URL; the registry's identity, name and age stay here.
    pub fn endpoint(&self, target: Option<&str>, token: Option<String>) -> Endpoint {
        Endpoint::new(self.kind, &self.base_url, target, token)
    }
}

/// What keeper last learned about a provider by talking to it (Story 61.1).
///
/// Serializes camelCase; stored as the `bot_providers.health_state` scalar.
/// Kept as a scalar column rather than a JSON snapshot because AD-139 bars a
/// blob in a row, and because a state the SQL can filter on is a state the
/// surface can list without reading every row into memory.
///
/// [`ProviderHealthState::Unknown`] is the default and the honest answer
/// before a probe: a capability keeper could not read is `unknown` and never
/// `false`, and a provider keeper has not called yet is the same shape of fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ProviderHealthState {
    /// Never probed, or probed and the answer could not be classified.
    Unknown,
    /// The endpoint answered as itself.
    Reachable,
    /// The endpoint could not be reached at all (connection refused, DNS, TLS).
    Unreachable,
    /// The endpoint answered `401`/`403`: it is there, and the credential is
    /// not the one it wants.
    Unauthorized,
    /// keeper holds a row for this provider but the secret port has no token
    /// for it — a keychain entry deleted, or a database restored without one.
    ///
    /// A distinct state because the alternative is failing at call time, and a
    /// person who finds out their provider has no credential *while* waiting
    /// for an answer has been told too late (FR-370).
    SecretMissing,
}

impl ProviderHealthState {
    /// The string stored in `bot_providers.health_state` and sent over IPC.
    pub fn as_registry_str(&self) -> &'static str {
        match self {
            ProviderHealthState::Unknown => "unknown",
            ProviderHealthState::Reachable => "reachable",
            ProviderHealthState::Unreachable => "unreachable",
            ProviderHealthState::Unauthorized => "unauthorized",
            ProviderHealthState::SecretMissing => "secretMissing",
        }
    }

    /// Parse a stored `health_state`, defaulting to
    /// [`ProviderHealthState::Unknown`] for an absent or unrecognized value.
    ///
    /// Total on purpose, unlike [`ProviderKind::from_registry_str`]: a health
    /// state keeper cannot read is precisely "keeper does not know", so the
    /// default is the truth rather than a fallback. A `kind` has no such
    /// harmless default, which is why the two differ.
    pub fn from_registry_str(value: &str) -> Self {
        match value {
            "reachable" => ProviderHealthState::Reachable,
            "unreachable" => ProviderHealthState::Unreachable,
            "unauthorized" => ProviderHealthState::Unauthorized,
            "secretMissing" => ProviderHealthState::SecretMissing,
            _ => ProviderHealthState::Unknown,
        }
    }
}

/// A provider's health snapshot, as three scalars (Story 61.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderHealth {
    /// The verdict.
    pub state: ProviderHealthState,
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
            state: ProviderHealthState::Unknown,
            checked_ms: None,
            detail: None,
        }
    }
}

/// The secret-port key holding a provider's credential (Story 61.1, FR-370,
/// AD-147).
///
/// Namespaced by provider id, in the `<concern>/<id>` shape the existing keys
/// use (`session/<ulid>`, `store_passphrase/<ulid>`,
/// `recovery_key/<account>`), so removing one provider deletes exactly one
/// secret and a key can never be mistaken for another concern's. The prefix
/// keeps its old spelling: a keychain entry is not renamed under a person who
/// saved it (DW-E108-6).
pub fn provider_token_key(provider_id: &str) -> String {
    format!("bot_provider_token/{provider_id}")
}

/// Read a provider's own credential from the secret port. `Ok(None)` is a real
/// answer — an Ollama on loopback needs none — not a failure.
pub fn resolve_token(
    platform: &dyn Platform,
    provider_id: &str,
) -> Result<Option<String>, CoreError> {
    platform.keychain_get(&provider_token_key(provider_id))
}

/// The bearer token for `provider_id`, honouring the provider's credential
/// source (Epic 82, AD-315).
///
/// `account` is the shell's in-memory view of the configured account. With
/// none, this is exactly [`resolve_token`]: no registry row is read, so an
/// install without an account pays nothing and cannot fail on the database.
/// A provider opted into *this* account
/// (`bots.provider_credential_source.<id> = account:<account id>`) sends the
/// account's access token — refreshed as needed, never stored under the
/// provider — and fails with the account's own error when it cannot give one,
/// so the caller can say "Sign in again" rather than "the token went missing".
pub async fn resolve_credential(
    platform: &dyn Platform,
    http: &reqwest::Client,
    account: Option<&crate::org_account::descriptor::AccountDescriptor>,
    provider_id: &str,
) -> Result<Option<String>, crate::org_account::AccountError> {
    resolve_credential_with(platform, http, account, provider_id, || {
        resolve_token(platform, provider_id)
    })
    .await
}

/// [`resolve_credential`] with the keychain read supplied by the caller — the
/// one place the account-or-keychain decision is made, whichever key the
/// keychain half reads.
pub async fn resolve_credential_with(
    platform: &dyn Platform,
    http: &reqwest::Client,
    account: Option<&crate::org_account::descriptor::AccountDescriptor>,
    provider_id: &str,
    keychain: impl FnOnce() -> Result<Option<String>, CoreError>,
) -> Result<Option<String>, crate::org_account::AccountError> {
    use crate::org_account::AccountError;
    let internal = |e: CoreError| AccountError::Internal(e.to_string());
    let Some(account) = account else {
        return keychain().map_err(internal);
    };
    let data_dir = platform.data_dir().map_err(internal)?;
    let source = crate::registry::get_bots_provider_credential_source(
        &data_dir,
        provider_id,
        Some(&account.id),
    )
    .map_err(internal)?;
    if source.is_none() {
        return keychain().map_err(internal);
    }
    crate::org_account::oidc::access_token(platform, http, account)
        .await
        .map(Some)
}

/// Store a provider's credential behind the secret port (Story 61.1, FR-370).
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

/// Delete a provider's credential (Story 61.1).
///
/// Idempotent, like every other keychain delete in the tree: removing a secret
/// that was never written is not an error, so this is safe on the rollback path
/// of a half-finished provider add.
pub fn delete_provider_token(platform: &dyn Platform, provider_id: &str) -> Result<(), CoreError> {
    platform.keychain_delete(&provider_token_key(provider_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keychain key is namespaced per provider, so removing one provider
    /// deletes exactly one secret — under the spelling older builds wrote.
    #[test]
    fn the_secret_key_is_namespaced_by_provider() {
        assert_eq!(
            provider_token_key("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            "bot_provider_token/01ARZ3NDEKTSV4RRFFQ69G5FAV"
        );
    }

    /// Health round-trips, and anything unreadable reads back as `Unknown` —
    /// never as a state keeper did not observe.
    #[test]
    fn health_round_trips_and_defaults_to_unknown() {
        for state in [
            ProviderHealthState::Unknown,
            ProviderHealthState::Reachable,
            ProviderHealthState::Unreachable,
            ProviderHealthState::Unauthorized,
            ProviderHealthState::SecretMissing,
        ] {
            assert_eq!(
                ProviderHealthState::from_registry_str(state.as_registry_str()),
                state
            );
        }
        assert_eq!(
            ProviderHealthState::from_registry_str("degraded"),
            ProviderHealthState::Unknown
        );
        assert_eq!(ProviderHealth::unknown().state, ProviderHealthState::Unknown);
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
            serde_json::to_string(&ProviderHealthState::SecretMissing).expect("serialize state"),
            "\"secretMissing\""
        );
    }
}
