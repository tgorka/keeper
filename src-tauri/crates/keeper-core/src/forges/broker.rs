//! The organisation's GitHub broker (makistack `github-broker`, AD-334):
//! keeper shows it the account's access token and gets back GitHub App
//! installation tokens, one per (app, owner, repositories, permissions).
//!
//! `GET /v1/whoami` names what the person may reach; `POST /v1/token` mints
//! a token. Both answers and every token stay in this process's memory,
//! keyed by the signed-in person, and are served only while that person's
//! session exists.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use serde::Deserialize;

use super::tokens::ForgeError;
use super::{body, now_ms, send};
use crate::org_account::descriptor::{AccountDescriptor, GithubBroker};
use crate::org_account::{oidc, session};
use crate::platform::Platform;

/// Permissions one token request asks for.
pub type Permissions = &'static [(&'static str, &'static str)];

/// What a listing token asks for, in order of preference: the first a
/// grant allows. Every installation token can read metadata; `contents:
/// read` lists as well where a grant's ceiling has no `metadata`.
pub const LIST_ASKS: &[Permissions] = &[&[("metadata", "read")], &[("contents", "read")]];
/// What a drive's token asks for: write where a grant allows it, else read
/// (that drive only downloads).
pub const DRIVE_ASKS: &[Permissions] = &[&[("contents", "write")], &[("contents", "read")]];

/// A cached token is used until this long before it expires.
const TOKEN_EARLY_MS: i64 = 5 * 60 * 1000;

/// Which repositories of an owner a grant covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantRepositories {
    All,
    Only(Vec<String>),
}

impl<'de> Deserialize<'de> for GrantRepositories {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Star(String),
            List(Vec<String>),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Star(star) if star == "*" => Ok(GrantRepositories::All),
            Raw::Star(other) => Err(serde::de::Error::custom(format!(
                "repositories must be \"*\" or a list, not {other:?}"
            ))),
            Raw::List(names) => Ok(GrantRepositories::Only(names)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Grant {
    pub app: String,
    pub owners: Vec<String>,
    pub repositories: GrantRepositories,
    /// The ceiling: a request may ask for these levels or lower, and for
    /// nothing else.
    #[serde(default)]
    pub permissions: BTreeMap<String, String>,
}

fn level(name: &str) -> u8 {
    match name {
        "read" => 1,
        "write" => 2,
        "admin" => 3,
        _ => 0,
    }
}

impl Grant {
    /// Whether the broker's policy allows this request under this grant:
    /// the owner is named, the repository (when one is asked for) is in its
    /// list, and every permission is within its ceiling. GitHub logins and
    /// repository names ignore case.
    pub fn covers(&self, owner: &str, repository: Option<&str>, permissions: Permissions) -> bool {
        let owner_named = self.owners.iter().any(|o| o.eq_ignore_ascii_case(owner));
        let repository_listed = match (&self.repositories, repository) {
            (GrantRepositories::All, _) | (_, None) => true,
            (GrantRepositories::Only(names), Some(repository)) => {
                names.iter().any(|n| n.eq_ignore_ascii_case(repository))
            }
        };
        let within_ceiling = permissions.iter().all(|(name, wanted)| {
            let ceiling = self.permissions.get(*name).map_or(0, |l| level(l));
            level(wanted) > 0 && ceiling >= level(wanted)
        });
        owner_named && repository_listed && within_ceiling
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Whoami {
    pub sub: String,
    /// The broker's name for this person; `None` when its policy has none.
    pub subject: Option<String>,
    #[serde(default)]
    pub grants: Vec<Grant>,
}

impl Whoami {
    /// Every owner of every grant, once, A→Z (GitHub logins ignore case).
    pub fn owners(&self) -> Vec<String> {
        let mut owners: Vec<String> = self
            .grants
            .iter()
            .flat_map(|grant| grant.owners.iter().cloned())
            .collect();
        owners.sort_by_cached_key(|owner| owner.to_ascii_lowercase());
        owners.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        owners
    }
}

/// `GET <url>/v1/whoami`.
pub fn whoami_request(broker_url: &str) -> String {
    format!("{}/v1/whoami", broker_url.trim_end_matches('/'))
}

/// `POST <url>/v1/token`.
pub fn token_url(broker_url: &str) -> String {
    format!("{}/v1/token", broker_url.trim_end_matches('/'))
}

/// The `POST /v1/token` body: every repository of the owner the grant allows
/// when `repositories` is `None`.
pub fn token_request(
    app: &str,
    owner: &str,
    repositories: Option<&[String]>,
    permissions: Permissions,
) -> serde_json::Value {
    let mut body = serde_json::json!({ "app": app, "owner": owner });
    if let Some(repositories) = repositories {
        body["repositories"] = serde_json::json!(repositories);
    }
    if !permissions.is_empty() {
        let map: serde_json::Map<String, serde_json::Value> = permissions
            .iter()
            .map(|(name, level)| ((*name).to_owned(), (*level).into()))
            .collect();
        body["permissions"] = serde_json::Value::Object(map);
    }
    body
}

pub fn parse_whoami(body: &[u8]) -> Result<Whoami, ForgeError> {
    serde_json::from_slice(body).map_err(|_| {
        ForgeError::Refused("The GitHub broker sent an answer keeper could not read.".to_owned())
    })
}

/// A minted installation token.
#[derive(Clone, PartialEq, Eq)]
pub struct BrokerToken {
    pub token: String,
    pub expires_ms: i64,
}

impl std::fmt::Debug for BrokerToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrokerToken")
            .field("expires_ms", &self.expires_ms)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct RawToken {
    token: String,
    expires_at: String,
}

/// `{token, expires_at: RFC 3339, …}`.
pub fn parse_token(body: &[u8]) -> Result<BrokerToken, ForgeError> {
    let unreadable =
        || ForgeError::Refused("The GitHub broker sent a token keeper could not read.".to_owned());
    let raw: RawToken = serde_json::from_slice(body).map_err(|_| unreadable())?;
    let expires =
        chrono::DateTime::parse_from_rfc3339(&raw.expires_at).map_err(|_| unreadable())?;
    if raw.token.is_empty() {
        return Err(unreadable());
    }
    Ok(BrokerToken {
        token: raw.token,
        expires_ms: expires.timestamp_millis(),
    })
}

/// Why the broker would not give one owner's token. The rest still list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerProblem {
    /// `404 not_installed`.
    NotInstalled,
    /// `503 app_unconfigured`.
    Unconfigured,
    /// `forbidden` or `github_refused`, with the broker's detail.
    Refused(String),
    /// No grant covers the request.
    NotGranted,
}

impl OwnerProblem {
    pub fn sentence(&self, owner: &str, app: Option<&str>, broker_host: &str) -> String {
        match self {
            OwnerProblem::NotInstalled => match app {
                Some(app) => format!("keeper's GitHub app {app} isn't installed on {owner}."),
                None => format!("keeper's GitHub app isn't installed on {owner}."),
            },
            OwnerProblem::Unconfigured => {
                format!("GitHub access for {owner} isn't set up on {broker_host} yet.")
            }
            OwnerProblem::Refused(detail) => format!("{broker_host} refused {owner}: {detail}"),
            OwnerProblem::NotGranted => {
                format!("{broker_host} gives you no access to {owner}'s repositories.")
            }
        }
    }
}

/// What went wrong asking the broker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerFailure {
    /// The whole request failed; nothing lists.
    Error(ForgeError),
    /// One owner failed, with the app keeper asked when it asked one; the
    /// others still list.
    Owner(OwnerProblem, Option<String>),
    /// `whoami` names no grants: this person has no GitHub access here.
    NoGrants,
}

#[derive(Deserialize, Default)]
struct RawError {
    #[serde(default)]
    error: String,
    #[serde(default)]
    detail: Option<String>,
}

/// A non-200 `{error, detail}` answer from `broker_host`. A 401 asks for a
/// sign-in, unless the token it refused was refreshed for this very request:
/// signing in again would give the same token, so that is a refusal.
pub fn classify_error(
    status: u16,
    body: &[u8],
    broker_host: &str,
    fresh_bearer: bool,
) -> BrokerFailure {
    let raw: RawError = serde_json::from_slice(body).unwrap_or_default();
    let detail = raw.detail.unwrap_or_else(|| raw.error.clone());
    let owner = |problem| BrokerFailure::Owner(problem, None);
    match (status, raw.error.as_str()) {
        (401, _) if fresh_bearer => BrokerFailure::Error(ForgeError::Refused(format!(
            "{broker_host} did not accept your account's sign-in: {detail}"
        ))),
        (401, _) => BrokerFailure::Error(ForgeError::NeedsSignIn(
            "Sign in again to reach GitHub through your organization.".to_owned(),
        )),
        (500..=599, "github_refused") => BrokerFailure::Error(ForgeError::Unreachable(format!(
            "GitHub isn't answering {broker_host} right now."
        ))),
        (_, "not_installed") => owner(OwnerProblem::NotInstalled),
        (_, "app_unconfigured") => owner(OwnerProblem::Unconfigured),
        (_, "forbidden" | "github_refused") => owner(OwnerProblem::Refused(detail)),
        (400, _) => BrokerFailure::Error(ForgeError::Internal(format!(
            "the GitHub broker refused keeper's request: {detail}"
        ))),
        (500..=599, _) => BrokerFailure::Error(ForgeError::Unreachable(format!(
            "{broker_host} can't answer right now."
        ))),
        _ => BrokerFailure::Error(ForgeError::Refused(format!(
            "{broker_host} refused keeper (HTTP {status})."
        ))),
    }
}

/// The app to ask for `owner`'s token (and `repository`'s, for a drive) with
/// `permissions`, as the broker's policy decides: only grants that cover the
/// request count, and among those the descriptor's `preferred` app wins,
/// else the first in `whoami` order.
pub fn app_for<'a>(
    grants: &'a [Grant],
    owner: &str,
    repository: Option<&str>,
    permissions: Permissions,
    preferred: Option<&str>,
) -> Option<&'a str> {
    let covering = || {
        grants
            .iter()
            .filter(move |grant| grant.covers(owner, repository, permissions))
    };
    preferred
        .and_then(|preferred| covering().find(|grant| grant.app == preferred))
        .or_else(|| covering().next())
        .map(|grant| grant.app.as_str())
}

/// The first of `asks` some grant allows, and the app to ask it of.
pub fn choose<'a>(
    grants: &'a [Grant],
    owner: &str,
    repository: Option<&str>,
    asks: &[Permissions],
    preferred: Option<&str>,
) -> Option<(&'a str, Permissions)> {
    asks.iter().find_map(|&permissions| {
        app_for(grants, owner, repository, permissions, preferred).map(|app| (app, permissions))
    })
}

/// Whether a drive of `owner/repository` may push: some grant allows
/// `contents: write` on it.
pub fn can_write(grants: &[Grant], owner: &str, repository: &str) -> bool {
    grants
        .iter()
        .any(|grant| grant.covers(owner, Some(repository), DRIVE_ASKS[0]))
}

/// The sentence for a person the broker gives no GitHub access.
pub fn no_access(broker_host: &str) -> String {
    format!("Your account has no GitHub access on {broker_host}. Ask its administrator to add you.")
}

fn configured(d: &AccountDescriptor) -> Result<&GithubBroker, BrokerFailure> {
    d.github_broker.as_ref().ok_or_else(|| {
        BrokerFailure::Error(ForgeError::Internal(
            "the account has no GitHub broker".to_owned(),
        ))
    })
}

/// Whose answers these are: the broker, the account and the signed-in
/// person.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Who {
    broker: String,
    account: String,
    sub: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TokenKey {
    who: Who,
    app: String,
    owner: String,
    repositories: String,
    permissions: String,
}

static WHOAMI: LazyLock<Mutex<HashMap<Who, Arc<Whoami>>>> = LazyLock::new(Default::default);
static TOKENS: LazyLock<Mutex<HashMap<TokenKey, BrokerToken>>> = LazyLock::new(Default::default);

/// The signed-in person, or the sign-in the broker needs: nothing cached
/// is served without a live session.
fn who(
    platform: &dyn Platform,
    d: &AccountDescriptor,
    broker: &GithubBroker,
) -> Result<Who, BrokerFailure> {
    match session::identity(platform, d) {
        Ok(Some(identity)) => Ok(Who {
            broker: broker.url.clone(),
            account: d.id.clone(),
            sub: identity.sub,
        }),
        Ok(None) => Err(BrokerFailure::Error(ForgeError::NeedsSignIn(
            "Sign in to your account to see its GitHub repositories.".to_owned(),
        ))),
        Err(error) => Err(BrokerFailure::Error(error.into())),
    }
}

/// The account's access token, and whether it was refreshed for this call.
async fn bearer(
    platform: &dyn Platform,
    http: &reqwest::Client,
    d: &AccountDescriptor,
) -> Result<(String, bool), BrokerFailure> {
    let before = session::load_bound_session(platform, d)
        .ok()
        .flatten()
        .map(|stored| stored.access_token);
    let token = oidc::access_token(platform, http, d)
        .await
        .map_err(|e| BrokerFailure::Error(e.into()))?;
    let fresh = before.as_deref() != Some(token.as_str());
    Ok((token, fresh))
}

/// What the person may reach, cached for the process until `refresh` or an
/// error.
pub async fn whoami(
    platform: &dyn Platform,
    http: &reqwest::Client,
    d: &AccountDescriptor,
    refresh: bool,
) -> Result<Arc<Whoami>, BrokerFailure> {
    let broker = configured(d)?;
    let key = who(platform, d, broker)?;
    if !refresh {
        if let Some(hit) = WHOAMI
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return Ok(Arc::clone(hit));
        }
    }
    WHOAMI
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&key);
    let (token, fresh) = bearer(platform, http, d).await?;
    let request = http.get(whoami_request(&broker.url)).bearer_auth(token);
    let response = send(http, request).await.map_err(BrokerFailure::Error)?;
    let status = response.status().as_u16();
    let bytes = body(response).await.map_err(BrokerFailure::Error)?;
    if status != 200 {
        return Err(classify_error(status, &bytes, &broker.host(), fresh));
    }
    let answer = Arc::new(parse_whoami(&bytes).map_err(BrokerFailure::Error)?);
    WHOAMI
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key, Arc::clone(&answer));
    Ok(answer)
}

/// A token for `owner` (and, for a drive, one repository): the first of
/// `asks` a grant allows, from the cache while it has 5 minutes left. The
/// app is [`choose`]n as the broker's policy would.
pub async fn mint(
    platform: &dyn Platform,
    http: &reqwest::Client,
    d: &AccountDescriptor,
    owner: &str,
    repositories: Option<&[String]>,
    asks: &[Permissions],
) -> Result<String, BrokerFailure> {
    let broker = configured(d)?;
    let answer = whoami(platform, http, d, false).await?;
    if answer.grants.is_empty() {
        return Err(BrokerFailure::NoGrants);
    }
    let repository = match repositories {
        Some([one]) => Some(one.as_str()),
        _ => None,
    };
    let (app, permissions) = choose(
        &answer.grants,
        owner,
        repository,
        asks,
        broker.app.as_deref(),
    )
    .ok_or(BrokerFailure::Owner(OwnerProblem::NotGranted, None))?;
    let key = TokenKey {
        who: who(platform, d, broker)?,
        app: app.to_owned(),
        owner: owner.to_ascii_lowercase(),
        repositories: repositories.map(|r| r.join(",")).unwrap_or_default(),
        permissions: permissions
            .iter()
            .map(|(name, level)| format!("{name}={level}"))
            .collect::<Vec<_>>()
            .join(","),
    };
    if let Some(hit) = TOKENS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
        .filter(|hit| hit.expires_ms - TOKEN_EARLY_MS > now_ms())
    {
        return Ok(hit.token.clone());
    }
    let (token, fresh) = bearer(platform, http, d).await?;
    let request = http
        .post(token_url(&broker.url))
        .bearer_auth(token)
        .json(&token_request(app, owner, repositories, permissions));
    let response = send(http, request).await.map_err(BrokerFailure::Error)?;
    let status = response.status().as_u16();
    let bytes = body(response).await.map_err(BrokerFailure::Error)?;
    if status != 200 {
        let failure = match classify_error(status, &bytes, &broker.host(), fresh) {
            BrokerFailure::Owner(problem, _) => BrokerFailure::Owner(problem, Some(app.to_owned())),
            other => other,
        };
        if matches!(failure, BrokerFailure::Error(_)) {
            // A sign-in or broker failure may mean the grants changed.
            WHOAMI
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&key.who);
        }
        return Err(failure);
    }
    let minted = parse_token(&bytes).map_err(BrokerFailure::Error)?;
    let mut tokens = TOKENS.lock().unwrap_or_else(PoisonError::into_inner);
    let now = now_ms();
    tokens.retain(|_, token| token.expires_ms > now);
    tokens.insert(key, minted.clone());
    Ok(minted.token)
}

/// Drop a token GitHub refused, so no drive or listing is handed it again.
pub fn forget_token(token: &str) {
    TOKENS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|_, cached| cached.token != token);
}

/// Drop every answer and token: the person signed out or in, or the
/// account changed.
pub fn forget() {
    WHOAMI
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
    TOKENS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forges::testing::{self, Reply};

    const WHOAMI_BODY: &str = r#"{"sub":"3001","subject":"tomasz","grants":[
        {"app":"tgbot","owners":["tgorka","Acme"],"repositories":"*",
         "permissions":{"contents":"write","metadata":"read"}},
        {"app":"tgdev","owners":["acme","zeta"],"repositories":["keeper","notes"],
         "permissions":{"contents":"read"}}]}"#;

    #[test]
    fn whoami_parses_star_and_listed_repositories_and_no_grants() {
        let who = parse_whoami(WHOAMI_BODY.as_bytes()).expect("whoami");
        assert_eq!(who.subject.as_deref(), Some("tomasz"));
        assert_eq!(who.grants[0].repositories, GrantRepositories::All);
        assert_eq!(
            who.grants[1].repositories,
            GrantRepositories::Only(vec!["keeper".to_owned(), "notes".to_owned()])
        );
        assert_eq!(
            who.grants[0]
                .permissions
                .get("contents")
                .map(String::as_str),
            Some("write")
        );
        assert_eq!(who.owners(), ["Acme", "tgorka", "zeta"]);
        let nobody = parse_whoami(br#"{"sub":"9","subject":null,"grants":[]}"#).expect("nobody");
        assert_eq!(nobody.subject, None);
        assert!(nobody.grants.is_empty());
        assert!(parse_whoami(br#"{"sub":"1","subject":null,"grants":[{"app":"a","owners":[],"repositories":"all"}]}"#).is_err());
    }

    #[test]
    fn the_app_is_one_whose_grant_covers_the_request_and_the_preferred_wins_only_among_those() {
        let who = parse_whoami(WHOAMI_BODY.as_bytes()).expect("whoami");
        let grants = &who.grants;
        // Listing: tgdev's ceiling has no `metadata`, so the preferred app
        // cannot list acme and tgbot is asked.
        assert_eq!(
            choose(grants, "ACME", None, LIST_ASKS, Some("tgdev")),
            Some(("tgbot", LIST_ASKS[0]))
        );
        // Only tgdev names zeta; it can list with `contents: read`.
        assert_eq!(
            choose(grants, "zeta", None, LIST_ASKS, Some("tgbot")),
            Some(("tgdev", LIST_ASKS[1]))
        );
        // A drive writes only through a grant allowing write…
        assert_eq!(
            choose(grants, "acme", Some("keeper"), DRIVE_ASKS, Some("tgdev")),
            Some(("tgbot", DRIVE_ASKS[0]))
        );
        // …the preferred app wins among grants that do cover…
        assert_eq!(
            app_for(grants, "acme", Some("keeper"), DRIVE_ASKS[1], Some("tgdev")),
            Some("tgdev")
        );
        // …else it reads, and a repository outside every list is not granted.
        assert_eq!(
            choose(grants, "zeta", Some("Notes"), DRIVE_ASKS, None),
            Some(("tgdev", DRIVE_ASKS[1]))
        );
        assert_eq!(choose(grants, "zeta", Some("site"), DRIVE_ASKS, None), None);
        assert_eq!(
            choose(grants, "nobody", None, LIST_ASKS, Some("tgbot")),
            None
        );
        assert!(can_write(grants, "tgorka", "anything"));
        assert!(!can_write(grants, "zeta", "keeper"));
    }

    #[test]
    fn token_requests_carry_owner_repositories_and_permissions() {
        assert_eq!(
            token_request("tgbot", "acme", None, LIST_ASKS[0]),
            serde_json::json!({ "app": "tgbot", "owner": "acme",
                                "permissions": { "metadata": "read" } })
        );
        assert_eq!(
            token_request("tgbot", "acme", Some(&["keeper".to_owned()]), DRIVE_ASKS[0]),
            serde_json::json!({ "app": "tgbot", "owner": "acme", "repositories": ["keeper"],
                                "permissions": { "contents": "write" } })
        );
        assert_eq!(
            whoami_request("https://b.acme.dev:8455/"),
            "https://b.acme.dev:8455/v1/whoami"
        );
    }

    #[test]
    fn a_token_answer_carries_its_expiry() {
        let token = parse_token(
            br#"{"token":"ghs_x","expires_at":"2026-09-24T12:00:00Z","app":"tgbot",
                 "owner":"acme","repositories":"*","permissions":{"metadata":"read"}}"#,
        )
        .expect("token");
        assert_eq!(token.token, "ghs_x");
        assert_eq!(token.expires_ms, 1_790_251_200_000);
        assert!(parse_token(br#"{"token":"ghs_x","expires_at":"soon"}"#).is_err());
        assert!(parse_token(br#"{"token":"","expires_at":"2026-09-24T12:00:00Z"}"#).is_err());
    }

    #[test]
    fn errors_split_into_sign_in_owner_notices_and_outages() {
        let classify =
            |status, body: &str| classify_error(status, body.as_bytes(), "b.acme.dev", false);
        assert!(matches!(
            classify(
                401,
                r#"{"error":"unauthenticated","detail":"token rejected"}"#
            ),
            BrokerFailure::Error(ForgeError::NeedsSignIn(_))
        ));
        // A token refreshed for this very request: signing in again cannot help.
        assert_eq!(
            classify_error(
                401,
                br#"{"error":"unauthenticated","detail":"audience mismatch"}"#,
                "b.acme.dev",
                true
            ),
            BrokerFailure::Error(ForgeError::Refused(
                "b.acme.dev did not accept your account's sign-in: audience mismatch".to_owned()
            ))
        );
        assert_eq!(
            classify(
                404,
                r#"{"error":"not_installed","detail":"app tgbot is not installed on acme"}"#
            ),
            BrokerFailure::Owner(OwnerProblem::NotInstalled, None)
        );
        assert_eq!(
            classify(503, r#"{"error":"app_unconfigured","detail":"x"}"#),
            BrokerFailure::Owner(OwnerProblem::Unconfigured, None)
        );
        assert_eq!(
            classify(403, r#"{"error":"forbidden","detail":"not allowed: acme"}"#),
            BrokerFailure::Owner(OwnerProblem::Refused("not allowed: acme".to_owned()), None)
        );
        assert_eq!(
            classify(
                403,
                r#"{"error":"github_refused","detail":"Not Found","github_status":404}"#
            ),
            BrokerFailure::Owner(OwnerProblem::Refused("Not Found".to_owned()), None)
        );
        // GitHub itself failing behind the broker is an outage, not a policy.
        assert_eq!(
            classify(
                502,
                r#"{"error":"github_refused","detail":"Bad Gateway","github_status":502}"#
            ),
            BrokerFailure::Error(ForgeError::Unreachable(
                "GitHub isn't answering b.acme.dev right now.".to_owned()
            ))
        );
        for (status, body) in [
            (503, r#"{"error":"idp_unavailable","detail":"x"}"#),
            (502, r#"{"error":"broker_error","detail":"KeyError"}"#),
            (500, "not json"),
        ] {
            assert_eq!(
                classify(status, body),
                BrokerFailure::Error(ForgeError::Unreachable(
                    "b.acme.dev can't answer right now.".to_owned()
                )),
                "{status} {body}"
            );
        }
        assert_eq!(
            OwnerProblem::NotInstalled.sentence("acme", Some("tgbot"), "b.acme.dev"),
            "keeper's GitHub app tgbot isn't installed on acme."
        );
        assert_eq!(
            OwnerProblem::Unconfigured.sentence("acme", None, "b.acme.dev"),
            "GitHub access for acme isn't set up on b.acme.dev yet."
        );
    }

    /// The broker's `policy.decide` for [`WHOAMI_BODY`], written out by hand
    /// so the fake refuses exactly what the real broker would.
    fn policy_allows(request: &serde_json::Value) -> bool {
        let owner = request["owner"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let repositories: Vec<&str> = request["repositories"]
            .as_array()
            .map(|r| r.iter().filter_map(|n| n.as_str()).collect())
            .unwrap_or_default();
        let permissions = request["permissions"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        let asks = |name: &str| permissions.get(name).and_then(|l| l.as_str());
        match request["app"].as_str() {
            Some("tgbot") => {
                ["tgorka", "acme"].contains(&owner.as_str())
                    && permissions
                        .keys()
                        .all(|k| k == "contents" || k == "metadata")
                    && asks("contents").is_none_or(|l| l == "read" || l == "write")
                    && asks("metadata").is_none_or(|l| l == "read")
            }
            // Without `repositories`, the grant's own list.
            Some("tgdev") => {
                ["acme", "zeta"].contains(&owner.as_str())
                    && repositories.iter().all(|r| ["keeper", "notes"].contains(r))
                    && permissions.keys().all(|k| k == "contents")
                    && asks("contents").is_none_or(|l| l == "read")
            }
            _ => false,
        }
    }

    fn fake_broker() -> testing::Fake {
        testing::serve(|seen| match (seen.method.as_str(), seen.path.as_str()) {
            ("GET", "/v1/whoami") => {
                assert_eq!(seen.header("authorization"), Some("Bearer account-token"));
                Reply::json(200, WHOAMI_BODY)
            }
            ("POST", "/v1/token") => {
                assert_eq!(seen.header("authorization"), Some("Bearer account-token"));
                let request = seen.json();
                if !policy_allows(&request) {
                    return Reply::json(
                        403,
                        r#"{"error":"forbidden","detail":"permissions exceed grant"}"#,
                    );
                }
                Reply::json(
                    200,
                    &format!(
                        r#"{{"token":"ghs_{}_{}","expires_at":"2099-01-01T00:00:00Z"}}"#,
                        request["app"].as_str().unwrap_or_default(),
                        request["permissions"]["contents"]
                            .as_str()
                            .unwrap_or("meta")
                    ),
                )
            }
            _ => Reply::json(404, r#"{"error":"not_found"}"#),
        })
    }

    #[tokio::test]
    async fn minting_asks_what_the_policy_allows_and_reuses_an_unexpired_answer() {
        let fake = fake_broker();
        let p = testing::FakePlatform::default();
        let d = testing::signed_in_account(
            &p,
            &format!(
                r#", "github_broker": {{ "url": "{}", "app": "tgdev" }}"#,
                fake.base
            ),
        );
        let http = testing::http();
        for _ in 0..2 {
            let token = mint(
                &p,
                &http,
                &d,
                "acme",
                Some(&["keeper".to_owned()]),
                DRIVE_ASKS,
            )
            .await
            .expect("minted");
            assert_eq!(token, "ghs_tgbot_write");
        }
        let posts = || {
            fake.requests()
                .iter()
                .filter(|seen| seen.method == "POST")
                .count()
        };
        assert_eq!(posts(), 1, "the second call is served from memory");
        assert_eq!(
            mint(
                &p,
                &http,
                &d,
                "zeta",
                Some(&["notes".to_owned()]),
                DRIVE_ASKS
            )
            .await,
            Ok("ghs_tgdev_read".to_owned())
        );
        assert_eq!(
            mint(&p, &http, &d, "zeta", None, LIST_ASKS).await,
            Ok("ghs_tgdev_read".to_owned()),
            "tgdev has no `metadata`, so zeta lists with `contents: read`"
        );
        assert_eq!(
            mint(&p, &http, &d, "nobody", None, LIST_ASKS).await,
            Err(BrokerFailure::Owner(OwnerProblem::NotGranted, None))
        );
        assert_eq!(posts(), 3, "nothing the policy refuses is asked for");
    }

    #[tokio::test]
    async fn cached_answers_belong_to_the_signed_in_person_and_need_a_session() {
        use crate::org_account::session;
        let fake = fake_broker();
        let p = testing::FakePlatform::default();
        let d = testing::signed_in_account(
            &p,
            &format!(r#", "github_broker": {{ "url": "{}" }}"#, fake.base),
        );
        let http = testing::http();
        let repo = ["keeper".to_owned()];
        let drive = || mint(&p, &http, &d, "acme", Some(&repo), DRIVE_ASKS);
        drive().await.expect("first person");
        let count = |method: &str| {
            fake.requests()
                .iter()
                .filter(|seen| seen.method == method)
                .count()
        };
        assert_eq!((count("GET"), count("POST")), (1, 1));

        // Another person signs in to the same account: nothing of the first
        // is served to them.
        let mut other = session::load_session(&p, &d.id)
            .expect("read")
            .expect("session");
        other.sub = "sub-2".to_owned();
        session::store_session(&p, &d.id, &other).expect("store");
        drive().await.expect("second person");
        assert_eq!((count("GET"), count("POST")), (2, 2));

        // Signed out: no cached token, whoever held it.
        p.keychain_delete(&session::session_key(&d.id))
            .expect("sign out");
        assert!(matches!(
            drive().await,
            Err(BrokerFailure::Error(ForgeError::NeedsSignIn(_)))
        ));
        assert_eq!((count("GET"), count("POST")), (2, 2));
    }
}
