//! One way to get a forge token (AD-334): the broker, the account's forge
//! sign-in, or a device-flow connection in the keychain.

use serde::{Deserialize, Serialize};

use super::broker::{self, BrokerFailure};
use super::source::{remote_on_source, sources, ForgeSource, TokenVia, GITHUB_ID};
use super::{device_flow, now_ms};
use crate::org_account::descriptor::AccountDescriptor;
use crate::org_account::session::{self, refresh_lock};
use crate::org_account::{oidc, AccountError};
use crate::platform::Platform;

/// Why no token (or no listing): each a finished, secret-free sentence.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ForgeError {
    /// A device-flow source with no (or a dead) connection.
    #[error("Connect to see your repositories.")]
    NeedsConnect,
    /// The account's sign-in (or its forge leg) needs repeating.
    #[error("{0}")]
    NeedsSignIn(String),
    #[error("{0}")]
    Unreachable(String),
    /// The broker gives this person no GitHub access at all: an
    /// administrator's decision, not an outage, so no old list is shown.
    #[error("{0}")]
    NoAccess(String),
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Internal(String),
}

impl From<AccountError> for ForgeError {
    fn from(error: AccountError) -> Self {
        match error {
            AccountError::NeedsSignIn(s) => ForgeError::NeedsSignIn(s),
            AccountError::Unreachable(s) => ForgeError::Unreachable(s),
            AccountError::Refused(s) => ForgeError::Refused(s),
            AccountError::Cancelled => ForgeError::NeedsSignIn("Sign in again.".to_owned()),
            AccountError::Internal(s) => ForgeError::Internal(s),
        }
    }
}

/// The `forge/<id>/<client id>/session` keychain item.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredForgeToken {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// When `access_token` stops working, ms since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_ms: Option<i64>,
    /// Empty until GitHub has said who is connected; the next listing asks.
    pub login: String,
    /// The OAuth client it was issued to; an item for another client is
    /// never used.
    pub client_id: String,
}

impl std::fmt::Debug for StoredForgeToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredForgeToken")
            .field("login", &self.login)
            .field("expires_ms", &self.expires_ms)
            .finish_non_exhaustive()
    }
}

/// Refresh this long before a token's stated expiry.
const REFRESH_EARLY_MS: i64 = 60_000;

/// One item per source and OAuth client: keeper's own GitHub connection and
/// a descriptor's `github` entry with its own client never share one.
pub fn session_key(source_id: &str, client_id: &str) -> String {
    format!("forge/{source_id}/{client_id}/session")
}

/// Where a connection lived before items were kept per client. Never read;
/// Disconnect removes it.
fn legacy_session_key(source_id: &str) -> String {
    format!("forge/{source_id}/session")
}

fn keychain_error(error: crate::error::CoreError) -> ForgeError {
    ForgeError::Internal(format!("The keychain could not be used: {error}"))
}

/// The device-flow connection for `source` and its client. Reading never
/// deletes: an item keeper cannot use reads as none and stays until
/// Disconnect.
pub fn load_session(
    platform: &dyn Platform,
    source: &ForgeSource,
) -> Result<Option<StoredForgeToken>, ForgeError> {
    let Some(client_id) = source.client_id.as_deref() else {
        return Ok(None);
    };
    let key = session_key(&source.id, client_id);
    let Some(raw) = platform.keychain_get(&key).map_err(keychain_error)? else {
        return Ok(None);
    };
    match serde_json::from_str::<StoredForgeToken>(&raw) {
        Ok(stored) if stored.client_id == client_id => Ok(Some(stored)),
        _ => {
            tracing::warn!(item = %key, "forge connection keeper cannot use; ignored");
            Ok(None)
        }
    }
}

/// Keep a connection under the client that issued it.
pub fn store_session(
    platform: &dyn Platform,
    source: &ForgeSource,
    token: &StoredForgeToken,
) -> Result<(), ForgeError> {
    let raw = serde_json::to_string(token)
        .map_err(|e| ForgeError::Internal(format!("could not encode the connection: {e}")))?;
    platform
        .keychain_set(&session_key(&source.id, &token.client_id), &raw)
        .map_err(keychain_error)?;
    super::listing::forget(&source.id);
    Ok(())
}

/// Whether this device holds a connection `source` can use, without the
/// network.
pub fn has_connection(platform: &dyn Platform, source: &ForgeSource) -> bool {
    matches!(load_session(platform, source), Ok(Some(_)))
}

/// Disconnect: delete the keychain item. GitHub has no revocation keeper can
/// call without a client secret; the sheet links to its settings instead.
pub fn forge_disconnect(platform: &dyn Platform, source: &ForgeSource) -> Result<(), ForgeError> {
    super::listing::forget(&source.id);
    let legacy = platform.keychain_delete(&legacy_session_key(&source.id));
    if let Some(client_id) = source.client_id.as_deref() {
        platform
            .keychain_delete(&session_key(&source.id, client_id))
            .map_err(keychain_error)?;
    }
    legacy.map_err(keychain_error)
}

/// Forget account: the connections of every source the descriptor names.
/// keeper's own GitHub connection stays until Disconnect.
pub fn forget_descriptor_sessions(platform: &dyn Platform, descriptor: &AccountDescriptor) {
    let builtin = super::BUILTIN_GITHUB_CLIENT_ID;
    for source in sources(Some(descriptor), builtin) {
        let keepers_own = source.id == GITHUB_ID && source.client_id.as_deref() == builtin;
        if source.via == TokenVia::AccountForge || keepers_own {
            continue;
        }
        if let Err(error) = forge_disconnect(platform, &source) {
            tracing::warn!(%error, source = %source.id, "could not delete a forge connection");
        }
    }
}

fn signed_in(platform: &dyn Platform, d: &AccountDescriptor) -> bool {
    matches!(session::identity(platform, d), Ok(Some(_)))
}

/// Whether this device can get a token from `source` without asking the
/// person anything: a device-flow connection exists, or the account (the
/// broker's or the forge's) is signed in.
pub fn can_get_token(
    platform: &dyn Platform,
    source: &ForgeSource,
    account: Option<&AccountDescriptor>,
) -> bool {
    match source.via {
        TokenVia::DeviceFlow => has_connection(platform, source),
        TokenVia::Broker => {
            account.is_some_and(|d| signed_in(platform, d))
                || (source.own_via() == Some(TokenVia::DeviceFlow)
                    && has_connection(platform, source))
        }
        TokenVia::AccountForge => {
            account.is_some_and(|d| signed_in(platform, d) && session::forge_connected(platform, d))
        }
    }
}

/// `(source id, web_base origin)` of every source a pulled `forge:<id>` may
/// bind a drive to here (`settings_sync::Catalog::forge_origins`): one this
/// device can get a token from. A value naming any other stays unapplied,
/// so it never replaces a drive's working credential with one that fails.
pub fn token_origins(
    platform: &dyn Platform,
    descriptor: Option<&AccountDescriptor>,
    builtin_github_client_id: Option<&str>,
) -> Vec<(String, String)> {
    sources(descriptor, builtin_github_client_id)
        .into_iter()
        .filter(|source| {
            source.via != TokenVia::AccountForge && can_get_token(platform, source, descriptor)
        })
        .filter_map(|source| {
            let origin = source.origin()?;
            Some((source.id, origin))
        })
        .collect()
}

/// A token for `source`: the broker's is per owner and per repository
/// ([`drive_token`], the listing), so a broker source here falls back to its
/// own device-flow connection when it has one.
pub async fn forge_token(
    platform: &dyn Platform,
    http: &reqwest::Client,
    source: &ForgeSource,
    account: Option<&AccountDescriptor>,
) -> Result<String, ForgeError> {
    match source.via {
        TokenVia::AccountForge => {
            let d = account.ok_or_else(no_account)?;
            Ok(oidc::forge_token(platform, http, d).await?)
        }
        TokenVia::DeviceFlow => device_token(platform, http, source).await,
        TokenVia::Broker => match source.own_via() {
            Some(TokenVia::DeviceFlow) => device_token(platform, http, source).await,
            _ => Err(ForgeError::Internal(
                "a broker source gets its tokens per repository".to_owned(),
            )),
        },
    }
}

fn no_account() -> ForgeError {
    ForgeError::NeedsSignIn("Sign in to your account to see its repositories.".to_owned())
}

/// The token a drive at `remote_url` syncs with, from `source`, only when
/// [`remote_on_source`]. Through the broker it is a token for that one
/// repository: `contents: write` when a grant allows it, else read.
pub async fn drive_token(
    platform: &dyn Platform,
    http: &reqwest::Client,
    source: &ForgeSource,
    account: Option<&AccountDescriptor>,
    remote_url: &str,
) -> Result<String, ForgeError> {
    let wrong_place = || {
        ForgeError::NeedsSignIn(format!(
            "This drive's remote is not on {}, so keeper does not send it your {} connection.",
            source.host(),
            source.name
        ))
    };
    if !remote_on_source(source, remote_url) {
        return Err(wrong_place());
    }
    if source.via != TokenVia::Broker {
        return forge_token(platform, http, source, account).await;
    }
    let remote = url::Url::parse(remote_url.trim()).map_err(|_| wrong_place())?;
    let (owner, repo) = github_repository(&remote).ok_or_else(wrong_place)?;
    let d = account.ok_or_else(no_account)?;
    let broker_host = d
        .github_broker
        .as_ref()
        .map(|b| b.host())
        .unwrap_or_default();
    match broker::mint(
        platform,
        http,
        d,
        &owner,
        Some(std::slice::from_ref(&repo)),
        broker::DRIVE_ASKS,
    )
    .await
    {
        Ok(token) => Ok(token),
        Err(BrokerFailure::NoGrants) if source.own_via() == Some(TokenVia::DeviceFlow) => {
            device_token(platform, http, source).await
        }
        Err(BrokerFailure::NoGrants) => Err(ForgeError::NoAccess(broker::no_access(&broker_host))),
        Err(BrokerFailure::Owner(problem, app)) => Err(ForgeError::Refused(problem.sentence(
            &owner,
            app.as_deref(),
            &broker_host,
        ))),
        Err(BrokerFailure::Error(error)) => Err(error),
    }
}

/// `(owner, repository)` of `https://github.com/<owner>/<repo>(.git)`.
fn github_repository(remote: &url::Url) -> Option<(String, String)> {
    let mut segments = remote.path_segments()?.filter(|s| !s.is_empty());
    let owner = segments.next()?;
    let repo = segments.next()?;
    if segments.next().is_some() {
        return None;
    }
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    (!repo.is_empty()).then(|| (owner.to_owned(), repo.to_owned()))
}

/// The device-flow token, refreshed 60 s early when it expires; one refresh
/// per source at a time. A rejected refresh (or none possible) clears the
/// connection.
pub(crate) async fn device_token(
    platform: &dyn Platform,
    http: &reqwest::Client,
    source: &ForgeSource,
) -> Result<String, ForgeError> {
    let fresh = |stored: &StoredForgeToken| {
        stored
            .expires_ms
            .is_none_or(|at| at - REFRESH_EARLY_MS > now_ms())
    };
    let client_id = source
        .client_id
        .as_deref()
        .ok_or(ForgeError::NeedsConnect)?;
    if let Some(stored) = load_session(platform, source)? {
        if fresh(&stored) {
            return Ok(stored.access_token);
        }
    }
    let lock = refresh_lock(&session_key(&source.id, client_id));
    let _held = lock.lock().await;
    let stored = load_session(platform, source)?.ok_or(ForgeError::NeedsConnect)?;
    if fresh(&stored) {
        return Ok(stored.access_token);
    }
    let Some(refresh) = stored.refresh_token.clone() else {
        forge_disconnect(platform, source)?;
        return Err(ForgeError::NeedsConnect);
    };
    match device_flow::refresh(http, source, &refresh).await {
        Ok(tokens) => {
            let renewed = StoredForgeToken {
                access_token: tokens.access_token,
                // A refresh retires both tokens; keep the old one only when
                // the forge sent none.
                refresh_token: tokens.refresh_token.or(Some(refresh)),
                expires_ms: tokens.expires_ms,
                ..stored
            };
            store_session(platform, source, &renewed)?;
            Ok(renewed.access_token)
        }
        Err(ForgeError::NeedsConnect) => {
            forge_disconnect(platform, source)?;
            Err(ForgeError::NeedsConnect)
        }
        Err(other) => Err(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forges::testing::{self, Reply};
    use crate::forges::ForgeKind;

    fn device_source(web_base: &str) -> ForgeSource {
        ForgeSource {
            id: "gh-test".to_owned(),
            kind: ForgeKind::Github,
            name: "GitHub".to_owned(),
            web_base: web_base.to_owned(),
            api_base: format!("{web_base}/api"),
            client_id: Some("Iv1.c".to_owned()),
            via: TokenVia::DeviceFlow,
        }
    }

    fn stored(expires_ms: Option<i64>, refresh: Option<&str>) -> StoredForgeToken {
        StoredForgeToken {
            access_token: "old".to_owned(),
            refresh_token: refresh.map(str::to_owned),
            expires_ms,
            login: "tg".to_owned(),
            client_id: "Iv1.c".to_owned(),
        }
    }

    #[tokio::test]
    async fn an_expiring_device_token_is_refreshed_and_both_tokens_replaced() {
        let fake = testing::serve(|seen| {
            assert_eq!(seen.path, "/login/oauth/access_token");
            let form = seen.form();
            assert_eq!(
                form.get("grant_type").map(String::as_str),
                Some("refresh_token")
            );
            assert_eq!(form.get("refresh_token").map(String::as_str), Some("r1"));
            assert_eq!(form.get("client_id").map(String::as_str), Some("Iv1.c"));
            assert!(!form.contains_key("client_secret"));
            Reply::json(
                200,
                r#"{"access_token":"new","refresh_token":"r2","expires_in":28800}"#,
            )
        });
        let p = testing::FakePlatform::default();
        let source = device_source(&fake.base);
        store_session(&p, &source, &stored(Some(now_ms() + 30_000), Some("r1"))).expect("seed");
        let token = forge_token(&p, &testing::http(), &source, None)
            .await
            .expect("refreshed");
        assert_eq!(token, "new");
        let kept = load_session(&p, &source).expect("read").expect("kept");
        assert_eq!(kept.refresh_token.as_deref(), Some("r2"));
        assert!(kept.expires_ms.is_some_and(|at| at > now_ms() + 60_000));
        assert_eq!(fake.requests().len(), 1);

        // Fresh now: no second request.
        forge_token(&p, &testing::http(), &source, None)
            .await
            .expect("cached");
        assert_eq!(fake.requests().len(), 1);
    }

    #[tokio::test]
    async fn a_rejected_refresh_clears_the_connection_and_needs_connect() {
        let fake = testing::serve(|_| Reply::json(200, r#"{"error":"bad_refresh_token"}"#));
        let p = testing::FakePlatform::default();
        let source = device_source(&fake.base);
        store_session(&p, &source, &stored(Some(now_ms() - 1), Some("r1"))).expect("seed");
        assert_eq!(
            forge_token(&p, &testing::http(), &source, None).await,
            Err(ForgeError::NeedsConnect)
        );
        assert_eq!(load_session(&p, &source).expect("read"), None);
    }

    #[tokio::test]
    async fn a_connection_for_another_client_is_never_used_nor_deleted_by_reading() {
        let p = testing::FakePlatform::default();
        let mut source = device_source("https://github.com");
        store_session(&p, &source, &stored(None, None)).expect("seed");
        source.client_id = Some("Iv1.other".to_owned());
        assert_eq!(
            forge_token(&p, &testing::http(), &source, None).await,
            Err(ForgeError::NeedsConnect)
        );
        assert!(!has_connection(&p, &source));
        source.client_id = Some("Iv1.c".to_owned());
        assert!(
            has_connection(&p, &source),
            "the other client's connection is still there"
        );
        // keeper's own client and a descriptor's keep one item each.
        let other = ForgeSource {
            client_id: Some("Iv1.other".to_owned()),
            ..source.clone()
        };
        let token = StoredForgeToken {
            client_id: "Iv1.other".to_owned(),
            ..stored(None, None)
        };
        store_session(&p, &other, &token).expect("second client");
        assert!(has_connection(&p, &source) && has_connection(&p, &other));
    }

    #[test]
    fn only_sources_this_device_can_get_a_token_from_may_bind_a_pulled_drive() {
        let extra = r#", "github_broker": { "url": "https://b.acme.dev" },
            "forges": [ { "kind": "github", "id": "ghe", "web_base": "https://ghe.acme.dev",
                          "api_base": "https://ghe.acme.dev/api/v3", "client_id": "Iv1.ghe" } ]"#;
        let signed_in = testing::FakePlatform::default();
        let d = testing::signed_in_account(&signed_in, extra);
        let fresh = testing::FakePlatform::default();
        assert!(token_origins(&fresh, Some(&d), None).is_empty());
        let ghe = crate::forges::find(&sources(Some(&d), None), "ghe")
            .cloned()
            .expect("ghe");
        let token = StoredForgeToken {
            client_id: "Iv1.ghe".to_owned(),
            ..stored(None, None)
        };
        store_session(&fresh, &ghe, &token).expect("seed");
        assert_eq!(
            token_origins(&fresh, Some(&d), None),
            [("ghe".to_owned(), "https://ghe.acme.dev".to_owned())]
        );
        assert_eq!(
            token_origins(&signed_in, Some(&d), None),
            [("github".to_owned(), "https://github.com".to_owned())]
        );
    }

    #[tokio::test]
    async fn a_drive_token_goes_only_to_the_source_s_own_origin_over_https() {
        let p = testing::FakePlatform::default();
        let source = ForgeSource {
            web_base: "https://github.com".to_owned(),
            ..device_source("https://github.com")
        };
        store_session(&p, &source, &stored(None, None)).expect("seed");
        for remote in [
            "https://github.com/o/r",
            "https://github.com/o/r.git",
            "https://GitHub.com/o/r.git",
            "https://github.com:443/o/r.git",
        ] {
            assert_eq!(
                drive_token(&p, &testing::http(), &source, None, remote)
                    .await
                    .as_deref(),
                Ok("old"),
                "{remote}"
            );
        }
        for remote in [
            "http://github.com/o/r.git",
            "https://github.com:8443/o/r.git",
            "https://github.com.evil.com/o/r.git",
            "https://github.com@evil.com/o/r.git",
            "https://github.com./o/r.git",
            "https://gitlab.com/o/r.git",
            "git@github.com:o/r.git",
        ] {
            assert!(
                matches!(
                    drive_token(&p, &testing::http(), &source, None, remote).await,
                    Err(ForgeError::NeedsSignIn(_))
                ),
                "{remote}"
            );
        }
    }

    #[test]
    fn a_github_remote_names_its_owner_and_repository() {
        for (remote, want) in [
            ("https://github.com/o/r", Some(("o", "r"))),
            ("https://github.com/o/r.git", Some(("o", "r"))),
            ("https://github.com/o/r/", Some(("o", "r"))),
            ("https://github.com/o", None),
            ("https://github.com/o/r/tree/main", None),
        ] {
            let url = url::Url::parse(remote).expect("url");
            assert_eq!(
                github_repository(&url),
                want.map(|(o, r)| (o.to_owned(), r.to_owned())),
                "{remote}"
            );
        }
    }
}
