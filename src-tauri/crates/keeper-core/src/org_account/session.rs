//! The stored account session: its keychain shape, the identity read back from
//! it without the network, the single-flight lock `access_token()` refreshes
//! under, and the neutral git credential the shell hands to `keeper-sync`
//! (AD-310, AD-312).
//!
//! Tokens live in exactly two keychain items per account —
//! `account/<id>/session` and `account/<id>/forge` — and nowhere else: not on
//! disk, not over IPC, not in a log line. The stored shapes therefore have no
//! `Debug` implementation.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use super::descriptor::{AccountDescriptor, ForgeOauth, GitScheme, RepoAuthConfig};
use super::AccountError;
use crate::platform::Platform;

/// Who is signed in, as keeper keys the account: the (`iss`, `sub`) pair plus
/// what the UI shows. Read from the verified ID token (or sub-checked
/// UserInfo), never from the access token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub iss: String,
    pub sub: String,
    pub login: String,
    pub display_name: String,
    pub email: Option<String>,
    pub roles: Vec<String>,
}

/// What a keychain item was issued for. An item is only ever used with the
/// descriptor it was minted under: a setup link or a hand edit that keeps the
/// account `id` but names another issuer, client, token endpoint or
/// repository must not receive the old tokens.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Binding {
    issuer: Option<String>,
    client_id: String,
    token_endpoint: Option<String>,
    /// The forge item only: the repository its token is sent to.
    repo_url: Option<String>,
}

impl Binding {
    pub(crate) fn session(d: &AccountDescriptor) -> Self {
        Binding {
            issuer: Some(d.auth.issuer.clone()),
            client_id: d.auth.client_id.clone(),
            token_endpoint: d.auth.endpoints.token.clone(),
            repo_url: None,
        }
    }

    pub(crate) fn forge(d: &AccountDescriptor, forge: &ForgeOauth) -> Self {
        Binding {
            issuer: forge.issuer.clone(),
            client_id: forge.client_id.clone(),
            token_endpoint: forge.token_url.clone(),
            repo_url: Some(d.config.url.clone()),
        }
    }
}

/// The `account/<id>/session` keychain item.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct StoredSession {
    pub binding: Binding,
    pub iss: String,
    pub sub: String,
    pub refresh_token: Option<String>,
    pub access_token: String,
    /// When `access_token` stops being accepted, ms since the Unix epoch.
    pub access_expires_ms: Option<i64>,
    pub id_token: String,
    pub login: String,
    pub display_name: String,
    pub email: Option<String>,
    pub roles: Vec<String>,
}

impl StoredSession {
    pub(crate) fn identity(&self) -> Identity {
        Identity {
            iss: self.iss.clone(),
            sub: self.sub.clone(),
            login: self.login.clone(),
            display_name: self.display_name.clone(),
            email: self.email.clone(),
            roles: self.roles.clone(),
        }
    }

    /// The descriptor's current `required_role` against the stored roles.
    pub(crate) fn check_roles(&self, d: &AccountDescriptor) -> Result<(), AccountError> {
        super::claims::require_role(&self.roles, d.auth.required_role.as_deref(), &d.name)
    }
}

/// The `account/<id>/forge` keychain item (`oauth` mode only).
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct StoredForge {
    pub binding: Binding,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_ms: Option<i64>,
    pub login: String,
}

pub(crate) fn session_key(account_id: &str) -> String {
    format!("account/{account_id}/session")
}

pub(crate) fn forge_key(account_id: &str) -> String {
    format!("account/{account_id}/forge")
}

fn keychain_error(error: crate::error::CoreError) -> AccountError {
    AccountError::Internal(format!("The keychain could not be used: {error}"))
}

fn read_item<T: for<'de> Deserialize<'de>>(
    platform: &dyn Platform,
    key: &str,
) -> Result<Option<T>, AccountError> {
    let Some(raw) = platform.keychain_get(key).map_err(keychain_error)? else {
        return Ok(None);
    };
    serde_json::from_str(&raw).map(Some).map_err(|_| {
        // The item is ours and unreadable: a sign-in writes a fresh one.
        AccountError::NeedsSignIn("Sign in again to keep your settings in sync.".to_owned())
    })
}

fn write_item<T: Serialize>(
    platform: &dyn Platform,
    key: &str,
    item: &T,
) -> Result<(), AccountError> {
    let raw = serde_json::to_string(item)
        .map_err(|e| AccountError::Internal(format!("could not encode the session: {e}")))?;
    platform.keychain_set(key, &raw).map_err(keychain_error)
}

pub(crate) fn load_session(
    platform: &dyn Platform,
    account_id: &str,
) -> Result<Option<StoredSession>, AccountError> {
    read_item(platform, &session_key(account_id))
}

pub(crate) fn store_session(
    platform: &dyn Platform,
    account_id: &str,
    session: &StoredSession,
) -> Result<(), AccountError> {
    write_item(platform, &session_key(account_id), session)
}

pub(crate) fn load_forge(
    platform: &dyn Platform,
    account_id: &str,
) -> Result<Option<StoredForge>, AccountError> {
    read_item(platform, &forge_key(account_id))
}

pub(crate) fn store_forge(
    platform: &dyn Platform,
    account_id: &str,
    forge: &StoredForge,
) -> Result<(), AccountError> {
    write_item(platform, &forge_key(account_id), forge)
}

/// Delete both keychain items. Every delete is attempted; the first failure is
/// reported after the second has run.
pub(crate) fn delete_all(platform: &dyn Platform, account_id: &str) -> Result<(), AccountError> {
    let session = platform.keychain_delete(&session_key(account_id));
    let forge = platform.keychain_delete(&forge_key(account_id));
    session.and(forge).map_err(keychain_error)
}

/// Delete only the forge item, so the next interactive sign-in connects the
/// forge again (a forge that rejects its token, a different person).
pub fn forget_forge(platform: &dyn Platform, account_id: &str) -> Result<(), AccountError> {
    platform
        .keychain_delete(&forge_key(account_id))
        .map_err(keychain_error)
}

/// Whether a stored item may be used with `expected`; a mismatched item is
/// deleted, and the person signs in again.
fn bound<T>(
    platform: &dyn Platform,
    key: &str,
    item: Option<T>,
    binding: impl Fn(&T) -> &Binding,
    expected: &Binding,
    sentence: &str,
) -> Result<Option<T>, AccountError> {
    match item {
        Some(item) if binding(&item) != expected => {
            tracing::warn!(item = %key, "keychain item belongs to another sign-in setup; deleted");
            platform.keychain_delete(key).map_err(keychain_error)?;
            Err(AccountError::NeedsSignIn(sentence.to_owned()))
        }
        other => Ok(other),
    }
}

/// The session item, when it was issued under `d`'s sign-in setup.
pub(crate) fn load_bound_session(
    platform: &dyn Platform,
    d: &AccountDescriptor,
) -> Result<Option<StoredSession>, AccountError> {
    bound(
        platform,
        &session_key(&d.id),
        load_session(platform, &d.id)?,
        |s| &s.binding,
        &Binding::session(d),
        "Sign in again to keep your settings in sync.",
    )
}

/// The forge item, when it was issued under `d`'s forge and repository.
pub(crate) fn load_bound_forge(
    platform: &dyn Platform,
    d: &AccountDescriptor,
) -> Result<Option<StoredForge>, AccountError> {
    let RepoAuthConfig::Oauth(forge) = &d.config.auth else {
        return Ok(None);
    };
    bound(
        platform,
        &forge_key(&d.id),
        load_forge(platform, &d.id)?,
        |f| &f.binding,
        &Binding::forge(d, forge),
        "Reconnect the repository.",
    )
}

/// Whether the forge leg (`oauth` mode) holds a token issued for `d`'s forge
/// and repository, without the network. Anything else reads as not
/// connected (a mismatched item is deleted).
pub fn forge_connected(platform: &dyn Platform, d: &AccountDescriptor) -> bool {
    matches!(load_bound_forge(platform, d), Ok(Some(_)))
}

/// The signed-in identity from the stored session, without the network.
/// `Ok(None)` when nobody is signed in.
///
/// A session issued under another sign-in setup is deleted and needs a sign-in;
/// stored roles that no longer satisfy `d`'s `required_role` are `Refused`, so
/// a restore never installs the account's layers for someone the descriptor
/// now shuts out.
pub fn identity(
    platform: &dyn Platform,
    d: &AccountDescriptor,
) -> Result<Option<Identity>, AccountError> {
    let Some(session) = load_bound_session(platform, d)? else {
        return Ok(None);
    };
    session.check_roles(d)?;
    Ok(Some(session.identity()))
}

/// The neutral git credential for the config repository, shaped by the
/// descriptor's `config.auth`. `token` is the sign-in access token in `same`
/// mode and the forge token in `oauth` mode.
#[derive(Clone, PartialEq, Eq)]
pub enum GitAuth {
    None,
    Basic { username: String, password: String },
    Bearer(String),
}

impl std::fmt::Debug for GitAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitAuth::None => f.write_str("None"),
            GitAuth::Basic { username, .. } => f
                .debug_struct("Basic")
                .field("username", username)
                .field("password", &"<redacted>")
                .finish(),
            GitAuth::Bearer(_) => f.write_str("Bearer(<redacted>)"),
        }
    }
}

/// Map `config.auth` onto a [`GitAuth`].
///
/// `oauth` mode is Basic with the forge token as the password: Gitea and
/// Forgejo take a token in the password slot whatever the username says, and
/// `oauth2` is the spelling GitLab requires.
pub fn git_credential(d: &AccountDescriptor, token: &str) -> GitAuth {
    match &d.config.auth {
        RepoAuthConfig::None => GitAuth::None,
        RepoAuthConfig::Same {
            scheme: GitScheme::Basic,
            username,
        } => GitAuth::Basic {
            username: username.clone(),
            password: token.to_owned(),
        },
        RepoAuthConfig::Same {
            scheme: GitScheme::Bearer,
            ..
        } => GitAuth::Bearer(token.to_owned()),
        RepoAuthConfig::Oauth(_) => GitAuth::Basic {
            username: "oauth2".to_owned(),
            password: token.to_owned(),
        },
    }
}

/// The per-keychain-item refresh lock.
///
/// Refresh tokens rotate on every provider keeper targets, and a rotated
/// token replayed is a dead grant (authentik logs it as suspicious). Two
/// concurrent refreshes of one item would therefore sign the person out, so
/// every refresh of one item runs under this lock and re-reads the keychain
/// once it holds it — the second caller finds the first caller's token.
pub(crate) fn refresh_lock(item_key: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: LazyLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
        LazyLock::new(Default::default);
    let mut locks = LOCKS.lock().unwrap_or_else(PoisonError::into_inner);
    Arc::clone(locks.entry(item_key.to_owned()).or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::org_account::descriptor;

    fn descriptor_with(auth: &str) -> AccountDescriptor {
        descriptor::parse_json(&format!(
            r#"{{"version":1,"id":"acme","name":"Acme",
                "auth":{{"issuer":"https://id.acme.dev","client_id":"keeper"}},
                "config":{{"url":"https://git.acme.dev/c.git","api_base":"https://git.acme.dev/api/v1",
                           "auth":{auth}}}}}"#
        ))
        .expect("a valid descriptor")
    }

    #[test]
    fn git_credential_follows_the_repo_auth_mode() {
        assert_eq!(
            git_credential(&descriptor_with(r#"{"mode":"same"}"#), "t"),
            GitAuth::Basic {
                username: "oauth2".to_owned(),
                password: "t".to_owned()
            }
        );
        assert_eq!(
            git_credential(
                &descriptor_with(r#"{"mode":"same","scheme":"bearer"}"#),
                "t"
            ),
            GitAuth::Bearer("t".to_owned())
        );
        assert_eq!(
            git_credential(&descriptor_with(r#"{"mode":"none"}"#), "t"),
            GitAuth::None
        );
    }

    #[test]
    fn git_auth_debug_never_prints_the_token() {
        let shown = format!(
            "{:?} {:?}",
            GitAuth::Basic {
                username: "u".to_owned(),
                password: "secret-token".to_owned()
            },
            GitAuth::Bearer("secret-token".to_owned())
        );
        assert!(!shown.contains("secret-token"), "{shown}");
    }
}
