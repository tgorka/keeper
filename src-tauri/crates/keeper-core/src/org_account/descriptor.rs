//! The account descriptor: who the person signs in with and where their
//! settings live (AD-308).
//!
//! One schema, two spellings. An operator serves it as JSON (a setup link, a
//! QR code, a pasted `https://` URL); keeper stores it as TOML in
//! `~/.keeper/account.toml` (the app data dir on iOS). It is deliberately
//! outside the layer stack: the account tiers are found *through* it, and a
//! file inside the config repository must never be able to redirect the
//! account to another issuer or repository.
//!
//! Every refusal happens here, at parse time, so nothing downstream has to
//! re-check a descriptor it was handed.

use std::net::IpAddr;
use std::path::Path;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use super::AccountError;
use crate::config::{LayerFault, LayerFaultKind, LayerTier};
use crate::error::CoreError;

/// The file name under `~/.keeper/` (or the iOS data dir).
pub const FILE_NAME: &str = "account.toml";

/// The only schema version this build understands.
const VERSION: u32 = 1;

/// The largest descriptor keeper reads, from the network or from a link. A
/// typical one is ~600 bytes; the cap exists so a hostile URL cannot make
/// keeper buffer an arbitrary body.
const MAX_DESCRIPTOR_BYTES: usize = 64 * 1024;

const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountDescriptor {
    pub version: u32,
    /// `[a-z0-9-]{1,32}` — part of the keychain keys and the redirect URI.
    pub id: String,
    /// Shown in Settings and on the sign-in sheet.
    pub name: String,
    pub auth: AuthConfig,
    pub config: RepoConfig,
    /// Extra repository sources the browse sheet lists (AD-333). Not part of
    /// the sign-in: editing them never replaces the account.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forges: Vec<ForgeEntry>,
    /// The organisation's GitHub broker (makistack `github-broker`): GitHub
    /// installation tokens for the account's access token (AD-334).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_broker: Option<GithubBroker>,
}

/// One `[[forges]]` entry: a forge whose repositories keeper can list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForgeEntry {
    pub kind: ForgeEntryKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base: Option<String>,
    /// The public OAuth client for the device flow (GitHub only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForgeEntryKind {
    Github,
    Forgejo,
}

pub const GITHUB_WEB_BASE: &str = "https://github.com";
pub const GITHUB_API_BASE: &str = "https://api.github.com";
/// The id of the forge the account's own `oauth` repository lives on.
pub const ACCOUNT_FORGE_ID: &str = "account-forge";

impl ForgeEntry {
    /// The source id: as written, else the kind's name.
    pub fn source_id(&self) -> &str {
        self.id.as_deref().unwrap_or(match self.kind {
            ForgeEntryKind::Github => "github",
            ForgeEntryKind::Forgejo => "forgejo",
        })
    }

    pub fn web_base(&self) -> Option<&str> {
        self.web_base.as_deref().or(match self.kind {
            ForgeEntryKind::Github => Some(GITHUB_WEB_BASE),
            ForgeEntryKind::Forgejo => None,
        })
    }

    pub fn api_base(&self) -> Option<&str> {
        self.api_base.as_deref().or(match self.kind {
            ForgeEntryKind::Github => Some(GITHUB_API_BASE),
            ForgeEntryKind::Forgejo => None,
        })
    }
}

/// `[github_broker]`: where GitHub access comes from (AD-333).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubBroker {
    pub url: String,
    /// The GitHub App preferred when several grants cover one owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
}

impl GithubBroker {
    pub fn host(&self) -> String {
        host_of(&self.url)
    }
}

/// The one identity (OIDC).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    /// Compared as an exact string with the `iss` keeper receives, so it is
    /// never normalised here.
    pub issuer: String,
    pub client_id: String,
    #[serde(default = "default_scopes")]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_scopes: Vec<String>,
    /// Extra `aud` values the id_token may carry (Zitadel adds its project id).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trusted_audiences: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect_uri: Option<String>,
    #[serde(default = "default_username_claim")]
    pub username_claim: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roles_claim: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_role: Option<String>,
    #[serde(default, skip_serializing_if = "Endpoints::is_empty")]
    pub endpoints: Endpoints,
}

/// Overrides of what discovery answers, each optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub userinfo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwks: Option<String>,
}

impl Endpoints {
    fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    fn named(&self) -> [(&'static str, &Option<String>); 6] {
        [
            ("auth.endpoints.authorization", &self.authorization),
            ("auth.endpoints.token", &self.token),
            ("auth.endpoints.userinfo", &self.userinfo),
            ("auth.endpoints.revocation", &self.revocation),
            ("auth.endpoints.end_session", &self.end_session),
            ("auth.endpoints.jwks", &self.jwks),
        ]
    }
}

/// The config repository (git over HTTPS).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoConfig {
    pub url: String,
    #[serde(default = "default_branch")]
    pub branch: String,
    /// The forge's API root. Always explicit: nothing derives it from `url`
    /// (AD-316), because a guessed API base is a token sent to a guessed host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base: Option<String>,
    /// Which `user.toml` field holds the sign-in `sub` (AD-313).
    #[serde(default = "default_identity_field")]
    pub identity_field: String,
    #[serde(default)]
    pub auth: RepoAuthConfig,
}

/// How keeper authenticates to the config repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawRepoAuth", into = "RawRepoAuth")]
pub enum RepoAuthConfig {
    /// The sign-in access token is the git HTTP credential.
    Same { scheme: GitScheme, username: String },
    /// A forge that only accepts its own tokens (Gitea/Forgejo-style).
    Oauth(ForgeOauth),
    /// A repository that needs no credential.
    None,
}

impl Default for RepoAuthConfig {
    fn default() -> Self {
        RepoAuthConfig::Same {
            scheme: GitScheme::Basic,
            username: DEFAULT_GIT_USERNAME.to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitScheme {
    /// The token is the password, `username` the user.
    #[default]
    Basic,
    /// `Authorization: Bearer <token>`.
    Bearer,
}

/// The forge's own OAuth client, for `mode = "oauth"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeOauth {
    pub issuer: Option<String>,
    pub authorize_url: Option<String>,
    pub token_url: Option<String>,
    pub client_id: String,
    /// One string, sent byte-identical on every device and version.
    pub scope: String,
    pub redirect_uri: Option<String>,
    /// `{authorize_path_and_query}` is filled in when present.
    pub signin_url: Option<String>,
    /// `{api_base}` is filled in; `None` means `{api_base}/user`.
    pub user_url: Option<String>,
    pub username_field: String,
}

const DEFAULT_GIT_USERNAME: &str = "oauth2";
const DEFAULT_USERNAME_FIELD: &str = "login";
const AUTHORIZE_PLACEHOLDER: &str = "{authorize_path_and_query}";
const API_BASE_PLACEHOLDER: &str = "{api_base}";

fn default_scopes() -> Vec<String> {
    ["openid", "profile", "email", "offline_access"]
        .map(str::to_owned)
        .to_vec()
}

fn default_username_claim() -> String {
    "preferred_username".to_owned()
}

fn default_branch() -> String {
    "main".to_owned()
}

fn default_identity_field() -> String {
    "sub".to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RepoAuthMode {
    Same,
    Oauth,
    None,
}

/// The flat `[config.auth]` table as written. One table carries every mode's
/// fields, so the mode decides which of them may appear — a field that belongs
/// to another mode is a mistake worth naming rather than ignoring.
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRepoAuth {
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<RepoAuthMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scheme: Option<GitScheme>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    issuer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    authorize_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    redirect_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    signin_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username_field: Option<String>,
}

impl TryFrom<RawRepoAuth> for RepoAuthConfig {
    type Error = String;

    fn try_from(raw: RawRepoAuth) -> Result<Self, String> {
        let mode = raw.mode.unwrap_or(RepoAuthMode::Same);
        let oauth_fields = [
            ("issuer", raw.issuer.is_some()),
            ("authorize_url", raw.authorize_url.is_some()),
            ("token_url", raw.token_url.is_some()),
            ("client_id", raw.client_id.is_some()),
            ("scope", raw.scope.is_some()),
            ("redirect_uri", raw.redirect_uri.is_some()),
            ("signin_url", raw.signin_url.is_some()),
            ("user_url", raw.user_url.is_some()),
            ("username_field", raw.username_field.is_some()),
        ];
        let same_fields = [
            ("scheme", raw.scheme.is_some()),
            ("username", raw.username.is_some()),
        ];
        let misplaced = |fields: &[(&str, bool)], wanted: &str| {
            fields
                .iter()
                .find(|(_, present)| *present)
                .map(|(field, _)| {
                    format!("`config.auth.{field}` only applies to mode = \"{wanted}\"")
                })
        };
        match mode {
            RepoAuthMode::Same => {
                if let Some(message) = misplaced(&oauth_fields, "oauth") {
                    return Err(message);
                }
                let scheme = raw.scheme.unwrap_or_default();
                if scheme == GitScheme::Bearer && raw.username.is_some() {
                    return Err(
                        "`config.auth.username` only applies to scheme = \"basic\"".to_owned()
                    );
                }
                Ok(RepoAuthConfig::Same {
                    scheme,
                    username: raw
                        .username
                        .unwrap_or_else(|| DEFAULT_GIT_USERNAME.to_owned()),
                })
            }
            RepoAuthMode::Oauth => {
                if let Some(message) = misplaced(&same_fields, "same") {
                    return Err(message);
                }
                let client_id = raw
                    .client_id
                    .ok_or("mode = \"oauth\" needs `config.auth.client_id`")?;
                let scope = raw
                    .scope
                    .ok_or("mode = \"oauth\" needs `config.auth.scope`")?;
                Ok(RepoAuthConfig::Oauth(ForgeOauth {
                    issuer: raw.issuer,
                    authorize_url: raw.authorize_url,
                    token_url: raw.token_url,
                    client_id,
                    scope,
                    redirect_uri: raw.redirect_uri,
                    signin_url: raw.signin_url,
                    user_url: raw.user_url,
                    username_field: raw
                        .username_field
                        .unwrap_or_else(|| DEFAULT_USERNAME_FIELD.to_owned()),
                }))
            }
            RepoAuthMode::None => {
                let mut every = same_fields.iter().chain(oauth_fields.iter());
                match every.find(|(_, present)| *present) {
                    Some((field, _)) => Err(format!(
                        "`config.auth.{field}` does not apply to mode = \"none\""
                    )),
                    None => Ok(RepoAuthConfig::None),
                }
            }
        }
    }
}

impl From<RepoAuthConfig> for RawRepoAuth {
    fn from(auth: RepoAuthConfig) -> Self {
        match auth {
            RepoAuthConfig::Same { scheme, username } => RawRepoAuth {
                mode: Some(RepoAuthMode::Same),
                scheme: Some(scheme),
                // A bearer credential has no user; writing one would make the
                // stored file fail its own parse.
                username: (scheme == GitScheme::Basic).then_some(username),
                ..RawRepoAuth::default()
            },
            RepoAuthConfig::Oauth(forge) => RawRepoAuth {
                mode: Some(RepoAuthMode::Oauth),
                issuer: forge.issuer,
                authorize_url: forge.authorize_url,
                token_url: forge.token_url,
                client_id: Some(forge.client_id),
                scope: Some(forge.scope),
                redirect_uri: forge.redirect_uri,
                signin_url: forge.signin_url,
                user_url: forge.user_url,
                username_field: Some(forge.username_field),
                ..RawRepoAuth::default()
            },
            RepoAuthConfig::None => RawRepoAuth {
                mode: Some(RepoAuthMode::None),
                ..RawRepoAuth::default()
            },
        }
    }
}

impl AccountDescriptor {
    /// Where the identity provider sends the browser back.
    pub fn redirect_uri(&self) -> String {
        self.auth
            .redirect_uri
            .clone()
            .unwrap_or_else(|| format!("keeper://oauth/{}/callback", self.id))
    }

    /// Where the forge sends the browser back, in `oauth` mode only.
    pub fn forge_redirect_uri(&self) -> Option<String> {
        match &self.config.auth {
            RepoAuthConfig::Oauth(forge) => Some(
                forge
                    .redirect_uri
                    .clone()
                    .unwrap_or_else(|| format!("keeper://oauth/{}/forge/callback", self.id)),
            ),
            RepoAuthConfig::Same { .. } | RepoAuthConfig::None => None,
        }
    }

    /// The sign-in host, as the confirmation sheet shows it.
    pub fn issuer_host(&self) -> String {
        host_of(&self.auth.issuer)
    }

    /// The config repository's host, as the confirmation sheet shows it.
    pub fn repo_host(&self) -> String {
        host_of(&self.config.url)
    }

    /// The forge host in `oauth` mode — the issuer, else the authorize URL.
    pub fn forge_host(&self) -> Option<String> {
        match &self.config.auth {
            RepoAuthConfig::Oauth(forge) => forge
                .issuer
                .as_deref()
                .or(forge.authorize_url.as_deref())
                .map(host_of),
            RepoAuthConfig::Same { .. } | RepoAuthConfig::None => None,
        }
    }

    /// The origins this account's own tokens belong to: the issuer, the
    /// config repository and, in `oauth` mode, the forge. Sorted, once each.
    pub fn trusted_origins(&self) -> Vec<String> {
        let mut urls = vec![self.auth.issuer.as_str(), self.config.url.as_str()];
        if let RepoAuthConfig::Oauth(forge) = &self.config.auth {
            urls.extend(forge.issuer.as_deref());
            urls.extend(forge.authorize_url.as_deref());
        }
        let mut origins: Vec<String> = urls
            .into_iter()
            .filter_map(super::settings_sync::url_origin)
            .collect();
        origins.sort();
        origins.dedup();
        origins
    }

    /// Whether a drive at `remote_url` may sign in with this account: the
    /// remote sits at one of [`trusted_origins`](Self::trusted_origins).
    /// Anywhere else — github.com among them — the account's token would be
    /// handed to a stranger, and the stranger would refuse it anyway.
    pub fn serves_remote(&self, remote_url: &str) -> bool {
        super::settings_sync::url_origin(remote_url)
            .is_some_and(|origin| self.trusted_origins().contains(&origin))
    }

    /// `same`, `oauth` or `none`.
    pub fn repo_mode(&self) -> &'static str {
        match self.config.auth {
            RepoAuthConfig::Same { .. } => "same",
            RepoAuthConfig::Oauth(_) => "oauth",
            RepoAuthConfig::None => "none",
        }
    }

    /// Whether confirming `self` ends `previous`: another id, or the same id
    /// with any sign-in or repository setting changed. Only the display name
    /// may change without a sign-out.
    pub fn replaces(&self, previous: &AccountDescriptor) -> bool {
        previous.id != self.id || previous.auth != self.auth || previous.config != self.config
    }
}

fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| url.to_owned())
}

/// A descriptor keeper will not use, and why — a sentence for the person who
/// wrote it, with the line when the parser knows one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct DescriptorError {
    pub message: String,
    pub line: Option<usize>,
}

impl DescriptorError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            line: None,
        }
    }
}

impl From<DescriptorError> for AccountError {
    fn from(error: DescriptorError) -> Self {
        AccountError::Refused(error.message)
    }
}

/// Parse the operator's JSON form, then [`validate`].
pub fn parse_json(text: &str) -> Result<AccountDescriptor, DescriptorError> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|error| DescriptorError {
        message: format!("The account descriptor is not valid JSON: {error}"),
        line: Some(error.line()),
    })?;
    refuse_secrets_json(&value)?;
    let descriptor: AccountDescriptor =
        serde_json::from_value(value).map_err(|error| DescriptorError {
            message: format!("The account descriptor does not fit the schema: {error}"),
            line: None,
        })?;
    validate(&descriptor)?;
    Ok(descriptor)
}

/// Parse the stored TOML form, then [`validate`].
pub fn parse_toml(text: &str) -> Result<AccountDescriptor, DescriptorError> {
    let line = |error: &toml::de::Error| error.span().map(|span| line_of(text, span.start));
    let table: toml::Table = toml::from_str(text).map_err(|error| DescriptorError {
        line: line(&error),
        message: format!("{FILE_NAME} is not valid TOML: {}", first_line(&error)),
    })?;
    refuse_secrets_toml(&table)?;
    let descriptor: AccountDescriptor = toml::from_str(text).map_err(|error| DescriptorError {
        line: line(&error),
        message: format!(
            "{FILE_NAME} does not fit the schema: {}",
            first_line(&error)
        ),
    })?;
    validate(&descriptor)?;
    Ok(descriptor)
}

/// `toml`'s message leads with a location header and a caret excerpt; the
/// fault already carries the line, so keep the sentence.
fn first_line(error: &toml::de::Error) -> String {
    error
        .message()
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

/// The TOML keeper stores. Parses back to an equal descriptor.
pub fn to_toml(d: &AccountDescriptor) -> String {
    toml::to_string(d).unwrap_or_else(|error| {
        // Every field is a string, a list of strings or a table of them; a
        // failure here is a schema change that forgot TOML's rules.
        tracing::error!(%error, "account descriptor could not be written as TOML");
        String::new()
    })
}

fn is_secret_key(key: &str) -> bool {
    key == "secret" || key.ends_with("_secret") || key.ends_with("Secret")
}

const SECRET_REFUSAL: &str =
    "keeper is a public client; remove the secret from the account descriptor.";

fn refuse_secrets_json(value: &serde_json::Value) -> Result<(), DescriptorError> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if is_secret_key(key) {
                    return Err(DescriptorError::new(SECRET_REFUSAL));
                }
                refuse_secrets_json(value)?;
            }
            Ok(())
        }
        serde_json::Value::Array(items) => items.iter().try_for_each(refuse_secrets_json),
        _ => Ok(()),
    }
}

fn refuse_secrets_toml(table: &toml::Table) -> Result<(), DescriptorError> {
    fn walk(value: &toml::Value) -> Result<(), DescriptorError> {
        match value {
            toml::Value::Table(table) => refuse_secrets_toml(table),
            toml::Value::Array(items) => items.iter().try_for_each(walk),
            _ => Ok(()),
        }
    }
    for (key, value) in table {
        if is_secret_key(key) {
            return Err(DescriptorError::new(SECRET_REFUSAL));
        }
        walk(value)?;
    }
    Ok(())
}

/// Every refusal of spec §2.2, in one place.
pub fn validate(d: &AccountDescriptor) -> Result<(), DescriptorError> {
    if d.version != VERSION {
        return Err(DescriptorError::new(format!(
            "This account descriptor is version {}; this keeper understands version {VERSION}.",
            d.version
        )));
    }
    if !valid_id(&d.id) {
        return Err(DescriptorError::new(format!(
            "The account id \"{}\" must be 1 to 32 characters of a-z, 0-9 and -.",
            d.id
        )));
    }
    require_text("name", &d.name)?;

    let auth = &d.auth;
    let issuer = plain_url("auth.issuer", &auth.issuer)?;
    let issuer_host = host_str(&issuer);
    require_text("auth.client_id", &auth.client_id)?;
    if !auth.scopes.iter().any(|scope| scope == "openid") {
        return Err(DescriptorError::new(
            "`auth.scopes` must include \"openid\"; keeper signs in with OpenID Connect.",
        ));
    }
    require_text("auth.username_claim", &auth.username_claim)?;
    if let Some(claim) = &auth.roles_claim {
        require_text("auth.roles_claim", claim)?;
    }
    if let Some(role) = &auth.required_role {
        require_text("auth.required_role", role)?;
    }
    if let Some(redirect) = &auth.redirect_uri {
        redirect_url("auth.redirect_uri", redirect)?;
    }
    // Every override receives a code, a verifier or a token, and the
    // confirmation sheet shows only the issuer's host: an override elsewhere
    // would send them to a host the person never saw.
    for (field, endpoint) in auth.endpoints.named() {
        if let Some(endpoint) = endpoint {
            let url = plain_url(field, endpoint)?;
            same_host(field, &url, "the sign-in server", issuer_host)?;
        }
    }

    let config = &d.config;
    let repo = plain_url("config.url", &config.url)?;
    let repo_host = host_str(&repo);
    let forge_issuer = match &config.auth {
        RepoAuthConfig::Oauth(ForgeOauth {
            issuer: Some(issuer),
            ..
        }) => Some(plain_url("config.auth.issuer", issuer)?),
        _ => None,
    };
    let forge_issuer_host = forge_issuer.as_ref().map(host_str);
    if let Some(api_base) = &config.api_base {
        let url = plain_url("config.api_base", api_base)?;
        api_host("config.api_base", &url, repo_host, forge_issuer_host)?;
    }
    if !valid_branch(&config.branch) {
        return Err(DescriptorError::new(format!(
            "`config.branch` \"{}\" is not a branch name.",
            config.branch
        )));
    }
    require_text("config.identity_field", &config.identity_field)?;
    if RESERVED_USER_FIELDS.contains(&config.identity_field.as_str()) {
        return Err(DescriptorError::new(format!(
            "`config.identity_field` cannot be \"{}\": keeper writes that field of user.toml itself.",
            config.identity_field
        )));
    }

    match &config.auth {
        RepoAuthConfig::Same { scheme, username } => {
            if *scheme == GitScheme::Basic {
                require_text("config.auth.username", username)?;
            }
        }
        RepoAuthConfig::Oauth(forge) => {
            validate_forge(
                forge,
                config.api_base.as_deref(),
                repo_host,
                forge_issuer_host,
            )?;
        }
        RepoAuthConfig::None => {}
    }
    validate_forges(d)
}

/// `[[forges]]` and `[github_broker]` (AD-333): every address that will
/// receive a token is `https` (loopback for tests) and belongs to the forge
/// the entry names, and every id names one source.
fn validate_forges(d: &AccountDescriptor) -> Result<(), DescriptorError> {
    let mut ids: Vec<&str> = Vec::new();
    if matches!(d.config.auth, RepoAuthConfig::Oauth(_)) {
        ids.push(ACCOUNT_FORGE_ID);
    }
    for (n, forge) in d.forges.iter().enumerate() {
        let field = |name: &str| format!("forges[{n}].{name}");
        let id = forge.source_id();
        if forge.kind == ForgeEntryKind::Forgejo {
            return Err(DescriptorError::new(
                "keeper lists only the account's own Forgejo; remove this [[forges]] entry.",
            ));
        }
        if !valid_id(id) {
            return Err(DescriptorError::new(format!(
                "The forge id \"{id}\" must be 1 to 32 characters of a-z, 0-9 and -."
            )));
        }
        if id == ACCOUNT_FORGE_ID || ids.contains(&id) {
            return Err(DescriptorError::new(format!(
                "Two forges share the id \"{id}\"; give each `[[forges]]` entry its own `id`."
            )));
        }
        ids.push(id);
        if let Some(name) = &forge.name {
            require_text(&field("name"), name)?;
        }
        let web = plain_url(
            &field("web_base"),
            forge.web_base().unwrap_or(GITHUB_WEB_BASE),
        )?;
        let api = plain_url(
            &field("api_base"),
            forge.api_base().unwrap_or(GITHUB_API_BASE),
        )?;
        // The API receives the person's GitHub token: github.com's is
        // api.github.com and nothing else, and any other forge's lives on
        // its own host.
        if is_github_com(&web) {
            if !is_github_api(&api) {
                return Err(DescriptorError::new(format!(
                    "`{}` must be {GITHUB_API_BASE} for github.com; keeper sends a GitHub token only to GitHub.",
                    field("api_base")
                )));
            }
        } else {
            same_host(
                &field("api_base"),
                &api,
                "the forge's `web_base`",
                host_str(&web),
            )?;
        }
        if let Some(client_id) = &forge.client_id {
            require_text(&field("client_id"), client_id)?;
        }
    }
    if let Some(broker) = &d.github_broker {
        plain_url("github_broker.url", &broker.url)?;
        if let Some(app) = &broker.app {
            require_text("github_broker.app", app)?;
        }
        // The broker's installation tokens are github.com's; a `github`
        // entry pointing elsewhere would send them to another host.
        let moved = d.forges.iter().any(|forge| {
            forge.source_id() == "github"
                && !forge
                    .web_base()
                    .and_then(|url| url::Url::parse(url).ok())
                    .is_some_and(|url| is_github_com(&url))
        });
        if moved {
            return Err(DescriptorError::new(
                "`[github_broker]` serves github.com, so the forge with id \"github\" must be github.com too.",
            ));
        }
    }
    Ok(())
}

/// `url` is github.com itself (any spelling: case, a trailing slash, the
/// default port), not a path under it or another host.
fn is_github_com(url: &url::Url) -> bool {
    same_root(url, GITHUB_WEB_BASE)
}

fn is_github_api(url: &url::Url) -> bool {
    same_root(url, GITHUB_API_BASE)
}

fn same_root(url: &url::Url, root: &str) -> bool {
    url::Url::parse(root).is_ok_and(|root| {
        url.origin() == root.origin() && url.path().trim_end_matches('/').is_empty()
    })
}

/// The `user.toml` fields keeper writes itself; the identity must live in
/// another one, or keeper would overwrite its own record with the `sub`.
const RESERVED_USER_FIELDS: [&str; 4] = ["login", "display_name", "issuer", "created"];

fn host_str(url: &url::Url) -> &str {
    url.host_str().unwrap_or_default()
}

fn same_host(
    field: &str,
    url: &url::Url,
    owner: &str,
    expected: &str,
) -> Result<(), DescriptorError> {
    let host = host_str(url);
    if host != expected {
        return Err(DescriptorError::new(format!(
            "`{field}` points at {host}, but {owner} is {expected}; keeper sends codes and tokens only to the host the confirmation shows."
        )));
    }
    Ok(())
}

/// The forge's API (`api_base`, `user_url`) receives the forge token, so it
/// lives on the repository's host or on the forge's sign-in host.
fn api_host(
    field: &str,
    url: &url::Url,
    repo_host: &str,
    forge_issuer_host: Option<&str>,
) -> Result<(), DescriptorError> {
    let host = host_str(url);
    if host != repo_host && Some(host) != forge_issuer_host {
        return Err(DescriptorError::new(format!(
            "`{field}` points at {host}, but the repository is on {repo_host}; keeper sends the repository's token only to the repository's host or its forge's sign-in host."
        )));
    }
    Ok(())
}

fn validate_forge(
    forge: &ForgeOauth,
    api_base: Option<&str>,
    repo_host: &str,
    forge_issuer_host: Option<&str>,
) -> Result<(), DescriptorError> {
    // The forge's sign-in endpoints belong to the forge's sign-in server, or,
    // without one, to the repository's host.
    let (forge_host, forge_label) = match forge_issuer_host {
        Some(host) => (host, "the forge's sign-in server"),
        None => (repo_host, "the repository"),
    };
    require_text("config.auth.client_id", &forge.client_id)?;
    require_text("config.auth.scope", &forge.scope)?;
    if forge.issuer.is_none() && (forge.authorize_url.is_none() || forge.token_url.is_none()) {
        return Err(DescriptorError::new(
            "mode = \"oauth\" needs the forge's `issuer`, or both `authorize_url` and `token_url`.",
        ));
    }
    if let Some(url) = &forge.authorize_url {
        let url = plain_url("config.auth.authorize_url", url)?;
        same_host("config.auth.authorize_url", &url, forge_label, forge_host)?;
    }
    if let Some(url) = &forge.token_url {
        let url = plain_url("config.auth.token_url", url)?;
        same_host("config.auth.token_url", &url, forge_label, forge_host)?;
    }
    if let Some(redirect) = &forge.redirect_uri {
        redirect_url("config.auth.redirect_uri", redirect)?;
    }
    if let Some(signin) = &forge.signin_url {
        secure_url(
            "config.auth.signin_url",
            &signin.replace(AUTHORIZE_PLACEHOLDER, "authorize"),
        )?;
    }
    require_text("config.auth.username_field", &forge.username_field)?;

    // The forge username is checked against the sign-in username (§3.5); a
    // descriptor that gives keeper no way to learn it cannot be honoured.
    let id_token_username = forge.issuer.is_some()
        && forge
            .scope
            .split_whitespace()
            .any(|scope| scope == "openid");
    let user_url = match &forge.user_url {
        Some(template) if template.contains(API_BASE_PLACEHOLDER) => match api_base {
            Some(base) => Some(template.replace(API_BASE_PLACEHOLDER, base.trim_end_matches('/'))),
            None => {
                return Err(DescriptorError::new(
                    "`config.auth.user_url` uses {api_base}, but `config.api_base` is not set.",
                ));
            }
        },
        other => other.clone(),
    };
    if let Some(url) = &user_url {
        let url = plain_url("config.auth.user_url", url)?;
        api_host("config.auth.user_url", &url, repo_host, forge_issuer_host)?;
    }
    if !id_token_username && api_base.is_none() && user_url.is_none() {
        return Err(DescriptorError::new(
            "mode = \"oauth\" needs a way to learn the forge username: a forge `issuer` with \"openid\" in `scope`, or `config.api_base`.",
        ));
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.starts_with(['-', '/'])
        && !branch.ends_with(['/', '.'])
        && !branch.contains("..")
        && !branch.ends_with(".lock")
        && branch
            .chars()
            .all(|c| !c.is_whitespace() && !c.is_control() && !"~^:?*[\\".contains(c))
}

fn require_text(field: &str, value: &str) -> Result<(), DescriptorError> {
    if value.trim().is_empty() {
        return Err(DescriptorError::new(format!(
            "`{field}` must not be empty."
        )));
    }
    Ok(())
}

fn is_loopback(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        None => false,
    }
}

/// `https`, or plain `http` to a loopback host (a test provider). Never a URL
/// with a user or password in it: a credential does not belong in a file.
fn secure_url(field: &str, value: &str) -> Result<url::Url, DescriptorError> {
    let url = url::Url::parse(value)
        .map_err(|_| DescriptorError::new(format!("`{field}` is not a URL: \"{value}\".")))?;
    let secure = match url.scheme() {
        "https" => url.host().is_some(),
        "http" => is_loopback(&url),
        _ => false,
    };
    if !secure {
        return Err(DescriptorError::new(format!(
            "`{field}` must be an https URL (plain http only for a loopback test server): \"{value}\"."
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(DescriptorError::new(format!(
            "`{field}` must not carry a user name or password."
        )));
    }
    Ok(url)
}

/// [`secure_url`] without a query or a fragment: a query is where a pasted
/// `?private_token=…` would ride into `account.toml` and every setup link.
fn plain_url(field: &str, value: &str) -> Result<url::Url, DescriptorError> {
    let url = secure_url(field, value)?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err(DescriptorError::new(format!(
            "`{field}` must not carry a query or a fragment."
        )));
    }
    Ok(url)
}

/// A redirect keeper can receive: its own `keeper://oauth/…` deep link, or a
/// loopback listener on desktop.
fn redirect_url(field: &str, value: &str) -> Result<(), DescriptorError> {
    let url = url::Url::parse(value)
        .map_err(|_| DescriptorError::new(format!("`{field}` is not a URL: \"{value}\".")))?;
    // Only the literal address the listener binds: `localhost` may resolve to
    // `::1`, where another process can be listening (RFC 8252 §8.3).
    let receivable = match url.scheme() {
        "keeper" => url.host_str() == Some("oauth"),
        "http" => url.host() == Some(url::Host::Ipv4(std::net::Ipv4Addr::LOCALHOST)),
        _ => false,
    };
    if !receivable {
        return Err(DescriptorError::new(format!(
            "`{field}` must be keeper://oauth/… or an http://127.0.0.1 loopback address: \"{value}\"."
        )));
    }
    Ok(())
}

/// What a person handed keeper: an address to fetch, or the descriptor itself.
#[derive(Debug, Clone, PartialEq, Eq)]
// One value per paste, consumed at once; boxing would only cost every caller a
// deref for a few hundred bytes that never sit in a collection.
#[allow(clippy::large_enum_variant)]
pub enum SetupInput {
    /// An `https` descriptor URL, still to be [`fetch`]ed.
    Url(url::Url),
    Inline(AccountDescriptor),
}

const NOT_A_SETUP_LINK: &str =
    "That is not a keeper setup link. Paste a keeper://setup link or an https:// address.";

/// `keeper://setup?descriptor=<https URL>`, `keeper://setup?d=<base64url JSON>`,
/// or a bare `https://` URL.
pub fn parse_setup_input(text: &str) -> Result<SetupInput, DescriptorError> {
    let text = text.trim();
    let url = url::Url::parse(text).map_err(|_| DescriptorError::new(NOT_A_SETUP_LINK))?;
    match url.scheme() {
        "https" => Ok(SetupInput::Url(https_only(url)?)),
        "keeper" if url.host_str() == Some("setup") => {
            let mut descriptor = None;
            let mut inline = None;
            for (key, value) in url.query_pairs() {
                match key.as_ref() {
                    "descriptor" => descriptor = Some(value.into_owned()),
                    "d" => inline = Some(value.into_owned()),
                    _ => {}
                }
            }
            match (descriptor, inline) {
                (Some(source), None) => {
                    let source = url::Url::parse(&source).map_err(|_| {
                        DescriptorError::new("The setup link's descriptor address is not a URL.")
                    })?;
                    Ok(SetupInput::Url(https_only(source)?))
                }
                (None, Some(encoded)) => decode_inline(&encoded).map(SetupInput::Inline),
                (Some(_), Some(_)) => Err(DescriptorError::new(
                    "The setup link carries both a descriptor address and an inline descriptor; keeper will not guess which one you meant.",
                )),
                (None, None) => Err(DescriptorError::new(
                    "The setup link carries no descriptor.",
                )),
            }
        }
        "http" => Err(DescriptorError::new(
            "keeper only reads an account descriptor over https.",
        )),
        _ => Err(DescriptorError::new(NOT_A_SETUP_LINK)),
    }
}

fn https_only(url: url::Url) -> Result<url::Url, DescriptorError> {
    if url.scheme() != "https" || url.host().is_none() {
        return Err(DescriptorError::new(
            "keeper only reads an account descriptor over https.",
        ));
    }
    Ok(url)
}

fn decode_inline(encoded: &str) -> Result<AccountDescriptor, DescriptorError> {
    let encoded = encoded.trim_end_matches('=');
    if encoded.len() > MAX_DESCRIPTOR_BYTES * 4 / 3 + 4 {
        return Err(DescriptorError::new(
            "The setup link's descriptor is too large.",
        ));
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| DescriptorError::new("The setup link's inline descriptor is damaged."))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| DescriptorError::new("The setup link's inline descriptor is damaged."))?;
    parse_json(&text)
}

/// The link another device or person opens: `descriptor=` when keeper knows
/// where the descriptor is served, else the whole descriptor inline.
pub fn setup_link(d: &AccountDescriptor, source: Option<&url::Url>) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    match source {
        Some(source) => query.append_pair("descriptor", source.as_str()),
        // A struct of strings always serialises.
        None => query.append_pair(
            "d",
            &URL_SAFE_NO_PAD.encode(serde_json::to_vec(d).unwrap_or_default()),
        ),
    };
    format!("keeper://setup?{}", query.finish())
}

/// Read a descriptor from an operator's `https` URL: no redirects, at most
/// 64 KiB, then [`parse_json`].
pub async fn fetch(
    http: &reqwest::Client,
    url: &url::Url,
) -> Result<AccountDescriptor, AccountError> {
    if url.scheme() != "https" {
        return Err(AccountError::Refused(
            "keeper only reads an account descriptor over https.".to_owned(),
        ));
    }
    get_descriptor(http, url).await
}

/// [`fetch`] without the scheme check, so tests can serve plain http on
/// loopback.
async fn get_descriptor(
    http: &reqwest::Client,
    url: &url::Url,
) -> Result<AccountDescriptor, AccountError> {
    let host = url.host_str().unwrap_or("the server");
    let mut response = http
        .get(url.clone())
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(FETCH_TIMEOUT)
        .send()
        .await
        .map_err(|error| {
            AccountError::Unreachable(format!(
                "keeper could not reach {host} to read the account descriptor: {error}"
            ))
        })?;
    // The client may follow redirects on its own; a moved URL is refused
    // either way, because the confirmation sheet names the host the person
    // was given, not wherever it bounced.
    if response.status().is_redirection() || response.url() != url {
        return Err(AccountError::Refused(format!(
            "{host} redirected the setup address elsewhere. keeper reads a descriptor only from the exact address it was given."
        )));
    }
    if !response.status().is_success() {
        return Err(AccountError::Unreachable(format!(
            "{host} answered {} for the account descriptor.",
            response.status()
        )));
    }
    let too_large = || {
        AccountError::Refused(format!(
            "The account descriptor at {host} is larger than 64 KiB; keeper will not read it."
        ))
    };
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DESCRIPTOR_BYTES as u64)
    {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        AccountError::Unreachable(format!(
            "keeper could not read the account descriptor from {host}: {error}"
        ))
    })? {
        if body.len() + chunk.len() > MAX_DESCRIPTOR_BYTES {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    let text = String::from_utf8(body).map_err(|_| {
        AccountError::Refused(format!("The account descriptor at {host} is not text."))
    })?;
    Ok(parse_json(&text)?)
}

/// Read `account.toml`. Absent is no account; anything wrong is a fault shown
/// with the layer faults, and still no account.
pub fn load(path: &Path) -> Result<Option<AccountDescriptor>, LayerFault> {
    let fault = |kind, message: String, line| LayerFault {
        tier: Some(LayerTier::AccountDescriptor),
        line,
        ..LayerFault::late(kind, path, message)
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(fault(
                LayerFaultKind::Unreadable,
                format!("could not read {FILE_NAME}: {error}"),
                None,
            ));
        }
    };
    parse_toml(&text)
        .map(Some)
        .map_err(|error| fault(LayerFaultKind::Malformed, error.message, error.line))
}

/// Write `account.toml` atomically: a reader sees the old file or the new
/// one, never half of either.
pub fn store(path: &Path, d: &AccountDescriptor) -> Result<(), CoreError> {
    use std::io::Write as _;

    let text = to_toml(d);
    let write = || -> std::io::Result<()> {
        let parent = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
        std::fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(".{FILE_NAME}.{}.tmp", std::process::id()));
        let result = (|| {
            let mut file = std::fs::File::create(&temporary)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temporary, path)?;
            #[cfg(unix)]
            std::fs::File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    };
    write().map_err(|error| CoreError::Internal(format!("could not write {FILE_NAME}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec §2.5 B with every default left out.
    const MINIMAL: &str = r#"{
        "version": 1,
        "id": "acme",
        "name": "Acme",
        "auth": { "issuer": "https://id.acme.dev", "client_id": "keeper" },
        "config": { "url": "https://git.acme.dev/git/people/keeper-config.git" }
    }"#;

    /// Spec §2.5 A: Zitadel plus a Forgejo forge in `oauth` mode.
    const FORGE: &str = r#"{
        "version": 1,
        "id": "acme",
        "name": "Acme",
        "auth": {
            "issuer": "https://id.acme.dev",
            "client_id": "283746519283746519@keeper",
            "extra_scopes": ["urn:zitadel:iam:org:projects:roles"],
            "trusted_audiences": ["283746519283746001"],
            "roles_claim": "urn:zitadel:iam:org:project:283746519283746001:roles",
            "required_role": "keeper"
        },
        "config": {
            "url": "https://git.acme.dev/git/people/keeper-config.git",
            "api_base": "https://git.acme.dev/git/api/v1",
            "identity_field": "zitadel_id",
            "auth": {
                "mode": "oauth",
                "issuer": "https://git.acme.dev/git",
                "client_id": "6f1c2a8e",
                "scope": "openid profile read:user write:repository",
                "signin_url": "https://git.acme.dev/git/user/oauth2/acme-id?redirect_to={authorize_path_and_query}"
            }
        }
    }"#;

    fn edited(edit: impl FnOnce(&mut serde_json::Value)) -> String {
        let mut value: serde_json::Value = serde_json::from_str(MINIMAL).expect("fixture");
        edit(&mut value);
        value.to_string()
    }

    fn refusal(text: &str) -> String {
        parse_json(text).expect_err("must be refused").message
    }

    #[test]
    fn omitted_fields_take_the_spec_defaults() {
        let d = parse_json(MINIMAL).expect("minimal descriptor");
        assert_eq!(
            d.config.auth,
            RepoAuthConfig::Same {
                scheme: GitScheme::Basic,
                username: "oauth2".to_owned()
            }
        );
        assert_eq!(
            d.auth.scopes,
            ["openid", "profile", "email", "offline_access"]
        );
        assert_eq!(d.auth.username_claim, "preferred_username");
        assert_eq!(d.config.identity_field, "sub");
        assert_eq!(d.config.branch, "main");
        assert_eq!(d.redirect_uri(), "keeper://oauth/acme/callback");
        assert_eq!(d.forge_redirect_uri(), None);
        assert_eq!(d.repo_mode(), "same");

        let same_only = edited(|v| v["config"]["auth"] = serde_json::json!({ "mode": "same" }));
        assert_eq!(
            parse_json(&same_only).expect("mode only").config.auth,
            d.config.auth
        );
    }

    #[test]
    fn the_api_base_is_never_derived_from_the_repository_url() {
        let d = parse_json(MINIMAL).expect("minimal descriptor");
        assert_eq!(d.config.api_base, None);
        assert_eq!(
            parse_toml(&to_toml(&d))
                .expect("round trip")
                .config
                .api_base,
            None
        );

        // An oauth forge whose username could only come from an API base keeper
        // would have to guess is refused, not completed from `url`.
        let guessing = edited(|v| {
            v["config"]["auth"] = serde_json::json!({
                "mode": "oauth",
                "authorize_url": "https://git.acme.dev/login/oauth/authorize",
                "token_url": "https://git.acme.dev/login/oauth/access_token",
                "client_id": "c",
                "scope": "read:user"
            });
        });
        assert!(refusal(&guessing).contains("forge username"));
    }

    #[test]
    fn the_forge_example_parses_and_survives_toml() {
        let d = parse_json(FORGE).expect("spec example A");
        let RepoAuthConfig::Oauth(forge) = &d.config.auth else {
            panic!("oauth mode");
        };
        assert_eq!(forge.username_field, "login");
        assert_eq!(forge.scope, "openid profile read:user write:repository");
        assert_eq!(
            d.forge_redirect_uri().as_deref(),
            Some("keeper://oauth/acme/forge/callback")
        );
        assert_eq!(d.forge_host().as_deref(), Some("git.acme.dev"));
        assert_eq!(parse_toml(&to_toml(&d)).expect("round trip"), d);

        let bearer = edited(|v| {
            v["config"]["auth"] = serde_json::json!({ "mode": "same", "scheme": "bearer" });
        });
        let d = parse_json(&bearer).expect("bearer");
        assert_eq!(parse_toml(&to_toml(&d)).expect("round trip"), d);
        let none = edited(|v| v["config"]["auth"] = serde_json::json!({ "mode": "none" }));
        let d = parse_json(&none).expect("none");
        assert_eq!(parse_toml(&to_toml(&d)).expect("round trip"), d);
    }

    #[test]
    fn every_spec_refusal_is_enforced() {
        let cases: Vec<(&str, String)> = vec![
            (
                "secret",
                edited(|v| v["auth"]["client_secret"] = "s3cret".into()),
            ),
            (
                "secret",
                edited(|v| v["config"]["auth"]["client_secret"] = "s3cret".into()),
            ),
            (
                "https",
                edited(|v| v["auth"]["issuer"] = "http://id.acme.dev".into()),
            ),
            (
                "https",
                edited(|v| v["config"]["url"] = "http://git.acme.dev/x.git".into()),
            ),
            (
                "https",
                edited(|v| v["config"]["api_base"] = "http://git.acme.dev/api".into()),
            ),
            (
                "https",
                edited(|v| v["config"]["url"] = "ssh://git@git.acme.dev/x.git".into()),
            ),
            (
                "user name or password",
                edited(|v| {
                    v["config"]["url"] = "https://me:token@git.acme.dev/x.git".into();
                }),
            ),
            ("account id", edited(|v| v["id"] = "Acme".into())),
            ("account id", edited(|v| v["id"] = "".into())),
            ("account id", edited(|v| v["id"] = "a".repeat(33).into())),
            ("version", edited(|v| v["version"] = 2.into())),
            (
                "openid",
                edited(|v| v["auth"]["scopes"] = serde_json::json!(["profile"])),
            ),
            (
                "keeper://oauth",
                edited(|v| {
                    v["auth"]["redirect_uri"] = "https://acme.dev/callback".into();
                }),
            ),
            (
                "only applies to mode = \"oauth\"",
                edited(|v| {
                    v["config"]["auth"] = serde_json::json!({ "mode": "same", "client_id": "c" });
                }),
            ),
            (
                "issuer",
                edited(|v| {
                    v["config"]["auth"] =
                        serde_json::json!({ "mode": "oauth", "client_id": "c", "scope": "openid" });
                }),
            ),
            (
                "unknown field",
                edited(|v| v["auth"]["requird_role"] = "keeper".into()),
            ),
        ];
        for (expected, text) in cases {
            let message = refusal(&text);
            assert!(
                message.contains(expected),
                "{expected:?} not in {message:?}"
            );
        }
    }

    #[test]
    fn an_endpoint_override_on_another_host_is_refused_naming_both_hosts() {
        for field in [
            "authorization",
            "token",
            "userinfo",
            "revocation",
            "end_session",
            "jwks",
        ] {
            let text = edited(|v| {
                v["auth"]["endpoints"] = serde_json::json!({ field: "https://evil.example/x" });
            });
            let message = refusal(&text);
            assert!(
                message.contains("evil.example") && message.contains("id.acme.dev"),
                "{field}: {message}"
            );
        }
        let same_host = edited(|v| {
            v["auth"]["endpoints"] =
                serde_json::json!({ "token": "https://id.acme.dev/oauth/v2/token" });
        });
        assert!(parse_json(&same_host).is_ok());
    }

    #[test]
    fn forge_endpoints_and_the_api_base_stay_on_the_forge_or_repository_host() {
        let forge = |auth: serde_json::Value, api_base: &str| {
            edited(|v| {
                v["config"]["api_base"] = api_base.into();
                v["config"]["auth"] = auth;
            })
        };
        let plain = |authorize: &str, token: &str| {
            serde_json::json!({ "mode": "oauth", "client_id": "c", "scope": "read:user",
                                "authorize_url": authorize, "token_url": token })
        };
        let good = "https://git.acme.dev/api/v1";
        // Without a forge issuer the endpoints belong to the repository host.
        assert!(parse_json(&forge(
            plain(
                "https://git.acme.dev/login/oauth/authorize",
                "https://git.acme.dev/login/oauth/access_token"
            ),
            good
        ))
        .is_ok());
        for (auth, api_base, bad_host) in [
            (
                plain(
                    "https://git.acme.dev/authorize",
                    "https://evil.example/token",
                ),
                good,
                "evil.example",
            ),
            (
                plain(
                    "https://evil.example/authorize",
                    "https://git.acme.dev/token",
                ),
                good,
                "evil.example",
            ),
            (
                plain("https://git.acme.dev/a", "https://git.acme.dev/t"),
                "https://api.evil.example/v1",
                "api.evil.example",
            ),
            (
                serde_json::json!({ "mode": "oauth", "client_id": "c", "scope": "read:user",
                                    "issuer": "https://forge.acme.dev",
                                    "token_url": "https://git.acme.dev/token" }),
                good,
                "git.acme.dev",
            ),
            (
                serde_json::json!({ "mode": "oauth", "client_id": "c", "scope": "read:user",
                                    "issuer": "https://forge.acme.dev",
                                    "user_url": "https://evil.example/user" }),
                good,
                "evil.example",
            ),
        ] {
            let message = refusal(&forge(auth, api_base));
            assert!(message.contains(bad_host), "{message}");
        }
        // An api_base on the forge issuer's host is the forge's own API.
        let on_forge = forge(
            serde_json::json!({ "mode": "oauth", "client_id": "c", "scope": "openid",
                                "issuer": "https://forge.acme.dev" }),
            "https://forge.acme.dev/api/v1",
        );
        assert!(parse_json(&on_forge).is_ok(), "{}", refusal(&on_forge));
    }

    #[test]
    fn a_query_is_refused_everywhere_but_the_signin_url() {
        for (field, text) in [
            (
                "config.url",
                edited(|v| {
                    v["config"]["url"] = "https://git.acme.dev/x.git?private_token=t".into()
                }),
            ),
            (
                "config.api_base",
                edited(|v| {
                    v["config"]["api_base"] = "https://git.acme.dev/api?access_token=t".into()
                }),
            ),
            (
                "auth.endpoints.token",
                edited(|v| {
                    v["auth"]["endpoints"] =
                        serde_json::json!({ "token": "https://id.acme.dev/token?k=v" });
                }),
            ),
            (
                "config.auth.token_url",
                edited(|v| {
                    v["config"]["api_base"] = "https://git.acme.dev/api".into();
                    v["config"]["auth"] = serde_json::json!({
                        "mode": "oauth", "client_id": "c", "scope": "s",
                        "authorize_url": "https://git.acme.dev/a",
                        "token_url": "https://git.acme.dev/t?secret_key=1" });
                }),
            ),
        ] {
            let message = refusal(&text);
            assert!(
                message.contains(field) && message.contains("query"),
                "{message}"
            );
        }
        assert!(parse_json(FORGE).is_ok(), "signin_url keeps its query");
    }

    #[test]
    fn an_identity_field_keeper_writes_itself_is_refused() {
        for field in ["login", "display_name", "issuer", "created"] {
            let text = edited(|v| v["config"]["identity_field"] = field.into());
            assert!(refusal(&text).contains("identity_field"), "{field}");
        }
        let own = edited(|v| v["config"]["identity_field"] = "zitadel_id".into());
        assert!(parse_json(&own).is_ok());
    }

    #[test]
    fn a_loopback_redirect_must_name_127_0_0_1_literally() {
        for redirect in [
            "http://localhost/callback",
            "http://127.0.0.2:8080/callback",
            "http://[::1]/callback",
        ] {
            let text = edited(|v| v["auth"]["redirect_uri"] = redirect.into());
            assert!(refusal(&text).contains("127.0.0.1"), "{redirect}");
        }
        let exact = edited(|v| v["auth"]["redirect_uri"] = "http://127.0.0.1:8123/cb".into());
        assert!(parse_json(&exact).is_ok());
    }

    #[test]
    fn loopback_http_is_accepted_for_test_providers() {
        let local = edited(|v| {
            v["auth"]["issuer"] = "http://127.0.0.1:8080".into();
            v["config"]["url"] = "http://localhost:3000/x.git".into();
            v["auth"]["redirect_uri"] = "http://127.0.0.1/callback".into();
        });
        assert!(parse_json(&local).is_ok());
    }

    #[test]
    fn a_secret_in_the_stored_toml_is_refused_too() {
        let d = parse_json(MINIMAL).expect("minimal descriptor");
        let text = to_toml(&d).replace("client_id = ", "client_secret = \"x\"\nclient_id = ");
        assert!(parse_toml(&text)
            .expect_err("refused")
            .message
            .contains("secret"));
    }

    #[test]
    fn forges_and_a_broker_parse_with_github_defaults_and_survive_toml() {
        let text = edited(|v| {
            v["forges"] = serde_json::json!([
                { "kind": "github" },
                { "kind": "github", "id": "ghe", "name": "Acme GitHub",
                  "web_base": "https://ghe.acme.dev", "api_base": "https://ghe.acme.dev/api/v3",
                  "client_id": "Iv1.ghe" }
            ]);
            v["github_broker"] = serde_json::json!({ "url": "https://broker.acme.dev:8455",
                                                    "app": "tgdev" });
        });
        let d = parse_json(&text).expect("forges descriptor");
        let github = &d.forges[0];
        assert_eq!(github.source_id(), "github");
        assert_eq!(github.web_base(), Some("https://github.com"));
        assert_eq!(github.api_base(), Some("https://api.github.com"));
        assert_eq!(d.forges[1].source_id(), "ghe");
        let broker = d.github_broker.as_ref().expect("broker");
        assert_eq!(broker.app.as_deref(), Some("tgdev"));
        assert_eq!(broker.host(), "broker.acme.dev");
        assert_eq!(parse_toml(&to_toml(&d)).expect("toml round trip"), d);
        // The sign-in is unchanged, so editing forges never signs anyone out.
        let plain = parse_json(MINIMAL).expect("minimal");
        assert!(!d.replaces(&plain));
    }

    #[test]
    fn forge_entries_refuse_http_secrets_duplicates_foreign_apis_and_a_moved_brokered_github() {
        let with = |forges: serde_json::Value, broker: Option<serde_json::Value>| {
            edited(|v| {
                v["forges"] = forges;
                if let Some(broker) = broker {
                    v["github_broker"] = broker;
                }
            })
        };
        let ghe = |extra: serde_json::Value| {
            let mut entry = serde_json::json!({ "kind": "github", "id": "ghe",
                "web_base": "https://ghe.acme.dev", "api_base": "https://ghe.acme.dev/api/v3",
                "client_id": "Iv1.ghe" });
            for (k, value) in extra.as_object().expect("object") {
                entry[k] = value.clone();
            }
            entry
        };
        for (needle, text) in [
            (
                "forges[0].api_base",
                with(
                    serde_json::json!([ghe(
                        serde_json::json!({ "api_base": "http://ghe.acme.dev/api/v3" })
                    )]),
                    None,
                ),
            ),
            (
                "forges[0].web_base",
                with(
                    serde_json::json!([
                        { "kind": "github", "web_base": "http://github.example" }
                    ]),
                    None,
                ),
            ),
            (
                "public client",
                with(
                    serde_json::json!([
                        { "kind": "github", "client_id": "c", "client_secret": "s" }
                    ]),
                    None,
                ),
            ),
            (
                "share the id \"github\"",
                with(
                    serde_json::json!([{ "kind": "github" }, { "kind": "github" }]),
                    None,
                ),
            ),
            (
                "share the id \"ghe\"",
                with(
                    serde_json::json!([ghe(serde_json::json!({})), ghe(serde_json::json!({}))]),
                    None,
                ),
            ),
            (
                "must be 1 to 32",
                with(
                    serde_json::json!([ghe(serde_json::json!({ "id": "Big" }))]),
                    None,
                ),
            ),
            // github.com's token goes to api.github.com and nowhere else.
            (
                "must be https://api.github.com for github.com",
                with(
                    serde_json::json!([
                        { "kind": "github", "api_base": "https://collector.evil" }
                    ]),
                    None,
                ),
            ),
            (
                "must be https://api.github.com for github.com",
                with(
                    serde_json::json!([
                        { "kind": "github", "id": "work", "web_base": "https://GitHub.com/",
                          "api_base": "https://api.github.com/v3" }
                    ]),
                    None,
                ),
            ),
            // Any other forge's API lives on its own host.
            (
                "`forges[0].api_base` points at collector.evil",
                with(
                    serde_json::json!([ghe(
                        serde_json::json!({ "api_base": "https://collector.evil/api/v3" })
                    )]),
                    None,
                ),
            ),
            (
                "keeper lists only the account's own Forgejo; remove this [[forges]] entry.",
                with(
                    serde_json::json!([{ "kind": "forgejo", "id": "cb",
                        "web_base": "https://codeberg.org",
                        "api_base": "https://codeberg.org/api/v1" }]),
                    None,
                ),
            ),
            (
                "github_broker.url",
                with(
                    serde_json::json!([]),
                    Some(serde_json::json!({ "url": "http://broker.acme.dev" })),
                ),
            ),
            (
                "must be github.com too",
                with(
                    serde_json::json!([
                        { "kind": "github", "web_base": "https://ghe.acme.dev",
                          "api_base": "https://ghe.acme.dev/api/v3" }
                    ]),
                    Some(serde_json::json!({ "url": "https://b.acme.dev" })),
                ),
            ),
            (
                "unknown field",
                with(
                    serde_json::json!([]),
                    Some(serde_json::json!({ "url": "https://b.acme.dev", "forges": ["github"] })),
                ),
            ),
        ] {
            let message = refusal(&text);
            assert!(message.contains(needle), "{needle}: {message}");
        }
        for fine in [
            // A GitHub Enterprise entry is fine without a broker.
            with(serde_json::json!([ghe(serde_json::json!({}))]), None),
            with(
                serde_json::json!([ghe(serde_json::json!({
                    "web_base": "http://127.0.0.1:3000", "api_base": "http://127.0.0.1:3000/api/v3"
                }))]),
                None,
            ),
            // github.com spelt with a trailing slash, next to the broker.
            with(
                serde_json::json!([
                    { "kind": "github", "web_base": "https://github.com/",
                      "api_base": "https://api.github.com/" }
                ]),
                Some(serde_json::json!({ "url": "https://b.acme.dev" })),
            ),
        ] {
            assert!(parse_json(&fine).is_ok(), "{fine}");
        }
    }

    #[test]
    fn the_account_serves_only_remotes_on_its_own_hosts() {
        let d = parse_json(FORGE).expect("spec example A");
        assert!(d.serves_remote("https://git.acme.dev/git/people/notes.git"));
        assert!(d.serves_remote("https://GIT.acme.dev:443/git/tgorka/tgdrive"));
        // Where the account's token would be a stranger's: GitHub (the
        // hesperia report, 2026-09-24), a look-alike, plain http, scp.
        assert!(!d.serves_remote("https://github.com/tgorka/bmad-stepper.git"));
        assert!(!d.serves_remote("https://git.acme.dev.evil.com/x.git"));
        assert!(!d.serves_remote("http://git.acme.dev/git/people/notes.git"));
        assert!(!d.serves_remote("git@git.acme.dev:people/notes.git"));
    }

    #[test]
    fn setup_links_round_trip_both_forms() {
        let d = parse_json(FORGE).expect("spec example A");
        let inline = setup_link(&d, None);
        assert!(inline.starts_with("keeper://setup?d="));
        assert_eq!(
            parse_setup_input(&inline).expect("inline"),
            SetupInput::Inline(d.clone())
        );

        let source = url::Url::parse("https://id.acme.dev/.well-known/keeper-account.json?v=1&x=2")
            .expect("url");
        let linked = setup_link(&d, Some(&source));
        assert_eq!(
            parse_setup_input(&linked).expect("linked"),
            SetupInput::Url(source.clone())
        );
        assert_eq!(
            parse_setup_input(&format!("  {source}\n")).expect("bare"),
            SetupInput::Url(source)
        );
    }

    #[test]
    fn setup_input_refuses_anything_but_https_and_keeper_setup() {
        for bad in [
            "http://id.acme.dev/keeper-account.json",
            "keeper://setup?descriptor=http%3A%2F%2Fid.acme.dev%2Fa.json",
            "keeper://oauth/acme/callback?d=e30",
            "keeper://setup",
            "keeper://setup?d=%%%",
            "keeper://setup?d=e30&descriptor=https%3A%2F%2Fa.dev%2Fa.json",
            "file:///etc/passwd",
            "not a link",
        ] {
            assert!(parse_setup_input(bad).is_err(), "{bad}");
        }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "keeper-org-account-{name}-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn store_then_load_returns_the_descriptor_and_absent_is_no_account() {
        let dir = scratch("store");
        let path = dir.join(FILE_NAME);
        assert_eq!(load(&path), Ok(None));

        let d = parse_json(FORGE).expect("spec example A");
        store(&path, &d).expect("store");
        assert_eq!(load(&path), Ok(Some(d)));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_broken_account_toml_is_a_fault_on_the_descriptor_tier() {
        let dir = scratch("fault");
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, "version = 1\nid = \"acme\"\nname = \n").expect("write");
        let fault = load(&path).expect_err("malformed");
        assert_eq!(fault.tier, Some(LayerTier::AccountDescriptor));
        assert_eq!(fault.kind, LayerFaultKind::Malformed);
        assert_eq!(fault.line, Some(3));

        std::fs::write(
            &path,
            to_toml(&parse_json(MINIMAL).expect("fixture")).replace("https://id", "http://id"),
        )
        .expect("write");
        let fault = load(&path).expect_err("refused");
        assert_eq!(fault.tier, Some(LayerTier::AccountDescriptor));
        assert!(fault.message.contains("https"), "{}", fault.message);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Serve `responses` in order, one connection each, on loopback.
    fn serve(responses: Vec<Vec<u8>>) -> url::Url {
        use std::io::{Read as _, Write as _};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for response in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(&response);
            }
        });
        url::Url::parse(&format!("http://127.0.0.1:{port}/keeper-account.json")).expect("url")
    }

    fn ok_response(body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[tokio::test]
    async fn a_fetch_reads_a_descriptor_under_the_cap() {
        let padded = format!(
            "{MINIMAL}{}",
            " ".repeat(MAX_DESCRIPTOR_BYTES - MINIMAL.len())
        );
        let url = serve(vec![ok_response(&padded)]);
        let d = get_descriptor(&reqwest::Client::new(), &url)
            .await
            .expect("fetched");
        assert_eq!(d.id, "acme");
    }

    #[tokio::test]
    async fn a_fetch_refuses_a_body_over_64_kib() {
        let padded = format!(
            "{MINIMAL}{}",
            " ".repeat(MAX_DESCRIPTOR_BYTES + 1 - MINIMAL.len())
        );
        let url = serve(vec![ok_response(&padded)]);
        let result = get_descriptor(&reqwest::Client::new(), &url).await;
        assert!(matches!(result, Err(AccountError::Refused(m)) if m.contains("64 KiB")));

        // Without a Content-Length the streamed cap still holds.
        let body = format!("{MINIMAL}{}", " ".repeat(MAX_DESCRIPTOR_BYTES));
        let mut chunked = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
        chunked.extend_from_slice(body.as_bytes());
        let url = serve(vec![chunked]);
        let result = get_descriptor(&reqwest::Client::new(), &url).await;
        assert!(matches!(result, Err(AccountError::Refused(m)) if m.contains("64 KiB")));
    }

    #[tokio::test]
    async fn a_fetch_refuses_a_redirect_even_when_the_client_follows_it() {
        let redirect = b"HTTP/1.1 302 Found\r\nLocation: /elsewhere.json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec();
        let url = serve(vec![redirect, ok_response(MINIMAL)]);
        let result = get_descriptor(&reqwest::Client::new(), &url).await;
        assert!(matches!(result, Err(AccountError::Refused(m)) if m.contains("redirected")));
    }

    #[tokio::test]
    async fn fetch_refuses_plain_http_before_any_request() {
        let url = url::Url::parse("http://127.0.0.1:9/keeper-account.json").expect("url");
        assert!(matches!(
            fetch(&reqwest::Client::new(), &url).await,
            Err(AccountError::Refused(_))
        ));
    }
}
