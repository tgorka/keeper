//! keeper's own OIDC client for the organisation account (AD-310, AD-311).
//!
//! `openidconnect` 4 does discovery, PKCE, the token exchange and the ID-token
//! signature, `iss`, `aud` and `nonce` checks, on the app's reqwest through
//! `oauth2-reqwest`. What the library leaves to its caller is here, in the
//! order of the validation checklist the research digest pins (ROidc §2):
//!
//! - allowed algorithms are discovery's `id_token_signing_alg_values_supported`
//!   ∩ the asymmetric set, and a JWKS without an asymmetric key is refused
//!   before the browser opens;
//! - `state`, `nonce` and the PKCE S256 verifier are fresh per attempt and
//!   consumed once; the callback must arrive on the exact redirect URI with
//!   exactly the expected `state`, and an RFC 9207 `iss` must be the issuer;
//! - every `aud` besides `client_id` must be in `trusted_audiences`; `azp`,
//!   when present, must be `client_id`;
//! - `exp` and `iat` get 60 s of leeway;
//! - `at_hash`, when present, must match the access token, because that token
//!   is reused as a credential elsewhere;
//! - an unknown `kid` refetches the JWKS once;
//! - UserInfo is used only when its `sub` equals the ID token's.
//!
//! Tokens are persisted to the keychain before any function here returns
//! them, and a rotated refresh token is written before the new access token is
//! used: rotation is universal, so a token lost between the provider rotating
//! it and keeper writing it is a signed-out person.

use std::future::Future;
use std::time::Duration;

use base64::Engine as _;
use chrono::{TimeDelta, Utc};
use oauth2_reqwest::ReqwestClient;
use openidconnect::core::{
    CoreAuthDisplay, CoreAuthenticationFlow, CoreClaimName, CoreClaimType, CoreClient,
    CoreClientAuthMethod, CoreErrorResponseType, CoreGrantType, CoreIdToken, CoreIdTokenVerifier,
    CoreJsonWebKey, CoreJsonWebKeySet, CoreJsonWebKeyType, CoreJweContentEncryptionAlgorithm,
    CoreJweKeyManagementAlgorithm, CoreJwsSigningAlgorithm, CoreResponseMode, CoreResponseType,
    CoreSubjectIdentifierType, CoreTokenResponse,
};
use openidconnect::{
    AccessToken, AccessTokenHash, AdditionalProviderMetadata, AsyncHttpClient, AuthUrl,
    AuthorizationCode, ClaimsVerificationError, ClientId, CsrfToken, DiscoveryError, EndSessionUrl,
    EndpointNotSet, EndpointSet, HttpClientError, HttpRequest, HttpResponse, IssuerUrl,
    JsonWebKey as _, JsonWebKeySetUrl, LogoutRequest, Nonce, OAuth2TokenResponse,
    PkceCodeChallenge, PkceCodeVerifier, ProviderMetadata, RedirectUrl, RefreshToken,
    RequestTokenError, RevocationUrl, Scope, SignatureVerificationError, TokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};
use url::Url;

use super::descriptor::{AccountDescriptor, Endpoints, ForgeOauth, RepoAuthConfig};
use super::loopback::{self, Listener};
use super::session::{
    self, forge_key, load_bound_forge, load_bound_session, load_forge, load_session, refresh_lock,
    session_key, store_forge, store_session, Binding, DeviceApp, Identity, StoredForge,
    StoredSession,
};
use super::{claims, AccountError};
use crate::oauth::{address_of, within_reach, OAuthCallback, OAuthFlowRegistry};
use crate::platform::Platform;

/// How long a sign-in waits for the browser to come back.
pub const FLOW_TIMEOUT: Duration = Duration::from_secs(300);

/// Clock skew tolerated on `exp` and `iat`.
const LEEWAY: TimeDelta = TimeDelta::seconds(60);

/// The oldest `iat` accepted: an ID token is minted by the exchange that
/// returns it, so anything older is a replay.
const IAT_MAX_AGE: TimeDelta = TimeDelta::minutes(10);

/// An access token is refreshed this long before it expires.
const REFRESH_EARLY_MS: i64 = 60_000;

/// How long sign-out's best-effort network calls may take before the keychain
/// items are deleted anyway.
const SIGN_OUT_NETWORK_BUDGET: Duration = Duration::from_secs(15);

/// The signature algorithms keeper verifies. HMAC needs the client secret a
/// public client does not have, and ES512 is unsupported by the library.
const ASYMMETRIC_ALGS: [CoreJwsSigningAlgorithm; 9] = [
    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha384,
    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha512,
    CoreJwsSigningAlgorithm::RsaSsaPssSha256,
    CoreJwsSigningAlgorithm::RsaSsaPssSha384,
    CoreJwsSigningAlgorithm::RsaSsaPssSha512,
    CoreJwsSigningAlgorithm::EcdsaP256Sha256,
    CoreJwsSigningAlgorithm::EcdsaP384Sha384,
    CoreJwsSigningAlgorithm::EdDsa,
];

/// The two discovery fields the core metadata type does not carry.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ExtraMetadata {
    #[serde(default)]
    revocation_endpoint: Option<String>,
    #[serde(default)]
    end_session_endpoint: Option<String>,
}

impl AdditionalProviderMetadata for ExtraMetadata {}

type Metadata = ProviderMetadata<
    ExtraMetadata,
    CoreAuthDisplay,
    CoreClientAuthMethod,
    CoreClaimName,
    CoreClaimType,
    CoreGrantType,
    CoreJweContentEncryptionAlgorithm,
    CoreJweKeyManagementAlgorithm,
    CoreJsonWebKey,
    CoreResponseMode,
    CoreResponseType,
    CoreSubjectIdentifierType,
>;

type FlowClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
    EndpointNotSet,
>;

/// What an OpenID provider adds on top of plain OAuth endpoints.
struct OidcMeta {
    issuer: IssuerUrl,
    jwks_url: JsonWebKeySetUrl,
    jwks: CoreJsonWebKeySet,
    algs: Vec<CoreJwsSigningAlgorithm>,
    trusted_audiences: Vec<String>,
    userinfo_url: Option<Url>,
    end_session_url: Option<EndSessionUrl>,
}

/// A provider as keeper uses it: discovery with the descriptor's overrides
/// applied, or (a forge without an issuer) plain OAuth endpoints.
struct Provider {
    host: String,
    client_id: ClientId,
    auth_url: AuthUrl,
    token_url: TokenUrl,
    revocation_url: Option<RevocationUrl>,
    oidc: Option<OidcMeta>,
}

/// The endpoint overrides a descriptor may carry, all optional.
#[derive(Default)]
struct Overrides<'a> {
    authorization: Option<&'a str>,
    token: Option<&'a str>,
    userinfo: Option<&'a str>,
    revocation: Option<&'a str>,
    end_session: Option<&'a str>,
    jwks: Option<&'a str>,
}

impl<'a> From<&'a Endpoints> for Overrides<'a> {
    fn from(e: &'a Endpoints) -> Self {
        Overrides {
            authorization: e.authorization.as_deref(),
            token: e.token.as_deref(),
            userinfo: e.userinfo.as_deref(),
            revocation: e.revocation.as_deref(),
            end_session: e.end_session.as_deref(),
            jwks: e.jwks.as_deref(),
        }
    }
}

fn host_of(raw: &str) -> String {
    Url::parse(raw)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_else(|| raw.to_owned())
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn bad_url(what: &str, host: &str) -> AccountError {
    AccountError::Refused(format!("{host} published an unusable {what} address."))
}

/// The HTTP client discovery asks through. Before a sign-in's browser opens
/// (`bounded`), each request has [`crate::oauth::REACH_TIMEOUT`]; a
/// background refresh's discovery keeps the HTTP client's own timeouts and
/// its quiet offline status. Either way a request that brings no response is
/// named by its own server — the issuer for its document, the key set's host
/// for its keys — never by the one asked before it (R541-03).
struct Discoverer<'a> {
    http: &'a ReqwestClient,
    bounded: bool,
}

impl<'a> Discoverer<'a> {
    /// A sign-in's discovery, before its browser opens.
    fn sign_in(http: &'a ReqwestClient) -> Self {
        Self {
            http,
            bounded: true,
        }
    }

    /// A background refresh's, or a key set read again after the browser.
    fn background(http: &'a ReqwestClient) -> Self {
        Self {
            http,
            bounded: false,
        }
    }
}

/// A discovery request that brought no response.
#[derive(Debug, thiserror::Error)]
enum Unreached {
    /// No answer within the reach budget: the sentence naming its server.
    #[error("{0}")]
    NoAnswer(String),
    /// The HTTP client failed (refused, reset, TLS…) reaching `address`, the
    /// `host[:port]` the request went to.
    #[error("{address}: {error}")]
    Failed {
        address: String,
        error: HttpClientError<reqwest::Error>,
    },
}

impl<'c> AsyncHttpClient<'c> for Discoverer<'_> {
    type Error = Unreached;
    type Future =
        std::pin::Pin<Box<dyn Future<Output = Result<HttpResponse, Unreached>> + Send + 'c>>;

    fn call(&'c self, request: HttpRequest) -> Self::Future {
        let address = address_of(&request.uri().to_string());
        Box::pin(async move {
            let asked = self.http.call(request);
            let answer = if self.bounded {
                within_reach(&address, asked)
                    .await
                    .map_err(Unreached::NoAnswer)?
            } else {
                asked.await
            };
            answer.map_err(|error| Unreached::Failed { address, error })
        })
    }
}

/// `host` names the provider; a request that brought no response names the
/// `host[:port]` it went to, which a person needs when the port is what a
/// network rule blocks.
fn discovery_error(host: &str, error: DiscoveryError<Unreached>) -> AccountError {
    match error {
        DiscoveryError::Request(Unreached::NoAnswer(sentence)) => AccountError::NoAnswer(sentence),
        DiscoveryError::Request(Unreached::Failed { address, .. }) => {
            AccountError::Unreachable(format!("Can't reach {address}."))
        }
        // A server having a bad minute is not a policy answer: "Blocked"
        // would tell the person to call an administrator about a 503.
        DiscoveryError::Response(status, ..) if status.is_server_error() => {
            AccountError::Unreachable(format!("{host} is not answering right now ({status})."))
        }
        DiscoveryError::Validation(reason) => AccountError::Refused(format!(
            "{host} is not the sign-in server this account names: {reason}"
        )),
        other => {
            tracing::warn!(%host, error = %other, "oidc discovery failed");
            AccountError::Refused(format!(
                "{host} did not answer as an OpenID Connect sign-in server."
            ))
        }
    }
}

/// An endpoint keeper will send a browser, a code or a token to: `https`, or
/// plain `http` to loopback when the issuer itself is a loopback test server.
/// Discovery is the provider's word, and a `file:` or `smb:` authorization
/// endpoint would otherwise be handed to the OS opener.
fn require_secure(what: &str, url: &Url, issuer: &Url, host: &str) -> Result<(), AccountError> {
    let secure = match url.scheme() {
        "https" => url.host().is_some(),
        "http" => is_loopback_host(url) && is_loopback_host(issuer),
        _ => false,
    };
    if secure {
        Ok(())
    } else {
        Err(bad_url(what, host))
    }
}

fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

/// Refuse a key set a public client cannot verify with: an authentik provider
/// without a signing key signs with HS256 and the client secret.
fn require_asymmetric(
    host: &str,
    jwks: CoreJsonWebKeySet,
) -> Result<CoreJsonWebKeySet, AccountError> {
    let asymmetric = jwks.keys().iter().any(|k| {
        matches!(
            k.key_type(),
            CoreJsonWebKeyType::RSA
                | CoreJsonWebKeyType::EllipticCurve
                | CoreJsonWebKeyType::OctetKeyPair
        )
    });
    if !asymmetric {
        return Err(AccountError::Refused(format!(
            "{host} publishes no public signing key, so keeper cannot verify its sign-ins. \
             Ask your administrator to give the application a signing key."
        )));
    }
    Ok(jwks)
}

async fn fetch_jwks(
    http: &Discoverer<'_>,
    host: &str,
    url: &JsonWebKeySetUrl,
) -> Result<CoreJsonWebKeySet, AccountError> {
    let jwks = CoreJsonWebKeySet::fetch_async(url, http)
        .await
        .map_err(|e| discovery_error(host, e))?;
    require_asymmetric(host, jwks)
}

/// Discovery for `issuer`, then the overrides, then the checks that must
/// fail before any browser opens.
async fn discover(
    http: &Discoverer<'_>,
    issuer: &str,
    client_id: &str,
    overrides: Overrides<'_>,
    trusted_audiences: &[String],
) -> Result<Provider, AccountError> {
    let host = host_of(issuer);
    let issuer_url = IssuerUrl::new(issuer.to_owned()).map_err(|_| bad_url("issuer", &host))?;
    let metadata = Metadata::discover_async(issuer_url.clone(), http)
        .await
        .map_err(|e| discovery_error(&host, e))?;

    let algs: Vec<CoreJwsSigningAlgorithm> = metadata
        .id_token_signing_alg_values_supported()
        .iter()
        .filter(|alg| ASYMMETRIC_ALGS.contains(alg))
        .cloned()
        .collect();
    if algs.is_empty() {
        return Err(AccountError::Refused(format!(
            "{host} signs sign-ins only with algorithms keeper cannot verify."
        )));
    }

    // Discovery already fetched `jwks_uri`; an override is fetched instead.
    let (jwks_url, jwks) = match overrides.jwks {
        Some(raw) => {
            let url =
                JsonWebKeySetUrl::new(raw.to_owned()).map_err(|_| bad_url("key set", &host))?;
            let jwks = fetch_jwks(http, &host, &url).await?;
            (url, jwks)
        }
        None => (
            metadata.jwks_uri().clone(),
            require_asymmetric(&host, metadata.jwks().clone())?,
        ),
    };

    let auth_url = match overrides.authorization {
        Some(raw) => AuthUrl::new(raw.to_owned()).map_err(|_| bad_url("authorization", &host))?,
        None => metadata.authorization_endpoint().clone(),
    };
    let token_url = match overrides.token {
        Some(raw) => TokenUrl::new(raw.to_owned()).map_err(|_| bad_url("token", &host))?,
        None => metadata
            .token_endpoint()
            .cloned()
            .ok_or_else(|| bad_url("token", &host))?,
    };
    let userinfo_url = match overrides.userinfo {
        Some(raw) => Some(Url::parse(raw).map_err(|_| bad_url("user info", &host))?),
        None => metadata.userinfo_endpoint().map(|u| u.url().clone()),
    };
    let extra = metadata.additional_metadata();
    let revocation_url = overrides
        .revocation
        .or(extra.revocation_endpoint.as_deref())
        .map(|raw| RevocationUrl::new(raw.to_owned()).map_err(|_| bad_url("revocation", &host)))
        .transpose()?;
    let end_session_url = overrides
        .end_session
        .or(extra.end_session_endpoint.as_deref())
        .map(|raw| EndSessionUrl::new(raw.to_owned()).map_err(|_| bad_url("sign-out", &host)))
        .transpose()?;

    let issuer_parsed = issuer_url.url();
    require_secure("key set", jwks_url.url(), issuer_parsed, &host)?;
    require_secure("authorization", auth_url.url(), issuer_parsed, &host)?;
    require_secure("token", token_url.url(), issuer_parsed, &host)?;
    if let Some(url) = &userinfo_url {
        require_secure("user info", url, issuer_parsed, &host)?;
    }
    if let Some(url) = &revocation_url {
        require_secure("revocation", url.url(), issuer_parsed, &host)?;
    }
    if let Some(url) = &end_session_url {
        require_secure("sign-out", url.url(), issuer_parsed, &host)?;
    }

    Ok(Provider {
        host,
        client_id: ClientId::new(client_id.to_owned()),
        auth_url,
        token_url,
        revocation_url,
        oidc: Some(OidcMeta {
            issuer: issuer_url,
            jwks_url,
            jwks,
            algs,
            trusted_audiences: trusted_audiences.to_vec(),
            userinfo_url,
            end_session_url,
        }),
    })
}

async fn account_provider(
    http: &Discoverer<'_>,
    d: &AccountDescriptor,
) -> Result<Provider, AccountError> {
    discover(
        http,
        &d.auth.issuer,
        &d.auth.client_id,
        Overrides::from(&d.auth.endpoints),
        &d.auth.trusted_audiences,
    )
    .await
}

/// The forge's provider: discovery when it names an issuer (explicit
/// authorize/token URLs still win), plain OAuth endpoints otherwise.
async fn forge_provider(http: &Discoverer<'_>, f: &ForgeOauth) -> Result<Provider, AccountError> {
    if let Some(issuer) = &f.issuer {
        return discover(
            http,
            issuer,
            &f.client_id,
            Overrides {
                authorization: f.authorize_url.as_deref(),
                token: f.token_url.as_deref(),
                ..Overrides::default()
            },
            &[],
        )
        .await;
    }
    let (Some(auth), Some(token)) = (&f.authorize_url, &f.token_url) else {
        return Err(AccountError::Refused(
            "The repository's sign-in names neither an issuer nor authorize and token URLs."
                .to_owned(),
        ));
    };
    let host = host_of(auth);
    Ok(Provider {
        client_id: ClientId::new(f.client_id.clone()),
        auth_url: AuthUrl::new(auth.clone()).map_err(|_| bad_url("authorization", &host))?,
        token_url: TokenUrl::new(token.clone()).map_err(|_| bad_url("token", &host))?,
        revocation_url: None,
        oidc: None,
        host,
    })
}

impl Provider {
    fn client(&self, redirect: Option<RedirectUrl>) -> FlowClient {
        let (issuer, jwks) = match &self.oidc {
            Some(oidc) => (oidc.issuer.clone(), oidc.jwks.clone()),
            // A plain-OAuth forge verifies no ID token; the client still
            // wants an issuer, and the authorization URL is the honest one.
            None => (
                IssuerUrl::from_url(self.auth_url.url().clone()),
                CoreJsonWebKeySet::new(Vec::new()),
            ),
        };
        let client = CoreClient::new(self.client_id.clone(), issuer, jwks)
            .set_auth_uri(self.auth_url.clone())
            .set_token_uri(self.token_url.clone())
            // The descriptor's scope list is sent as written, `openid` included.
            .disable_openid_scope();
        match redirect {
            Some(redirect) => client.set_redirect_uri(redirect),
            None => client,
        }
    }
}

/// Everything one authorization attempt produced, consumed by one exchange.
struct Grant {
    code: AuthorizationCode,
    verifier: PkceCodeVerifier,
    nonce: Nonce,
    redirect: RedirectUrl,
}

/// Removes an in-flight flow's registry entry on every exit path.
struct FlowGuard<'a> {
    flows: &'a OAuthFlowRegistry,
    state: &'a str,
}

impl Drop for FlowGuard<'_> {
    fn drop(&mut self) {
        self.flows.remove(self.state);
    }
}

/// What [`fill_signin_url`] leaves unencoded: RFC 3986's `unreserved` set
/// (`ALPHA / DIGIT / "-" / "." / "_" / "~"`), and nothing else.
const SIGNIN_VALUE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// A forge's `signin_url` with `{authorize_path_and_query}` filled with the
/// authorization request's path and query, so the forge's own
/// identity-provider login lands back on the authorization request.
///
/// The one place that substitution happens: the sign-in sheet (iOS), the
/// default browser (macOS, desktop) and the link on screen to copy by hand
/// all carry the URL this returns (`authorize` → `sign_in_link::offer`).
///
/// The value is percent-encoded down to RFC 3986's `unreserved` characters —
/// `&`, `=`, `?`, `/`, `%`, `#`, `+` and a space included — so it stays one
/// opaque value wherever the template puts it. Every template keeper has met
/// puts it in the query (`…/user/oauth2/<source>?redirect_to={…}`), where an
/// unencoded `&` hands the forge `client_id`, `state` and the rest as its
/// own parameters and keeps only `…/authorize?response_type=code` — which
/// Forgejo then answers with "Client ID not registered" (electra, 2026-10-28).
/// The same encoding is also a valid path segment and a valid fragment, so a
/// template that put it there would get one segment or one fragment, never
/// a path that walks elsewhere. A space is `%20`, not form encoding's `+`:
/// a forge that decodes the query as RFC 3986 rather than as a form reads
/// the same request.
fn fill_signin_url(signin_url: &str, authorize: &Url) -> String {
    let mut path_and_query = authorize.path().to_owned();
    if let Some(query) = authorize.query() {
        path_and_query.push('?');
        path_and_query.push_str(query);
    }
    let encoded = percent_encoding::utf8_percent_encode(&path_and_query, SIGNIN_VALUE).to_string();
    signin_url.replace(super::descriptor::AUTHORIZE_PLACEHOLDER, &encoded)
}

/// Check a callback against the attempt it answers and return its code.
fn accept_callback(
    callback: &str,
    redirect: &Url,
    state: &str,
    issuer: Option<&IssuerUrl>,
    host: &str,
) -> Result<AuthorizationCode, AccountError> {
    let refused = |why: &str| {
        tracing::warn!(%host, reason = why, "oidc callback refused");
        AccountError::Refused(format!(
            "The sign-in answer from {host} did not match the request keeper sent ({why})."
        ))
    };
    let url = Url::parse(callback).map_err(|_| refused("unreadable address"))?;
    let mut bare = url.clone();
    bare.set_query(None);
    bare.set_fragment(None);
    let mut expected = redirect.clone();
    expected.set_query(None);
    expected.set_fragment(None);
    if bare != expected {
        return Err(refused("wrong redirect address"));
    }

    let mut states = Vec::new();
    let mut codes = Vec::new();
    let mut iss = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "state" => states.push(value.into_owned()),
            "code" => codes.push(value.into_owned()),
            "iss" => iss = Some(value.into_owned()),
            "error" => return Err(refused("the server reported an error")),
            _ => {}
        }
    }
    if states.len() != 1 || states[0] != state {
        return Err(refused("state"));
    }
    if let (Some(got), Some(issuer)) = (iss, issuer) {
        if got != issuer.as_str() {
            return Err(refused("issuer"));
        }
    }
    match codes.as_slice() {
        [code] if !code.is_empty() => Ok(AuthorizationCode::new(code.clone())),
        _ => Err(refused("code")),
    }
}

/// Run one authorization-code + PKCE attempt through the browser and return
/// what the token exchange needs.
#[allow(clippy::too_many_arguments)]
async fn authorize(
    platform: &dyn Platform,
    flows: &OAuthFlowRegistry,
    provider: &Provider,
    redirect: &str,
    scopes: &[String],
    signin_url: Option<&str>,
    wait: Duration,
) -> Result<Grant, AccountError> {
    let redirect = Url::parse(redirect).map_err(|_| {
        AccountError::Refused("The account's redirect address is not a URL.".to_owned())
    })?;
    let mut listener = if loopback::is_loopback(&redirect) {
        Some(Listener::bind(&redirect)?)
    } else {
        None
    };
    let redirect = listener
        .as_ref()
        .map_or_else(|| redirect.clone(), |l| l.redirect_uri().clone());
    let redirect_url = RedirectUrl::from_url(redirect.clone());

    let client = provider.client(Some(redirect_url.clone()));
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let mut request = client.authorize_url(
        CoreAuthenticationFlow::AuthorizationCode,
        CsrfToken::new_random,
        Nonce::new_random,
    );
    for scope in scopes {
        request = request.add_scope(Scope::new(scope.clone()));
    }
    let (auth_url, state, nonce) = request.set_pkce_challenge(challenge).url();
    let open = match signin_url {
        Some(template) => fill_signin_url(template, &auth_url),
        None => auth_url.to_string(),
    };

    // Registered before the browser opens, so a fast callback cannot race it.
    let (rx, mut unshown) = flows.register_with_link(state.secret().clone());
    let _guard = FlowGuard {
        flows,
        state: state.secret(),
    };
    // The link goes to the person first, then to the browser; a page that
    // could not be opened — at once, or a sign-in sheet refused after it was
    // queued — leaves it on screen with a sentence, and the sign-in keeps
    // waiting for the callback the pasted link brings (R470, R480).
    let loopback = listener.is_some();
    let shown = crate::sign_in_link::offer(platform, &open, || {
        if loopback {
            platform.open_url(&open)
        } else {
            platform.start_web_auth(&open, redirect.scheme())
        }
    });

    let outcome = tokio::time::timeout(
        wait,
        shown.wait(&mut unshown, async {
            let mut rx = rx;
            let mut loopback_done = listener.is_none();
            loop {
                let active = !loopback_done;
                let next = async {
                    match listener.as_mut() {
                        Some(l) if active => l.next().await,
                        _ => std::future::pending().await,
                    }
                };
                tokio::select! {
                    outcome = &mut rx => break outcome,
                    url = next => match url {
                        // Routed like a deep link: the registry matches it by
                        // state, and anything else is ignored.
                        Some(url) => { flows.resolve(&url); }
                        None => loopback_done = true,
                    },
                }
            }
        }),
    )
    .await;
    // Whatever came back, the link is spent: its `state` left the registry.
    drop(shown);

    let callback = match outcome {
        Err(_) => return Err(AccountError::TimedOut),
        Ok(Err(_)) => {
            return Err(AccountError::Interrupted(
                "the sign-in ended without an answer".to_owned(),
            ))
        }
        Ok(Ok(OAuthCallback::Cancelled)) => return Err(AccountError::Cancelled),
        Ok(Ok(OAuthCallback::Failed(reason))) => {
            return Err(AccountError::Interrupted(
                provider_words(&reason).unwrap_or_else(|| "no reason was given".to_owned()),
            ))
        }
        Ok(Ok(OAuthCallback::Error { error, description })) if error == "access_denied" => {
            return Err(AccountError::Denied {
                host: provider.host.clone(),
                reason: description.as_deref().and_then(provider_words),
            })
        }
        Ok(Ok(OAuthCallback::Error { error, .. })) => {
            return Err(AccountError::Refused(format!(
                "{} refused the sign-in ({error}).",
                provider.host
            )))
        }
        Ok(Ok(OAuthCallback::Redirect(url))) => url,
    };
    let code = accept_callback(
        &callback,
        &redirect,
        state.secret(),
        provider.oidc.as_ref().map(|o| &o.issuer),
        &provider.host,
    )?;
    Ok(Grant {
        code,
        verifier,
        nonce,
        redirect: redirect_url,
    })
}

/// The longest piece of a provider's or platform's own words a sentence
/// quotes. An `error_description` is the provider's text, shown to a person:
/// bounded, on one line, with no control characters.
const PROVIDER_WORDS_MAX: usize = 200;

/// `raw` as words a sentence can quote, or `None` when nothing is left.
fn provider_words(raw: &str) -> Option<String> {
    let words: String = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let words = words.trim_end_matches(['.', ' ']);
    if words.is_empty() {
        return None;
    }
    let mut bounded: String = words.chars().take(PROVIDER_WORDS_MAX).collect();
    if words.chars().count() > PROVIDER_WORDS_MAX {
        bounded.push('…');
    }
    Some(bounded)
}

type TokenError = RequestTokenError<
    openidconnect::HttpClientError<reqwest::Error>,
    openidconnect::StandardErrorResponse<CoreErrorResponseType>,
>;

fn exchange_error(host: &str, error: TokenError) -> AccountError {
    match error {
        RequestTokenError::Request(_) => {
            AccountError::Unreachable(format!("keeper could not reach {host}."))
        }
        RequestTokenError::ServerResponse(response) => AccountError::Refused(format!(
            "{host} refused the sign-in ({}).",
            response.error()
        )),
        other => {
            tracing::warn!(%host, error = %other, "oidc token exchange failed");
            AccountError::Refused(format!(
                "{host} answered the sign-in with something keeper could not read."
            ))
        }
    }
}

/// A refresh the provider rejected is a dead grant; anything else is the
/// network or the server having a bad minute, which must not sign anyone out.
fn refresh_error(host: &str, error: TokenError, dead: &str) -> AccountError {
    match error {
        RequestTokenError::ServerResponse(_) => AccountError::NeedsSignIn(dead.to_owned()),
        RequestTokenError::Request(_) => {
            AccountError::Unreachable(format!("keeper could not reach {host}."))
        }
        other => {
            tracing::warn!(%host, error = %other, "oidc refresh failed");
            AccountError::Unreachable(format!("{host} did not answer the token refresh."))
        }
    }
}

async fn exchange(
    http: &ReqwestClient,
    provider: &Provider,
    grant: Grant,
) -> Result<CoreTokenResponse, AccountError> {
    provider
        .client(Some(grant.redirect))
        .exchange_code(grant.code)
        .set_pkce_verifier(grant.verifier)
        .request_async(http)
        .await
        .map_err(|e| exchange_error(&provider.host, e))
}

/// Which `nonce` an ID token must carry.
enum NonceRule<'a> {
    /// A sign-in: exactly the attempt's nonce.
    Exact(&'a Nonce),
    /// A refresh: absent, or equal to the original sign-in's.
    AbsentOrOriginal(Option<String>),
}

enum VerifyFailure {
    UnknownKey,
    Refused(String),
}

/// The claims of a verified ID token as JSON, for the claim rules.
fn verify_once(
    oidc: &OidcMeta,
    client_id: &ClientId,
    id_token: &CoreIdToken,
    nonce: &NonceRule<'_>,
    access_token: Option<&AccessToken>,
) -> Result<serde_json::Value, VerifyFailure> {
    let trusted = oidc.trusted_audiences.clone();
    let verifier = CoreIdTokenVerifier::new_public_client(
        client_id.clone(),
        oidc.issuer.clone(),
        oidc.jwks.clone(),
    )
    .set_allowed_algs(oidc.algs.clone())
    .set_other_audience_verifier_fn(move |aud| trusted.iter().any(|t| t == aud.as_str()))
    .set_time_fn(|| Utc::now() - LEEWAY)
    .set_issue_time_verifier_fn(|iat| {
        let now = Utc::now();
        if iat > now + LEEWAY {
            Err(format!("issued in the future ({iat})"))
        } else if iat < now - IAT_MAX_AGE - LEEWAY {
            Err(format!("issued too long ago ({iat})"))
        } else {
            Ok(())
        }
    });

    let verified = match nonce {
        NonceRule::Exact(expected) => id_token.claims(&verifier, *expected),
        NonceRule::AbsentOrOriginal(original) => {
            id_token.claims(&verifier, |got: Option<&Nonce>| {
                match (got, original.as_deref()) {
                    (None, _) => Ok(()),
                    (Some(got), Some(original)) if got.secret() == original => Ok(()),
                    _ => Err("nonce mismatch".to_owned()),
                }
            })
        }
    };
    let claims = verified.map_err(|e| match e {
        ClaimsVerificationError::SignatureVerification(
            SignatureVerificationError::NoMatchingKey,
        ) => VerifyFailure::UnknownKey,
        other => VerifyFailure::Refused(other.to_string()),
    })?;

    if let Some(azp) = claims.authorized_party() {
        if azp != client_id {
            return Err(VerifyFailure::Refused("azp is another client".to_owned()));
        }
    }
    if let (Some(expected), Some(access_token)) = (claims.access_token_hash(), access_token) {
        let alg = id_token
            .signing_alg()
            .map_err(|e| VerifyFailure::Refused(e.to_string()))?;
        let key = id_token
            .signing_key(&verifier)
            .map_err(|e| VerifyFailure::Refused(e.to_string()))?;
        let actual = AccessTokenHash::from_token(access_token, alg, key)
            .map_err(|e| VerifyFailure::Refused(e.to_string()))?;
        if &actual != expected {
            return Err(VerifyFailure::Refused(
                "at_hash does not match the access token".to_owned(),
            ));
        }
    }
    payload(&id_token.to_string())
        .ok_or_else(|| VerifyFailure::Refused("unreadable payload".to_owned()))
}

/// The payload of a compact JWT, decoded. Only ever called on a token whose
/// signature was just verified, or on our own stored one.
fn payload(jwt: &str) -> Option<serde_json::Value> {
    let part = jwt.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(part.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Verify an ID token, refetching the JWKS once when its `kid` is unknown
/// (providers publish a key before signing with it, so a key set fetched
/// earlier can simply be older than the token).
async fn verify(
    http: &ReqwestClient,
    provider: &mut Provider,
    id_token: &CoreIdToken,
    nonce: NonceRule<'_>,
    access_token: Option<&AccessToken>,
) -> Result<serde_json::Value, AccountError> {
    let host = provider.host.clone();
    let refused = |why: String| {
        tracing::warn!(%host, reason = %why, "id token refused");
        AccountError::Refused(format!(
            "{host} returned a sign-in keeper could not verify."
        ))
    };
    let Some(oidc) = provider.oidc.as_mut() else {
        return Err(refused("no issuer to verify against".to_owned()));
    };
    match verify_once(oidc, &provider.client_id, id_token, &nonce, access_token) {
        Ok(claims) => Ok(claims),
        Err(VerifyFailure::Refused(why)) => Err(refused(why)),
        Err(VerifyFailure::UnknownKey) => {
            oidc.jwks = fetch_jwks(
                &Discoverer::background(http),
                &provider.host,
                &oidc.jwks_url,
            )
            .await?;
            match verify_once(oidc, &provider.client_id, id_token, &nonce, access_token) {
                Ok(claims) => Ok(claims),
                Err(VerifyFailure::UnknownKey) => {
                    Err(refused("no key matches the token".to_owned()))
                }
                Err(VerifyFailure::Refused(why)) => Err(refused(why)),
            }
        }
    }
}

/// GET a JSON document with the bearer token.
async fn get_json(
    http: &reqwest::Client,
    url: &str,
    token: &str,
    host: &str,
) -> Result<serde_json::Value, AccountError> {
    let response = http
        .get(url)
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| AccountError::Unreachable(format!("keeper could not reach {host}.")))?;
    if !response.status().is_success() {
        return Err(AccountError::Refused(format!(
            "{host} answered {} when keeper asked who you are.",
            response.status()
        )));
    }
    response.json().await.map_err(|_| {
        AccountError::Refused(format!(
            "{host} answered with something keeper could not read."
        ))
    })
}

/// Fill claims the ID token lacks from UserInfo, when the provider has one.
/// A UserInfo answer about a different `sub` is discarded as a whole.
async fn merge_userinfo(
    http: &reqwest::Client,
    oidc: &OidcMeta,
    host: &str,
    access_token: &str,
    claims: &mut serde_json::Value,
) -> Result<(), AccountError> {
    let Some(url) = &oidc.userinfo_url else {
        return Ok(());
    };
    let info = get_json(http, url.as_str(), access_token, host).await?;
    if info.get("sub") != claims.get("sub") {
        tracing::warn!(%host, "userinfo sub differs from the id token; discarded");
        return Err(AccountError::Refused(format!(
            "{host} described a different person than the one who signed in."
        )));
    }
    if let (Some(target), serde_json::Value::Object(info)) = (claims.as_object_mut(), info) {
        for (key, value) in info {
            target.entry(key).or_insert(value);
        }
    }
    Ok(())
}

fn string_claim(claims: &serde_json::Value, name: &str) -> Option<String> {
    claims
        .get(name)
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Sign in to the account's identity provider through the platform's auth
/// session and persist the session before returning who signed in.
pub async fn sign_in(
    platform: &dyn Platform,
    flows: &OAuthFlowRegistry,
    http: &reqwest::Client,
    d: &AccountDescriptor,
) -> Result<Identity, AccountError> {
    sign_in_within(platform, flows, http, d, FLOW_TIMEOUT).await
}

async fn sign_in_within(
    platform: &dyn Platform,
    flows: &OAuthFlowRegistry,
    http: &reqwest::Client,
    d: &AccountDescriptor,
    wait: Duration,
) -> Result<Identity, AccountError> {
    let oauth_http = ReqwestClient::from(http.clone());
    let mut provider = account_provider(&Discoverer::sign_in(&oauth_http), d).await?;
    let scopes: Vec<String> = d
        .auth
        .scopes
        .iter()
        .chain(&d.auth.extra_scopes)
        .cloned()
        .collect();
    let grant = authorize(
        platform,
        flows,
        &provider,
        &d.redirect_uri(),
        &scopes,
        None,
        wait,
    )
    .await?;
    let nonce = grant.nonce.clone();
    let tokens = exchange(&oauth_http, &provider, grant).await?;
    let id_token = tokens.id_token().ok_or_else(|| {
        AccountError::Refused(format!(
            "{} did not return an ID token; the account must request the openid scope.",
            provider.host
        ))
    })?;
    let mut claims = verify(
        &oauth_http,
        &mut provider,
        id_token,
        NonceRule::Exact(&nonce),
        Some(tokens.access_token()),
    )
    .await?;
    if let Some(oidc) = &provider.oidc {
        merge_userinfo(
            http,
            oidc,
            &provider.host,
            tokens.access_token().secret(),
            &mut claims,
        )
        .await?;
    }

    let login = claims::username(&claims, &d.auth.username_claim)?;
    let roles = d
        .auth
        .roles_claim
        .as_deref()
        .map(|claim| claims::roles(&claims, claim))
        .unwrap_or_default();
    claims::require_role(&roles, d.auth.required_role.as_deref(), &d.name)?;

    let session = StoredSession {
        binding: Binding::session(d),
        iss: string_claim(&claims, "iss").unwrap_or_else(|| d.auth.issuer.clone()),
        sub: string_claim(&claims, "sub")
            .ok_or_else(|| AccountError::Refused(format!("{} named nobody.", provider.host)))?,
        refresh_token: tokens.refresh_token().map(|t| t.secret().clone()),
        access_token: tokens.access_token().secret().clone(),
        access_expires_ms: expiry_ms(tokens.expires_in()),
        id_token: id_token.to_string(),
        display_name: string_claim(&claims, "name").unwrap_or_else(|| login.clone()),
        email: string_claim(&claims, "email"),
        login,
        roles,
        sign_in: Some(ulid::Ulid::new().to_string()),
    };
    {
        let lock = refresh_lock(&session_key(&d.id));
        let _held = lock.lock().await;
        // The forge token belongs to whoever signed in before. Another person
        // (or an item nobody can vouch for) must connect the forge anew, or
        // their directory is fetched and published with someone else's token.
        let previous = load_session(platform, &d.id).ok().flatten();
        let same_person = previous.is_some_and(|p| p.iss == session.iss && p.sub == session.sub);
        if !same_person {
            let forge_lock = refresh_lock(&forge_key(&d.id));
            let _forge_held = forge_lock.lock().await;
            session::forget_forge(platform, &d.id)?;
        }
        store_session(platform, &d.id, &session)?;
    }
    tracing::info!(account = %d.id, "account signed in");
    Ok(session.identity())
}

/// How long a token whose response carried no `expires_in` is trusted before
/// keeper refreshes it. Without a lifetime it would never be refreshed, and
/// the first `401` would ask the person to sign in although the refresh token
/// was still good.
const DEFAULT_LIFETIME: Duration = Duration::from_secs(10 * 60);

fn expiry_ms(expires_in: Option<Duration>) -> Option<i64> {
    let lifetime = expires_in.unwrap_or(DEFAULT_LIFETIME);
    Some(now_ms().saturating_add(i64::try_from(lifetime.as_millis()).unwrap_or(i64::MAX)))
}

fn still_fresh(expires_ms: Option<i64>) -> bool {
    expires_ms.is_some_and(|at| at - REFRESH_EARLY_MS > now_ms())
}

/// Connect the config repository's forge (`oauth` mode): a PKCE flow against
/// the forge in the same browser session. The forge must sign the person in
/// as `expected_login`, or its tokens are discarded.
///
/// Forgejo and Gitea keep **one** grant per person and OAuth application,
/// with a counter every sign-in and every refresh advances: a token issued to
/// one device retires every other device's refresh token ("refresh token for
/// grant_id = 2 was used twice!"), and the person's devices take turns being
/// signed out of the repository (hesperia and kalypso, 2026-10-05 and
/// 2026-10-18). So the first flow, under the descriptor's shared client, is
/// a bootstrap: its token registers this device's own application (named
/// after `device`, so the person can tell it apart and revoke it in the
/// forge's settings), and a second flow under that application yields the
/// stored credential — a grant no other device's sign-in touches.
///
/// A forge that registers no application (no `api_base`, a scope without
/// `write:user`, a forge without the endpoint) keeps the shared client's
/// token, as before per-device applications.
pub async fn forge_connect(
    platform: &dyn Platform,
    flows: &OAuthFlowRegistry,
    http: &reqwest::Client,
    d: &AccountDescriptor,
    expected_login: &str,
    device: &str,
) -> Result<(), AccountError> {
    forge_connect_within(
        platform,
        flows,
        http,
        d,
        expected_login,
        device,
        FLOW_TIMEOUT,
    )
    .await
}

async fn forge_connect_within(
    platform: &dyn Platform,
    flows: &OAuthFlowRegistry,
    http: &reqwest::Client,
    d: &AccountDescriptor,
    expected_login: &str,
    device: &str,
    wait: Duration,
) -> Result<(), AccountError> {
    let RepoAuthConfig::Oauth(forge) = &d.config.auth else {
        return Err(AccountError::Internal(
            "the repository does not use a forge sign-in".to_owned(),
        ));
    };
    let redirect = d.forge_redirect_uri().ok_or_else(|| {
        AccountError::Internal("the repository has no forge redirect address".to_owned())
    })?;
    let oauth_http = ReqwestClient::from(http.clone());
    let mut provider = forge_provider(&Discoverer::sign_in(&oauth_http), forge).await?;
    let leg = ForgeLeg {
        platform,
        flows,
        http,
        oauth_http: &oauth_http,
        d,
        forge,
        redirect: &redirect,
        expected_login,
        wait,
    };
    // One scope string, byte-identical on every device and version: Forgejo
    // refuses an authorization whose scope differs from the shared grant's.
    let (mut tokens, mut login) = leg.authorize(&mut provider, &forge.scope).await?;

    let mut device_app = None;
    if let Some(api_base) = d.config.api_base.as_deref() {
        let bootstrap = tokens.access_token().secret().clone();
        let name = device_app_name(device);
        match register_device_app(http, api_base, &bootstrap, &name, &redirect).await {
            Ok(app) => {
                provider.client_id = ClientId::new(app.client_id.clone());
                match leg
                    .authorize(&mut provider, &device_scope(&forge.scope))
                    .await
                {
                    Ok(answer) => (tokens, login) = answer,
                    Err(error) => {
                        // Nothing will ever use it: take it away again while
                        // the bootstrap token can.
                        delete_device_app(http, api_base, &bootstrap, &app, &provider.host).await;
                        return Err(error);
                    }
                }
                tracing::info!(account = %d.id, app = %app.name, "account: this device signed in to the forge under its own application");
                device_app = Some(app);
            }
            Err(why) => tracing::warn!(
                account = %d.id,
                host = %provider.host,
                %why,
                "account: the forge registered no application for this device; it keeps the shared client, whose refresh token another device's sign-in ends"
            ),
        }
    }

    let stored = StoredForge {
        binding: Binding::forge(d, forge),
        access_token: tokens.access_token().secret().clone(),
        refresh_token: tokens.refresh_token().map(|t| t.secret().clone()),
        expires_ms: expiry_ms(tokens.expires_in()),
        login,
        device_app,
    };
    let lock = refresh_lock(&forge_key(&d.id));
    let _held = lock.lock().await;
    store_forge(platform, &d.id, &stored)
}

/// What every authorization of one forge connection shares.
struct ForgeLeg<'a> {
    platform: &'a dyn Platform,
    flows: &'a OAuthFlowRegistry,
    http: &'a reqwest::Client,
    oauth_http: &'a ReqwestClient,
    d: &'a AccountDescriptor,
    forge: &'a ForgeOauth,
    redirect: &'a str,
    expected_login: &'a str,
    wait: Duration,
}

impl ForgeLeg<'_> {
    /// One authorization under `provider`'s client, through `signin_url`,
    /// and the forge login it signed in — refused unless it is
    /// `expected_login`.
    async fn authorize(
        &self,
        provider: &mut Provider,
        scope: &str,
    ) -> Result<(CoreTokenResponse, String), AccountError> {
        let grant = authorize(
            self.platform,
            self.flows,
            provider,
            self.redirect,
            &[scope.to_owned()],
            self.forge.signin_url.as_deref(),
            self.wait,
        )
        .await?;
        let nonce = grant.nonce.clone();
        let tokens = exchange(self.oauth_http, provider, grant).await?;

        let mut login = None;
        if let (Some(_), Some(id_token)) = (&provider.oidc, tokens.id_token()) {
            let claims = verify(
                self.oauth_http,
                provider,
                id_token,
                NonceRule::Exact(&nonce),
                Some(tokens.access_token()),
            )
            .await?;
            login = string_claim(&claims, "preferred_username");
        }
        let login = match login {
            Some(login) => login,
            None => {
                forge_user_login(
                    self.http,
                    self.d,
                    self.forge,
                    tokens.access_token().secret(),
                    &provider.host,
                )
                .await?
            }
        };
        if login != self.expected_login {
            return Err(AccountError::Refused(format!(
                "The forge signed you in as {login}, but your keeper account is {}.",
                self.expected_login
            )));
        }
        Ok((tokens, login))
    }
}

/// What the person sees in the forge's list of applications: keeper, the
/// device, and the day, so an application a reset device left behind can be
/// told from the live one.
fn device_app_name(device: &str) -> String {
    format!("keeper ({device}, {})", Utc::now().format("%Y-%m-%d"))
}

/// The scope this device's own application asks for: the descriptor's, with
/// `write:user` — needed only by the bootstrap token that registered the
/// application — narrowed to `read:user`. Kept as written when `write:user`
/// is its only API scope: dropping it would leave Forgejo's "no API scope",
/// which is every scope.
fn device_scope(scope: &str) -> String {
    const OIDC: [&str; 4] = ["openid", "profile", "email", "groups"];
    let words: Vec<&str> = scope.split_whitespace().collect();
    let other_api = words
        .iter()
        .any(|w| *w != "write:user" && !OIDC.contains(w));
    if !words.contains(&"write:user") || !other_api {
        return scope.to_owned();
    }
    let mut narrowed: Vec<&str> = Vec::with_capacity(words.len());
    for word in words {
        let word = if word == "write:user" {
            "read:user"
        } else {
            word
        };
        if !narrowed.contains(&word) {
            narrowed.push(word);
        }
    }
    narrowed.join(" ")
}

/// The forge's answer to an application registration; its `client_secret`
/// is never read.
#[derive(Deserialize)]
struct RegisteredApp {
    id: i64,
    client_id: String,
}

/// Register this device's application on the forge
/// (`POST {api_base}/user/applications/oauth2`, Forgejo and Gitea): a public
/// client on the descriptor's forge redirect. `api_base` is on the
/// repository's host or its forge's sign-in host (the descriptor rule), so
/// the token goes nowhere else. The `Err` is a reason for the log.
async fn register_device_app(
    http: &reqwest::Client,
    api_base: &str,
    token: &str,
    name: &str,
    redirect: &str,
) -> Result<DeviceApp, String> {
    let url = format!(
        "{}/user/applications/oauth2",
        api_base.trim_end_matches('/')
    );
    let body = serde_json::json!({
        "name": name,
        "redirect_uris": [redirect],
        "confidential_client": false,
    });
    let response = http
        .post(&url)
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|_| "the forge could not be reached".to_owned())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "the forge answered {status} (an OAuth token registers an application only with write:user in config.auth.scope)"
        ));
    }
    let app: RegisteredApp = response
        .json()
        .await
        .map_err(|_| "the forge's answer could not be read".to_owned())?;
    if app.client_id.trim().is_empty() {
        return Err("the forge's answer carried no client_id".to_owned());
    }
    Ok(DeviceApp {
        id: app.id,
        client_id: app.client_id,
        name: name.to_owned(),
    })
}

/// Delete an application this device registered and will not use. Best
/// effort: an application left behind holds no token anyone has.
async fn delete_device_app(
    http: &reqwest::Client,
    api_base: &str,
    token: &str,
    app: &DeviceApp,
    host: &str,
) {
    let url = format!(
        "{}/user/applications/oauth2/{}",
        api_base.trim_end_matches('/'),
        app.id
    );
    match http.delete(&url).bearer_auth(token).send().await {
        Ok(response) if response.status().is_success() => {}
        Ok(response) => {
            tracing::warn!(%host, app = %app.name, status = %response.status(), "account: the forge kept an application this device does not use")
        }
        Err(error) => {
            tracing::warn!(%host, app = %app.name, %error, "account: the forge kept an application this device does not use")
        }
    }
}

/// The forge username from `user_url` (default `{api_base}/user`).
async fn forge_user_login(
    http: &reqwest::Client,
    d: &AccountDescriptor,
    forge: &ForgeOauth,
    token: &str,
    host: &str,
) -> Result<String, AccountError> {
    let template = forge.user_url.as_deref().unwrap_or("{api_base}/user");
    let url = if template.contains("{api_base}") {
        let api_base = d.config.api_base.as_deref().ok_or_else(|| {
            AccountError::Refused(
                "The repository names no api_base, so keeper cannot ask the forge who you are."
                    .to_owned(),
            )
        })?;
        template.replace("{api_base}", api_base.trim_end_matches('/'))
    } else {
        template.to_owned()
    };
    let user = get_json(http, &url, token, host).await?;
    string_claim(&user, &forge.username_field).ok_or_else(|| {
        AccountError::Refused(format!(
            "{host} did not say which user you are ({} is missing).",
            forge.username_field
        ))
    })
}

/// A valid access token for the account, refreshed 60 s before it expires.
///
/// Single-flight per account: concurrent callers wait on one refresh and all
/// receive its token. A rotated refresh token is written to the keychain the
/// moment the provider returns it — before the refreshed ID token is checked —
/// because the provider has already retired the old one. A session issued
/// under another sign-in setup is deleted ([`session::identity`]'s rule), and
/// the descriptor's `required_role` is checked against the stored roles on
/// every call and re-derived from every refreshed ID token.
pub async fn access_token(
    platform: &dyn Platform,
    http: &reqwest::Client,
    d: &AccountDescriptor,
) -> Result<String, AccountError> {
    access_token_or_minted(platform, http, d, false).await
}

/// A newly minted access token: always refreshed, never the stored one or
/// one another caller received. For a server that takes a token once, as a
/// fresh proof of sign-in, and spends its `jti` (the agents server's
/// enrolment and attestation, 109.6): a token reused there is a replay.
/// Concurrent callers each get their own, one after the other.
pub async fn minted_access_token(
    platform: &dyn Platform,
    http: &reqwest::Client,
    d: &AccountDescriptor,
) -> Result<String, AccountError> {
    access_token_or_minted(platform, http, d, true).await
}

async fn access_token_or_minted(
    platform: &dyn Platform,
    http: &reqwest::Client,
    d: &AccountDescriptor,
    mint: bool,
) -> Result<String, AccountError> {
    let needs_sign_in =
        || AccountError::NeedsSignIn("Sign in again to keep your settings in sync.".to_owned());
    if !mint {
        if let Some(session) = load_bound_session(platform, d)? {
            if still_fresh(session.access_expires_ms) {
                session.check_roles(d)?;
                return Ok(session.access_token);
            }
        }
    }
    let lock = refresh_lock(&session_key(&d.id));
    let _held = lock.lock().await;
    // Re-read under the lock: the caller before us may have just refreshed.
    let mut session = load_bound_session(platform, d)?.ok_or_else(needs_sign_in)?;
    session.check_roles(d)?;
    if !mint && still_fresh(session.access_expires_ms) {
        return Ok(session.access_token);
    }
    let refresh = session.refresh_token.clone().ok_or_else(needs_sign_in)?;

    let oauth_http = ReqwestClient::from(http.clone());
    let mut provider = account_provider(&Discoverer::background(&oauth_http), d).await?;
    let tokens = provider
        .client(None)
        .exchange_refresh_token(&RefreshToken::new(refresh))
        .request_async(&oauth_http)
        .await
        .map_err(|e| {
            refresh_error(
                &provider.host,
                e,
                "Sign in again to keep your settings in sync.",
            )
        })?;
    if let Some(rotated) = tokens.refresh_token() {
        session.refresh_token = Some(rotated.secret().clone());
        store_session(platform, &d.id, &session)?;
    }

    if let Some(id_token) = tokens.id_token() {
        session.roles = refreshed_roles(
            &oauth_http,
            http,
            &mut provider,
            d,
            &session,
            id_token,
            tokens.access_token(),
        )
        .await?;
        session.id_token = id_token.to_string();
    }
    session.access_token = tokens.access_token().secret().clone();
    session.access_expires_ms = expiry_ms(tokens.expires_in());
    store_session(platform, &d.id, &session)?;
    // Stored first, so a restore sees the roles the provider now reports.
    session.check_roles(d)?;
    Ok(session.access_token)
}

/// The rules for an ID token returned by a refresh (OIDC Core §12.2): the
/// same `iss`, `sub` and `aud` as the sign-in's, the original `auth_time`, a
/// nonce absent or the original one, and a username that still names the
/// same directory. Returns the roles it now carries (from UserInfo when the
/// token lacks the claim).
///
/// A token that fails verification is a sign-in keeper can no longer vouch
/// for (`NeedsSignIn`); a key set that cannot be fetched stays `Unreachable`.
async fn refreshed_roles(
    oauth_http: &ReqwestClient,
    http: &reqwest::Client,
    provider: &mut Provider,
    d: &AccountDescriptor,
    session: &StoredSession,
    id_token: &CoreIdToken,
    access_token: &AccessToken,
) -> Result<Vec<String>, AccountError> {
    let changed = || {
        AccountError::NeedsSignIn(
            "The sign-in server now names a different person. Sign in again.".to_owned(),
        )
    };
    let original = payload(&session.id_token).ok_or_else(changed)?;
    let mut claims = verify(
        oauth_http,
        provider,
        id_token,
        NonceRule::AbsentOrOriginal(string_claim(&original, "nonce")),
        Some(access_token),
    )
    .await
    .map_err(|error| match error {
        AccountError::Refused(_) => AccountError::NeedsSignIn(
            "keeper could not verify the sign-in server's answer. Sign in again.".to_owned(),
        ),
        other => other,
    })?;
    let audiences = |claims: &serde_json::Value| {
        let mut aud: Vec<String> = match claims.get("aud") {
            Some(serde_json::Value::String(one)) => vec![one.clone()],
            Some(serde_json::Value::Array(many)) => many
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect(),
            _ => Vec::new(),
        };
        aud.sort();
        aud.dedup();
        aud
    };
    let auth_time_changed = match (claims.get("auth_time"), original.get("auth_time")) {
        (Some(now), Some(then)) => now != then,
        _ => false,
    };
    if string_claim(&claims, "sub").as_deref() != Some(session.sub.as_str())
        || string_claim(&claims, "iss").as_deref() != Some(session.iss.as_str())
        || audiences(&claims) != audiences(&original)
        || auth_time_changed
    {
        return Err(changed());
    }

    let roles_claim = d.auth.roles_claim.as_deref();
    let lacks = |claims: &serde_json::Value| {
        claims.get(&d.auth.username_claim).is_none()
            || roles_claim.is_some_and(|c| claims::roles(claims, c).is_empty())
    };
    if lacks(&claims) {
        if let Some(oidc) = &provider.oidc {
            merge_userinfo(
                http,
                oidc,
                &provider.host,
                access_token.secret(),
                &mut claims,
            )
            .await?;
        }
    }
    if claims.get(&d.auth.username_claim).is_some()
        && claims::username(&claims, &d.auth.username_claim)? != session.login
    {
        return Err(changed());
    }
    Ok(roles_claim
        .map(|claim| claims::roles(&claims, claim))
        .unwrap_or_default())
}

/// A valid forge token (`oauth` mode), refreshed like [`access_token`].
pub async fn forge_token(
    platform: &dyn Platform,
    http: &reqwest::Client,
    d: &AccountDescriptor,
) -> Result<String, AccountError> {
    forge_token_with(platform, http, d, false).await
}

/// A forge token refreshed now, however long the stored one claims to last:
/// the answer to a forge that rejected a token before its expiry.
pub async fn forge_token_refreshed(
    platform: &dyn Platform,
    http: &reqwest::Client,
    d: &AccountDescriptor,
) -> Result<String, AccountError> {
    forge_token_with(platform, http, d, true).await
}

async fn forge_token_with(
    platform: &dyn Platform,
    http: &reqwest::Client,
    d: &AccountDescriptor,
    force: bool,
) -> Result<String, AccountError> {
    let RepoAuthConfig::Oauth(forge) = &d.config.auth else {
        return Err(AccountError::Internal(
            "the repository does not use a forge sign-in".to_owned(),
        ));
    };
    let reconnect = || AccountError::NeedsSignIn("Reconnect the repository.".to_owned());
    if !force {
        if let Some(stored) = load_bound_forge(platform, d)? {
            if still_fresh(stored.expires_ms) {
                return Ok(stored.access_token);
            }
        }
    }
    let lock = refresh_lock(&forge_key(&d.id));
    let _held = lock.lock().await;
    let mut stored = load_bound_forge(platform, d)?.ok_or_else(reconnect)?;
    if !force && still_fresh(stored.expires_ms) {
        return Ok(stored.access_token);
    }
    let refresh = stored.refresh_token.clone().ok_or_else(reconnect)?;

    let oauth_http = ReqwestClient::from(http.clone());
    let mut provider = forge_provider(&Discoverer::background(&oauth_http), forge).await?;
    // Refreshed under the client the tokens were issued to: this device's
    // own application, or the shared one.
    provider.client_id = ClientId::new(stored.client_id(forge).to_owned());
    let tokens = match provider
        .client(None)
        .exchange_refresh_token(&RefreshToken::new(refresh))
        .request_async(&oauth_http)
        .await
    {
        Ok(tokens) => tokens,
        Err(error) => {
            // The forge itself refused the refresh token: it is dead, not
            // unreachable. Forgejo keeps one grant per person and app, so any
            // other device's sign-in kills this one ("refresh token ... was
            // used twice"). Kept in the keychain, the dead item read as
            // "connected", so the next sign-in skipped the forge leg and the
            // account asked to sign in again for ever — hesperia after
            // kalypso signed in, 2026-10-05 and again 2026-10-18. Forgotten,
            // the next sign-in connects the repository anew.
            let dead = matches!(error, RequestTokenError::ServerResponse(_));
            let answer = refresh_error(&provider.host, error, "Reconnect the repository.");
            if dead {
                tracing::warn!(
                    account = %d.id,
                    "account: the forge refused its refresh token; forgetting it so the next sign-in reconnects the repository"
                );
                session::forget_forge(platform, &d.id)?;
            }
            return Err(answer);
        }
    };
    stored.access_token = tokens.access_token().secret().clone();
    stored.expires_ms = expiry_ms(tokens.expires_in());
    if let Some(rotated) = tokens.refresh_token() {
        stored.refresh_token = Some(rotated.secret().clone());
    }
    store_forge(platform, &d.id, &stored)?;
    Ok(stored.access_token)
}

/// Revoke a refresh token where the provider supports RFC 7009. Best effort:
/// the local items are deleted whatever this does.
///
/// A plain form POST rather than the library's `revoke_token`, which refuses
/// every non-`https` endpoint — including the loopback test providers the
/// descriptor rules accept. The same rule is kept here: `https`, or loopback.
async fn revoke(http: &reqwest::Client, provider: &Provider, refresh: &str) {
    let Some(url) = &provider.revocation_url else {
        return;
    };
    let url = url.url();
    if url.scheme() != "https" && !loopback::is_loopback(url) {
        tracing::warn!(host = %provider.host, "revocation endpoint is not https; skipped");
        return;
    }
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("token", refresh)
        .append_pair("token_type_hint", "refresh_token")
        .append_pair("client_id", provider.client_id.as_str())
        .finish();
    let sent = http
        .post(url.as_str())
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .send()
        .await;
    match sent {
        Ok(response) if response.status().is_success() => {}
        Ok(response) => {
            tracing::warn!(host = %provider.host, status = %response.status(), "refresh token revocation refused")
        }
        Err(error) => {
            tracing::warn!(host = %provider.host, %error, "refresh token revocation failed")
        }
    }
}

/// The provider's end-session request, for the shell to present with
/// [`Platform::start_web_auth`] — never the OS opener, whose argv would carry
/// the ID token hint.
#[derive(Clone, PartialEq, Eq)]
pub struct EndSession {
    pub url: String,
    /// The scheme of `post_logout_redirect_uri`, for the auth session.
    pub callback_scheme: String,
    /// The `state` the provider must return on the post-logout redirect.
    pub state: String,
}

impl std::fmt::Debug for EndSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The URL carries the ID token hint.
        f.debug_struct("EndSession")
            .field("callback_scheme", &self.callback_scheme)
            .finish_non_exhaustive()
    }
}

/// Sign out: revoke the refresh token(s), delete both keychain items — always,
/// whatever the network did — and return the provider's end-session request,
/// when it publishes one.
///
/// Only an item issued under `d`'s own sign-in setup is revoked at `d`'s
/// providers: an item left from another setup under the same account id is
/// deleted without sending its token anywhere.
pub async fn sign_out(
    platform: &dyn Platform,
    http: &reqwest::Client,
    d: &AccountDescriptor,
) -> Result<Option<EndSession>, AccountError> {
    let session_lock = refresh_lock(&session_key(&d.id));
    let forge_lock = refresh_lock(&forge_key(&d.id));
    let _session_held = session_lock.lock().await;
    let _forge_held = forge_lock.lock().await;

    // An unreadable or foreign item must not stop the deletes below.
    let stored = load_session(platform, &d.id)
        .ok()
        .flatten()
        .filter(|s| s.binding == Binding::session(d));
    let forge = match &d.config.auth {
        RepoAuthConfig::Oauth(forge_config) => load_forge(platform, &d.id)
            .ok()
            .flatten()
            .filter(|f| f.binding == Binding::forge(d, forge_config))
            .map(|f| (f, forge_config)),
        RepoAuthConfig::Same { .. } | RepoAuthConfig::None => None,
    };
    let network = best_effort(async {
        let oauth_http = ReqwestClient::from(http.clone());
        // Sign-out's network work has its own budget (`best_effort`).
        let discoverer = Discoverer::background(&oauth_http);
        let mut end_session = None;
        if let Some(stored) = &stored {
            match account_provider(&discoverer, d).await {
                Ok(provider) => {
                    if let Some(refresh) = &stored.refresh_token {
                        revoke(http, &provider, refresh).await;
                    }
                    end_session = end_session_request(&provider, d, stored);
                }
                Err(error) => tracing::warn!(account = %d.id, %error, "sign-out could not reach the provider"),
            }
        }
        if let Some((forge_item, forge_config)) = &forge {
            if let Some(refresh) = &forge_item.refresh_token {
                match forge_provider(&discoverer, forge_config).await {
                    Ok(mut provider) => {
                        provider.client_id =
                            ClientId::new(forge_item.client_id(forge_config).to_owned());
                        revoke(http, &provider, refresh).await;
                    }
                    Err(error) => tracing::warn!(account = %d.id, %error, "sign-out could not reach the forge"),
                }
            }
        }
        end_session
    })
    .await;

    session::delete_all(platform, &d.id)?;
    tracing::info!(account = %d.id, "account signed out");
    Ok(network.flatten())
}

async fn best_effort<T>(work: impl Future<Output = T>) -> Option<T> {
    match tokio::time::timeout(SIGN_OUT_NETWORK_BUDGET, work).await {
        Ok(value) => Some(value),
        Err(_) => {
            tracing::warn!("sign-out network calls timed out; deleting local tokens anyway");
            None
        }
    }
}

/// `keeper://oauth/<id>/signed-out`: where the provider sends the auth session
/// back once it has ended its own session (checklist G20).
fn post_logout_redirect(d: &AccountDescriptor) -> String {
    format!("keeper://oauth/{}/signed-out", d.id)
}

fn end_session_request(
    provider: &Provider,
    d: &AccountDescriptor,
    stored: &StoredSession,
) -> Option<EndSession> {
    let url = provider.oidc.as_ref()?.end_session_url.clone()?;
    let redirect = post_logout_redirect(d);
    let redirect_url = openidconnect::PostLogoutRedirectUrl::new(redirect.clone()).ok()?;
    let callback_scheme = Url::parse(&redirect).ok()?.scheme().to_owned();
    let state = CsrfToken::new_random();
    let mut request = LogoutRequest::from(url)
        .set_client_id(provider.client_id.clone())
        .set_post_logout_redirect_uri(redirect_url)
        .set_state(state.clone());
    if let Ok(hint) = stored.id_token.parse::<CoreIdToken>() {
        request = request.set_id_token_hint(&hint);
    }
    Some(EndSession {
        url: request.http_get_url().to_string(),
        callback_scheme,
        state: state.secret().clone(),
    })
}

#[cfg(test)]
mod tests {
    //! Against a fake provider: a `std::net` HTTP server on 127.0.0.1 that
    //! answers discovery, JWKS, token and revocation, and signs RS256 ID
    //! tokens with a key generated once per test run (`TEST_KEY`). No
    //! private key is checked in: `*.pem` is ignored and the pre-commit hook
    //! refuses one. The fake platform is
    //! the browser: it reads the authorization URL and calls back through the
    //! registry the way the shell's deep-link handler does.

    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use openidconnect::core::{CoreIdTokenClaims, CoreRsaPrivateSigningKey};
    use openidconnect::{
        Audience, EmptyAdditionalClaims, EndUserUsername, JsonWebKeyId, PrivateSigningKey,
        StandardClaims, SubjectIdentifier,
    };

    use super::*;
    use crate::error::CoreError;
    use crate::org_account::descriptor;
    use crate::vm::NotifyTarget;

    /// One RSA key per test binary, generated on first use. A key in the tree
    /// would be refused by the pre-commit secret scan, and `*.pem` is ignored.
    static TEST_KEY: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding};
        let key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 2048).expect("generate");
        key.to_pkcs1_pem(LineEnding::LF)
            .expect("encode")
            .to_string()
    });

    const CLIENT_ID: &str = "keeper";
    /// How long a test waits for something that must NOT happen (a sign-in
    /// nobody answers ends at this deadline).
    const SHORT: Duration = Duration::from_secs(2);
    /// How long a test waits for something that SHOULD happen: generous, so
    /// a loaded runner is slow, never a failure.
    const ARRIVES: Duration = Duration::from_secs(30);

    // ---- the fake provider ---------------------------------------------

    #[derive(Default)]
    struct Seen {
        /// Every POST /token body, decoded.
        token_requests: Vec<HashMap<String, String>>,
        revocations: Vec<HashMap<String, String>>,
        jwks_fetches: usize,
        /// The nonce the browser saw in the authorization request.
        nonce: Option<String>,
        /// The `client_id` and `scope` of the last authorization request.
        authorized: Option<(String, String)>,
        /// Forgejo's grants (`Behaviour::forgejo`): one per client, its
        /// counter and whether its scope carries `write:user`.
        grants: HashMap<String, (u32, bool)>,
        /// Every application registration's body, in order.
        apps: Vec<serde_json::Value>,
        /// Every application id deleted.
        deleted_apps: Vec<String>,
    }

    #[derive(Default, Clone)]
    struct Behaviour {
        /// Sign the ID token with this nonce instead of the requested one.
        nonce_override: Option<String>,
        /// Return this access token while the ID token's at_hash covers `access-1`.
        access_token_override: Option<String>,
        /// Serve the first JWKS under a stale `kid`.
        stale_jwks_first: bool,
        /// Answer refreshes `400 invalid_grant`.
        reject_refresh: bool,
        /// How long a refresh takes, so concurrent callers overlap.
        refresh_delay: Duration,
        /// The login the fake forge's `/api/v1/user` reports.
        forge_login: Option<String>,
        /// Answer discovery with this status line instead of the document.
        discovery_status: Option<&'static str>,
        /// Hold the actual discovery response until the interactive retry.
        discovery_hold: Option<Arc<DiscoveryHold>>,
        /// Replace one field of the discovery document.
        discovery_patch: Option<(&'static str, String)>,
        /// Return an ID token on refresh: these claims over the sign-in's,
        /// signed under `refresh_kid` (default `k1`).
        refresh_claims: Option<serde_json::Value>,
        refresh_kid: Option<&'static str>,
        /// Leave `expires_in` out of every token response.
        omit_expires_in: bool,
        /// Answer `/token` as Forgejo does: one grant per client, a counter
        /// every sign-in and refresh advances, and a refresh token from an
        /// earlier count refused ("was used twice"). Serve the application
        /// registration API to a token whose scope carries `write:user`.
        forgejo: bool,
    }

    struct DiscoveryHold {
        arrived: tokio::sync::Notify,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    /// The claims every fake ID token starts from.
    fn base_claims(issuer: &str) -> serde_json::Value {
        let now = Utc::now().timestamp();
        serde_json::json!({
            "iss": issuer,
            "aud": [CLIENT_ID],
            "sub": "sub-1",
            "iat": now,
            "exp": now + 3600,
            "auth_time": 1_000,
            "preferred_username": "tgorka",
        })
    }

    /// An RS256 JWT over `claims`, signed with the test key under `kid`.
    fn jwt(kid: &str, claims: &serde_json::Value) -> String {
        let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let header = serde_json::json!({ "alg": "RS256", "typ": "JWT", "kid": kid });
        let input = format!(
            "{}.{}",
            b64(header.to_string().as_bytes()),
            b64(claims.to_string().as_bytes())
        );
        let signature = signing_key(kid)
            .sign(
                &CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
                input.as_bytes(),
            )
            .expect("sign");
        format!("{input}.{}", b64(&signature))
    }

    fn merged(mut base: serde_json::Value, patch: &serde_json::Value) -> serde_json::Value {
        if let (Some(base), Some(patch)) = (base.as_object_mut(), patch.as_object()) {
            // A `null` drops the claim: the provider did not send it.
            for (key, value) in patch {
                if value.is_null() {
                    base.remove(key);
                } else {
                    base.insert(key.clone(), value.clone());
                }
            }
        }
        base
    }

    struct Fake {
        issuer: String,
        seen: Arc<Mutex<Seen>>,
    }

    fn signing_key(kid: &str) -> CoreRsaPrivateSigningKey {
        CoreRsaPrivateSigningKey::from_pem(&TEST_KEY, Some(JsonWebKeyId::new(kid.to_owned())))
            .expect("the generated test key parses")
    }

    fn form(body: &str) -> HashMap<String, String> {
        url::form_urlencoded::parse(body.as_bytes())
            .into_owned()
            .collect()
    }

    fn start_fake(behaviour: Behaviour) -> Fake {
        // The key is generated here, not inside the first discovery: an
        // unoptimised RSA generation on a loaded machine can outlast the
        // sign-in's reach budget (`REACH_TIMEOUT`).
        std::sync::LazyLock::force(&TEST_KEY);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake provider");
        let issuer = format!("http://{}", listener.local_addr().expect("addr"));
        let seen = Arc::new(Mutex::new(Seen::default()));
        let behaviour = Arc::new(Mutex::new(behaviour));
        let fake = Fake {
            issuer: issuer.clone(),
            seen: Arc::clone(&seen),
        };
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (issuer, seen, behaviour) =
                    (issuer.clone(), Arc::clone(&seen), Arc::clone(&behaviour));
                std::thread::spawn(move || serve(stream, &issuer, &seen, &behaviour));
            }
        });
        fake
    }

    /// Method, target, body and the head's `Authorization` value.
    fn read_request(stream: &mut TcpStream) -> Option<(String, String, String, String)> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let head_end = loop {
            let n = stream.read(&mut chunk).ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break at + 4;
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        let length = head
            .lines()
            .find_map(|l| {
                let (name, value) = l.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0);
        while buf.len() < head_end + length {
            let n = stream.read(&mut chunk).ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let mut first = head.lines().next()?.split(' ');
        let method = first.next()?.to_owned();
        let target = first.next()?.to_owned();
        let body = String::from_utf8_lossy(&buf[head_end..]).into_owned();
        let authorization = head
            .lines()
            .find_map(|l| {
                let (name, value) = l.split_once(':')?;
                name.eq_ignore_ascii_case("authorization")
                    .then(|| value.trim().to_owned())
            })
            .unwrap_or_default();
        Some((method, target, body, authorization))
    }

    fn reply(stream: &mut TcpStream, status: &str, body: &str) {
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        );
    }

    fn id_token(issuer: &str, nonce: &str, access_token: &str) -> String {
        let now = Utc::now();
        let claims = CoreIdTokenClaims::new(
            IssuerUrl::new(issuer.to_owned()).expect("issuer"),
            vec![Audience::new(CLIENT_ID.to_owned())],
            now + TimeDelta::hours(1),
            now,
            StandardClaims::new(SubjectIdentifier::new("sub-1".to_owned()))
                .set_preferred_username(Some(EndUserUsername::new("tgorka".to_owned()))),
            EmptyAdditionalClaims {},
        )
        .set_nonce(Some(Nonce::new(nonce.to_owned())));
        CoreIdToken::new(
            claims,
            &signing_key("k1"),
            CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
            Some(&AccessToken::new(access_token.to_owned())),
            None,
        )
        .expect("sign the id token")
        .to_string()
    }

    fn serve(
        mut stream: TcpStream,
        issuer: &str,
        seen: &Mutex<Seen>,
        behaviour: &Mutex<Behaviour>,
    ) {
        let Some((method, target, body, authorization)) = read_request(&mut stream) else {
            return;
        };
        let behaviour = behaviour.lock().expect("behaviour").clone();
        match (method.as_str(), target.as_str()) {
            ("GET", "/.well-known/openid-configuration") => {
                if let Some(hold) = &behaviour.discovery_hold {
                    hold.arrived.notify_one();
                    let _ = hold.release.lock().expect("discovery release").recv();
                }
                if let Some(status) = behaviour.discovery_status {
                    reply(&mut stream, status, "{}");
                    return;
                }
                let mut doc = serde_json::json!({
                    "issuer": issuer,
                    "authorization_endpoint": format!("{issuer}/authorize"),
                    "token_endpoint": format!("{issuer}/token"),
                    "jwks_uri": format!("{issuer}/jwks"),
                    "revocation_endpoint": format!("{issuer}/revoke"),
                    "end_session_endpoint": format!("{issuer}/logout"),
                    "response_types_supported": ["code"],
                    "subject_types_supported": ["public"],
                    "id_token_signing_alg_values_supported": ["RS256", "HS256"],
                });
                if let Some((field, value)) = behaviour.discovery_patch {
                    doc[field] = value.into();
                }
                reply(&mut stream, "200 OK", &doc.to_string());
            }
            ("GET", "/jwks") => {
                let first = {
                    let mut seen = seen.lock().expect("seen");
                    seen.jwks_fetches += 1;
                    seen.jwks_fetches == 1
                };
                let kid = if behaviour.stale_jwks_first && first {
                    "old"
                } else {
                    "k1"
                };
                let jwk =
                    serde_json::to_value(signing_key(kid).as_verification_key()).expect("jwk");
                reply(
                    &mut stream,
                    "200 OK",
                    &serde_json::json!({ "keys": [jwk] }).to_string(),
                );
            }
            ("POST", "/token") if behaviour.forgejo => {
                let params = form(&body);
                let (status, answer) = forgejo_token(&params, seen);
                seen.lock().expect("seen").token_requests.push(params);
                reply(&mut stream, status, &answer);
            }
            ("POST", "/api/v1/user/applications/oauth2") if behaviour.forgejo => {
                // An OAuth token's API scope is its grant's: write:user, or 403.
                if !authorization.ends_with(".u") {
                    reply(
                        &mut stream,
                        "403 Forbidden",
                        r#"{"message":"token does not have at least one of required scope(s)"}"#,
                    );
                    return;
                }
                let request: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let mut seen = seen.lock().expect("seen");
                seen.apps.push(request.clone());
                let id = seen.apps.len();
                let app = serde_json::json!({
                    "id": id,
                    "name": request["name"],
                    "client_id": format!("device-{id}"),
                    "client_secret": "never-read",
                    "redirect_uris": request["redirect_uris"],
                    "confidential_client": request["confidential_client"],
                });
                reply(&mut stream, "201 Created", &app.to_string());
            }
            ("DELETE", target)
                if behaviour.forgejo && target.starts_with("/api/v1/user/applications/oauth2/") =>
            {
                let id = target.rsplit('/').next().unwrap_or_default().to_owned();
                seen.lock().expect("seen").deleted_apps.push(id);
                reply(&mut stream, "204 No Content", "");
            }
            ("POST", "/token") => {
                let params = form(&body);
                let grant = params.get("grant_type").cloned().unwrap_or_default();
                let nonce = {
                    let mut seen = seen.lock().expect("seen");
                    seen.token_requests.push(params);
                    seen.nonce.clone().unwrap_or_default()
                };
                let mut body = if grant == "authorization_code" {
                    let nonce = behaviour.nonce_override.unwrap_or(nonce);
                    serde_json::json!({
                        "access_token": behaviour.access_token_override.as_deref().unwrap_or("access-1"),
                        "token_type": "Bearer",
                        "expires_in": 3600,
                        "refresh_token": "refresh-1",
                        "id_token": id_token(issuer, &nonce, "access-1"),
                    })
                } else if behaviour.reject_refresh {
                    reply(
                        &mut stream,
                        "400 Bad Request",
                        r#"{"error":"invalid_grant"}"#,
                    );
                    return;
                } else {
                    std::thread::sleep(behaviour.refresh_delay);
                    let mut body = serde_json::json!({
                        "access_token": "access-2",
                        "token_type": "Bearer",
                        "expires_in": 3600,
                        "refresh_token": "refresh-2",
                    });
                    if let Some(patch) = &behaviour.refresh_claims {
                        let claims = merged(base_claims(issuer), patch);
                        body["id_token"] =
                            jwt(behaviour.refresh_kid.unwrap_or("k1"), &claims).into();
                    }
                    body
                };
                if behaviour.omit_expires_in {
                    if let Some(body) = body.as_object_mut() {
                        body.remove("expires_in");
                    }
                }
                reply(&mut stream, "200 OK", &body.to_string());
            }
            ("POST", "/revoke") => {
                seen.lock().expect("seen").revocations.push(form(&body));
                reply(&mut stream, "200 OK", "{}");
            }
            ("GET", "/api/v1/user") => {
                let login = behaviour.forge_login.unwrap_or_else(|| "tgorka".to_owned());
                reply(
                    &mut stream,
                    "200 OK",
                    &serde_json::json!({ "login": login }).to_string(),
                );
            }
            _ => reply(&mut stream, "404 Not Found", "{}"),
        }
    }

    /// Forgejo's token endpoint over `seen.grants`: tokens name their client
    /// and the count they were issued at (`fa.<client>.<n>.<u|->`,
    /// `fr.<client>.<n>`).
    fn forgejo_token(
        params: &HashMap<String, String>,
        seen: &Mutex<Seen>,
    ) -> (&'static str, String) {
        let refused = |error: &str| {
            (
                "400 Bad Request",
                serde_json::json!({ "error": error }).to_string(),
            )
        };
        let issue = |client: &str, n: u32, user: bool| {
            let scope = if user { "u" } else { "-" };
            serde_json::json!({
                "access_token": format!("fa.{client}.{n}.{scope}"),
                "token_type": "Bearer",
                "expires_in": 3600,
                "refresh_token": format!("fr.{client}.{n}"),
            })
            .to_string()
        };
        let mut seen = seen.lock().expect("seen");
        let client = params.get("client_id").cloned().unwrap_or_default();
        match params.get("grant_type").map(String::as_str) {
            Some("authorization_code") => {
                let (authorized, scope) = seen.authorized.clone().unwrap_or_default();
                if authorized != client {
                    return refused("invalid_grant");
                }
                let user = scope.split_whitespace().any(|w| w == "write:user");
                let grant = seen.grants.entry(client.clone()).or_insert((0, user));
                grant.0 += 1;
                ("200 OK", issue(&client, grant.0, grant.1))
            }
            Some("refresh_token") => {
                let refresh = params.get("refresh_token").cloned().unwrap_or_default();
                let mut parts = refresh.split('.');
                let (Some("fr"), Some(of), Some(n)) = (
                    parts.next(),
                    parts.next(),
                    parts.next().and_then(|n| n.parse::<u32>().ok()),
                ) else {
                    return refused("unauthorized_client");
                };
                if of != client {
                    return refused("invalid_grant");
                }
                let Some(grant) = seen.grants.get_mut(&client) else {
                    return refused("invalid_grant");
                };
                if grant.0 != n {
                    // "A client tried to use a refresh token for grant_id = 2 was used twice!"
                    return refused("unauthorized_client");
                }
                grant.0 += 1;
                ("200 OK", issue(&client, grant.0, grant.1))
            }
            _ => refused("unsupported_grant_type"),
        }
    }

    impl Fake {
        fn token_requests(&self) -> Vec<HashMap<String, String>> {
            self.seen.lock().expect("seen").token_requests.clone()
        }

        fn refreshes(&self) -> usize {
            self.token_requests()
                .iter()
                .filter(|r| r.get("grant_type").map(String::as_str) == Some("refresh_token"))
                .count()
        }
    }

    // ---- the fake platform (the browser) --------------------------------

    #[derive(Clone, Copy, PartialEq)]
    enum Browser {
        /// Calls back with the state it was given.
        Honest,
        /// Calls back carrying a second, attacker-chosen `state` beside the real one.
        ExtraState,
        /// Calls back for a different flow's state.
        ForeignState,
        /// The identity provider answers `error=access_denied`, with an
        /// `error_description` when one is given.
        Denied(Option<&'static str>),
        /// The person closes the sign-in sheet.
        Closed,
        /// The sheet hands back an answer that is not this flow's: the
        /// platform ends the flow with this reason (`OAuthFlowRegistry::fail`).
        BrokenRoundTrip(&'static str),
        /// The sheet's presentation is queued, then refused later (iOS's
        /// `presentationContextInvalid`): the platform says so through
        /// `OAuthFlowRegistry::not_shown`, as `web_auth::finish` does.
        RefusedLater,
        /// The flow is dropped without any answer.
        Dropped,
        /// The sheet stays open, unanswered, until something in keeper ends
        /// the flow.
        Waits,
        /// Answers the first sign-in honestly and closes every later sheet.
        HonestThenClosed,
        /// The system cannot open the sign-in page at all; the person pastes
        /// the link into a browser themselves.
        WontOpen,
    }

    struct FakePlatform {
        data_dir: PathBuf,
        keychain: Mutex<HashMap<String, String>>,
        flows: Arc<OAuthFlowRegistry>,
        seen: Arc<Mutex<Seen>>,
        browser: Browser,
        /// Every URL shown, with the scheme passed to `start_web_auth` (or
        /// `None` for `open_url`).
        shown: Mutex<Vec<(String, Option<String>)>>,
        /// Every link the sign-in screen was given (R470), in order; `None`
        /// where it was taken away.
        links: Mutex<Vec<Option<crate::sign_in_link::SignInLinkVm>>>,
    }

    fn query(url: &Url, key: &str) -> Vec<String> {
        url.query_pairs()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
            .collect()
    }

    fn one(url: &Url, key: &str) -> String {
        query(url, key).pop().unwrap_or_default()
    }

    impl FakePlatform {
        fn new(fake: &Fake, browser: Browser) -> Self {
            static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let data_dir =
                std::env::temp_dir().join(format!("keeper-oidc-test-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&data_dir).expect("temp data dir");
            FakePlatform {
                data_dir,
                keychain: Mutex::default(),
                flows: Arc::new(OAuthFlowRegistry::new()),
                seen: Arc::clone(&fake.seen),
                browser,
                shown: Mutex::default(),
                links: Mutex::default(),
            }
        }

        /// What the browser does with an authorization URL: remember the
        /// nonce for the provider and produce the callback URL.
        fn visit(&self, url: &str) -> Result<String, CoreError> {
            let mut url = Url::parse(url).map_err(|e| CoreError::Internal(e.to_string()))?;
            // A forge `signin_url`: the authorization request rides in `redirect_to`.
            if let Some(inner) = query(&url, "redirect_to").pop() {
                url = url
                    .join(&inner)
                    .map_err(|e| CoreError::Internal(e.to_string()))?;
            }
            {
                let mut seen = self.seen.lock().expect("seen");
                seen.nonce = Some(one(&url, "nonce"));
                seen.authorized = Some((one(&url, "client_id"), one(&url, "scope")));
            }
            let state = one(&url, "state");
            let states = match self.browser {
                Browser::Honest
                | Browser::Denied(_)
                | Browser::Closed
                | Browser::BrokenRoundTrip(_)
                | Browser::RefusedLater
                | Browser::Dropped
                | Browser::Waits
                | Browser::WontOpen
                | Browser::HonestThenClosed => format!("state={state}"),
                Browser::ExtraState => format!("state=evil&state={state}"),
                Browser::ForeignState => "state=someone-else".to_owned(),
            };
            Ok(format!(
                "{}?code=code-1&{states}",
                one(&url, "redirect_uri")
            ))
        }

        fn session(&self, id: &str) -> Option<serde_json::Value> {
            let raw = self
                .keychain
                .lock()
                .expect("keychain")
                .get(&session_key(id))
                .cloned()?;
            serde_json::from_str(&raw).ok()
        }
    }

    impl Drop for FakePlatform {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.data_dir);
        }
    }

    impl Platform for FakePlatform {
        fn data_dir(&self) -> Result<PathBuf, CoreError> {
            Ok(self.data_dir.clone())
        }
        fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError> {
            self.keychain
                .lock()
                .expect("keychain")
                .insert(key.to_owned(), value.to_owned());
            Ok(())
        }
        fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError> {
            Ok(self.keychain.lock().expect("keychain").get(key).cloned())
        }
        fn keychain_delete(&self, key: &str) -> Result<(), CoreError> {
            self.keychain.lock().expect("keychain").remove(key);
            Ok(())
        }
        fn open_url(&self, url: &str) -> Result<(), CoreError> {
            self.shown
                .lock()
                .expect("shown")
                .push((url.to_owned(), None));
            // A loopback redirect: the browser GETs the listener, which routes
            // the callback through the registry itself.
            let callback =
                Url::parse(&self.visit(url)?).map_err(|e| CoreError::Internal(e.to_string()))?;
            std::thread::spawn(move || {
                let addr: SocketAddr = format!(
                    "{}:{}",
                    callback.host_str().unwrap_or_default(),
                    callback.port().unwrap_or(80)
                )
                .parse()
                .expect("loopback addr");
                let mut stream = TcpStream::connect(addr).expect("reach the loopback");
                let target = format!(
                    "{}?{}",
                    callback.path(),
                    callback.query().unwrap_or_default()
                );
                let _ = write!(stream, "GET {target} HTTP/1.1\r\nHost: {addr}\r\n\r\n");
                let _ = stream.read_to_end(&mut Vec::new());
            });
            Ok(())
        }
        fn start_web_auth(&self, url: &str, callback_scheme: &str) -> Result<(), CoreError> {
            self.shown
                .lock()
                .expect("shown")
                .push((url.to_owned(), Some(callback_scheme.to_owned())));
            if matches!(self.browser, Browser::WontOpen) {
                return Err(CoreError::Internal("no browser here".to_owned()));
            }
            let callback = self.visit(url)?;
            // From the callback: behind a `signin_url` the authorization
            // request, and its state, ride inside `redirect_to`.
            let state = one(&Url::parse(&callback).expect("callback url"), "state");
            match self.browser {
                Browser::Denied(description) => {
                    let mut answer =
                        Url::parse(&callback).map_err(|e| CoreError::Internal(e.to_string()))?;
                    answer.set_query(None);
                    answer
                        .query_pairs_mut()
                        .append_pair("error", "access_denied")
                        .extend_pairs(description.map(|d| ("error_description", d)))
                        .append_pair("state", &state);
                    self.flows.resolve(answer.as_str());
                }
                Browser::Closed => self.flows.cancel(&state),
                Browser::HonestThenClosed if self.shown.lock().expect("shown").len() > 1 => {
                    self.flows.cancel(&state);
                }
                Browser::BrokenRoundTrip(reason) => {
                    self.flows.fail(&state, reason);
                }
                Browser::RefusedLater => {
                    let flows = Arc::clone(&self.flows);
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(20));
                        flows.not_shown(&state, "presentationContextInvalid");
                    });
                }
                Browser::Dropped => self.flows.remove(&state),
                Browser::Waits | Browser::WontOpen => {}
                Browser::Honest
                | Browser::ExtraState
                | Browser::ForeignState
                | Browser::HonestThenClosed => {
                    self.flows.resolve(&callback);
                }
            }
            Ok(())
        }
        fn show_sign_in_link(&self, _: u64, link: Option<&crate::sign_in_link::SignInLinkVm>) {
            self.links.lock().expect("links").push(link.cloned());
        }
        fn notify(&self, _: &str, _: &str, _: &NotifyTarget) -> Result<(), CoreError> {
            Ok(())
        }
        fn sidecar_path(&self, _: &str) -> Result<PathBuf, CoreError> {
            Err(CoreError::Unsupported("no sidecars in tests".to_owned()))
        }
        fn exclude_from_backup(&self, _: &std::path::Path) -> Result<(), CoreError> {
            Ok(())
        }
        fn set_badge_count(&self, _: Option<u32>) -> Result<(), CoreError> {
            Ok(())
        }
    }

    fn account(fake: &Fake, id: &str, redirect: Option<&str>) -> AccountDescriptor {
        let redirect = redirect
            .map(|r| format!(r#","redirect_uri":"{r}""#))
            .unwrap_or_default();
        descriptor::parse_json(&format!(
            r#"{{"version":1,"id":"{id}","name":"Acme",
                "auth":{{"issuer":"{}","client_id":"{CLIENT_ID}"{redirect}}},
                "config":{{"url":"https://git.acme.dev/c.git"}}}}"#,
            fake.issuer
        ))
        .expect("a valid test descriptor")
    }

    fn http() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("client")
    }

    async fn sign_in_at_fake(
        p: &FakePlatform,
        d: &AccountDescriptor,
    ) -> Result<Identity, AccountError> {
        sign_in_within(p, &p.flows, &http(), d, ARRIVES).await
    }

    /// Seed a signed-in session for `d`, as a sign-in at `d`'s issuer would
    /// have stored it, whose access token has expired.
    fn seed_expired_session(p: &FakePlatform, d: &AccountDescriptor) {
        seed_session(p, d, Vec::new(), now_ms() - 1);
    }

    fn seed_session(p: &FakePlatform, d: &AccountDescriptor, roles: Vec<String>, expires_ms: i64) {
        let mut original = base_claims(&d.auth.issuer);
        original["nonce"] = "n0".into();
        let session = StoredSession {
            binding: Binding::session(d),
            iss: d.auth.issuer.clone(),
            sub: "sub-1".to_owned(),
            refresh_token: Some("refresh-1".to_owned()),
            access_token: "access-1".to_owned(),
            access_expires_ms: Some(expires_ms),
            id_token: jwt("k1", &original),
            login: "tgorka".to_owned(),
            display_name: "tgorka".to_owned(),
            email: None,
            roles,
            sign_in: Some("seeded".to_owned()),
        };
        store_session(p, &d.id, &session).expect("seed session");
    }

    // ---- sign-in ---------------------------------------------------------

    /// The person, handed a link with its sentence, pastes it into their own
    /// browser: wait (bounded — a sign-in that ended at once never shows the
    /// sentence, and the test must fail rather than wait for it), then bring
    /// the browser's callback.
    async fn paste_the_unopened_link(p: &FakePlatform) {
        let unopened = || {
            p.links
                .lock()
                .expect("links")
                .iter()
                .flatten()
                .find(|link| link.not_opened.is_some())
                .map(|link| link.url.clone())
        };
        let link = tokio::time::timeout(ARRIVES, async {
            loop {
                if let Some(link) = unopened() {
                    break link;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the link with its sentence");
        let callback = p.visit(&link).expect("the person's browser answers");
        assert!(p.flows.resolve(&callback), "the sign-in still waits");
    }

    /// The link is on the sign-in screen before the browser is asked for it,
    /// and a page the system could not open does not end the sign-in: the
    /// link stays with the sentence, the callback the person's own browser
    /// brings finishes it, and then the link is taken away (102.9 #1, R470,
    /// R480).
    #[tokio::test]
    async fn a_page_that_did_not_open_leaves_its_link_and_the_pasted_link_finishes_the_sign_in() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::WontOpen);
        let d = account(&fake, "paste", None);

        let (identity, ()) = tokio::join!(sign_in_at_fake(&p, &d), paste_the_unopened_link(&p));

        assert_eq!(identity.expect("signed in by hand").login, "tgorka");
        let links = p.links.lock().expect("links").clone();
        let shown = p.shown.lock().expect("shown").clone();
        assert_eq!(links.len(), 3, "{links:?}");
        let first = links[0].as_ref().expect("the link");
        assert_eq!(
            first.url, shown[0].0,
            "the link is the page keeper asked for"
        );
        assert_eq!(first.not_opened, None);
        let second = links[1].as_ref().expect("the link again");
        assert_eq!(second.url, first.url);
        assert_eq!(
            second.not_opened.as_deref(),
            Some(crate::sign_in_link::NOT_OPENED)
        );
        assert_eq!(links[2], None, "the finished sign-in's link is gone");
    }

    /// iOS queues the sign-in sheet, so `start_web_auth` succeeds — and the
    /// sheet is refused afterwards. That refusal does not end the sign-in
    /// either: the link gets its sentence, and the pasted link finishes the
    /// very same flow (R480, DW-1590).
    #[tokio::test]
    async fn a_sheet_refused_after_it_was_queued_leaves_the_same_sign_in_to_the_pasted_link() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::RefusedLater);
        let d = account(&fake, "later", None);

        let (identity, ()) = tokio::join!(sign_in_at_fake(&p, &d), paste_the_unopened_link(&p));

        assert_eq!(identity.expect("signed in by hand").login, "tgorka");
        assert_eq!(
            p.shown.lock().expect("shown").len(),
            1,
            "one flow, one page"
        );
        assert_eq!(
            fake.token_requests().len(),
            1,
            "its code was exchanged once"
        );
        let links = p.links.lock().expect("links").clone();
        assert_eq!(links.last(), Some(&None), "{links:?}");
    }

    /// However a sign-in ends — refused, cancelled, out of time — its link
    /// is taken away when it ends, not merely hidden (R480/03).
    #[tokio::test]
    async fn every_ending_takes_the_link_away() {
        let fake = start_fake(Behaviour::default());
        for browser in [
            Browser::Denied(None),
            Browser::Closed,
            Browser::BrokenRoundTrip("no answer"),
            Browser::Waits,
        ] {
            let p = FakePlatform::new(&fake, browser);
            let d = account(&fake, "gone", None);
            // `Waits` answers nothing: the sign-in ends at its deadline.
            assert!(sign_in_within(&p, &p.flows, &http(), &d, SHORT)
                .await
                .is_err());
            let links = p.links.lock().expect("links").clone();
            assert!(matches!(links.first(), Some(Some(_))), "{links:?}");
            assert_eq!(links.last(), Some(&None), "{links:?}");
        }
    }

    #[tokio::test]
    async fn each_attempt_sends_fresh_state_nonce_and_an_s256_challenge_the_verifier_answers() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "pkce", None);

        let identity = sign_in_at_fake(&p, &d).await.expect("first sign-in");
        sign_in_at_fake(&p, &d).await.expect("second sign-in");

        assert_eq!(identity.sub, "sub-1");
        assert_eq!(identity.login, "tgorka");
        let shown = p.shown.lock().expect("shown").clone();
        assert_eq!(shown.len(), 2);
        let urls: Vec<Url> = shown
            .iter()
            .map(|(u, _)| Url::parse(u).expect("url"))
            .collect();
        assert_eq!(
            shown[0].1.as_deref(),
            Some("keeper"),
            "custom scheme goes to the auth session"
        );
        for key in ["state", "nonce", "code_challenge"] {
            assert_ne!(one(&urls[0], key), one(&urls[1], key), "{key} was reused");
            assert!(one(&urls[0], key).len() >= 16, "{key} is not random enough");
        }
        assert_eq!(one(&urls[0], "code_challenge_method"), "S256");
        assert_eq!(
            one(&urls[0], "scope"),
            "openid profile email offline_access",
            "the descriptor's scopes, openid not doubled"
        );

        let requests = fake.token_requests();
        for (request, url) in requests.iter().zip(&urls) {
            let verifier = request.get("code_verifier").expect("the verifier is sent");
            let challenge = PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(
                verifier.clone(),
            ));
            assert_eq!(challenge.as_str(), one(url, "code_challenge"));
            assert_eq!(
                request.get("redirect_uri").map(String::as_str),
                Some("keeper://oauth/pkce/callback")
            );
        }
        let session = p
            .session("pkce")
            .expect("session persisted before returning");
        assert_eq!(session["refresh_token"], "refresh-1");
        assert_eq!(session["sub"], "sub-1");
    }

    #[tokio::test]
    async fn a_callback_carrying_a_second_state_is_refused_before_any_exchange() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::ExtraState);
        let d = account(&fake, "extra-state", None);

        let result = sign_in_at_fake(&p, &d).await;

        assert!(
            matches!(result, Err(AccountError::Refused(_))),
            "{result:?}"
        );
        assert!(fake.token_requests().is_empty(), "the code was exchanged");
        assert!(p.session("extra-state").is_none());
    }

    #[tokio::test]
    async fn a_callback_for_another_state_never_completes_the_flow() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::ForeignState);
        let d = account(&fake, "foreign-state", None);

        let result = sign_in_within(&p, &p.flows, &http(), &d, Duration::from_millis(300)).await;

        assert!(matches!(result, Err(AccountError::TimedOut)), "{result:?}");
        assert!(fake.token_requests().is_empty());
        assert!(p.session("foreign-state").is_none());
    }

    /// R608-02: drive the actual authorization future and its FlowGuard,
    /// not a manual registry removal. Its sheet opens at t=299, then the
    /// original 300 s timeout (or dropping the future) must retire it even
    /// if no later presentation attempt ever looks at the table.
    #[tokio::test(start_paused = true)]
    async fn an_authorize_timeout_or_drop_retires_its_sheet_without_another_attempt() {
        use crate::sign_in_sheet::{PreviousSheet, SheetId, Sheets, DISMISSAL};

        let fake = start_fake(Behaviour::default());
        for timed_out in [true, false] {
            let p = FakePlatform::new(&fake, Browser::Waits);
            let provider = Provider {
                host: "git.acme.dev".to_owned(),
                client_id: ClientId::new(CLIENT_ID.to_owned()),
                auth_url: AuthUrl::new("https://git.acme.dev/authorize".to_owned()).expect("url"),
                token_url: TokenUrl::new("https://git.acme.dev/token".to_owned()).expect("url"),
                revocation_url: None,
                oidc: None,
            };
            let start = std::time::Instant::now();
            let deadline = start + Duration::from_secs(300);
            let sheets = Arc::new(Mutex::new(Sheets::new()));
            let weak = Arc::downgrade(&sheets);
            assert!(p.flows.on_end(move |_| {
                weak.upgrade()
                    .expect("table")
                    .lock()
                    .expect("table")
                    .ended(deadline);
            }));
            let mut authorizing = Box::pin(authorize(
                &p,
                &p.flows,
                &provider,
                "keeper://oauth/test/callback",
                &[],
                None,
                Duration::from_secs(300),
            ));
            assert!(futures_util::poll!(authorizing.as_mut()).is_pending());
            tokio::time::advance(Duration::from_secs(299)).await;
            let state = one(
                &Url::parse(&p.shown.lock().expect("shown")[0].0).expect("url"),
                "state",
            );
            sheets
                .lock()
                .expect("table")
                .open(SheetId::next(), Arc::clone(&p.flows), &state, ());
            assert_eq!(
                sheets.lock().expect("table").previous(deadline),
                PreviousSheet::OnScreen
            );
            if timed_out {
                tokio::time::advance(Duration::from_secs(1)).await;
                assert!(matches!(authorizing.await, Err(AccountError::TimedOut)));
            } else {
                drop(authorizing);
            }
            assert_eq!(
                sheets.lock().expect("table").previous(deadline + DISMISSAL),
                PreviousSheet::EndedAgo(DISMISSAL),
                "the authorization's guard must retire its sheet without another look"
            );
            assert!(!p.flows.waits(&state));
        }
    }

    /// R381 / 100.1 #2: a sign-in that ends without a code reads four ways —
    /// closed by the person (quiet), refused by the identity provider (with
    /// its own words when it sent any), timed out, or a broken round trip —
    /// and none of them exchanges anything or stores a session.
    #[tokio::test]
    async fn each_way_a_sign_in_ends_without_a_code_reads_as_itself() {
        let fake = start_fake(Behaviour::default());
        let host = Url::parse(&fake.issuer)
            .expect("issuer")
            .host_str()
            .expect("host")
            .to_owned();
        let cases: [(Browser, &str); 5] = [
            (
                Browser::Denied(Some("User is\nnot allowed to use this project.")),
                "denied with words",
            ),
            (Browser::Denied(None), "denied"),
            (Browser::Closed, "closed"),
            (
                Browser::BrokenRoundTrip("the sign-in page answered without its state"),
                "broken round trip",
            ),
            (Browser::Dropped, "dropped"),
        ];
        for (browser, case) in cases {
            let p = FakePlatform::new(&fake, browser);
            let d = account(&fake, "ends", None);
            let result = sign_in_at_fake(&p, &d).await;
            match (browser, &result) {
                (Browser::Denied(Some(_)), Err(AccountError::Denied { host: h, reason })) => {
                    assert_eq!(h, &host, "{case}");
                    assert_eq!(
                        reason.as_deref(),
                        Some("User is not allowed to use this project"),
                        "{case}: the provider's words, on one line"
                    );
                }
                (Browser::Denied(None), Err(AccountError::Denied { reason: None, .. })) => {}
                (Browser::Closed, Err(AccountError::Cancelled)) => {}
                (Browser::BrokenRoundTrip(_), Err(AccountError::Interrupted(reason))) => {
                    assert_eq!(
                        reason, "the sign-in page answered without its state",
                        "{case}"
                    );
                }
                (Browser::Dropped, Err(AccountError::Interrupted(_))) => {}
                _ => panic!("{case}: {result:?}"),
            }
            assert!(p.session("ends").is_none(), "{case}");
        }
        assert!(fake.token_requests().is_empty(), "a code was exchanged");
    }

    /// Where a test's `fmt` subscriber writes, so its lines can be read back.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("captured").extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// R381 / 100.1 #3 — the Marta bug: an `access_denied` sign-in used to end
    /// with no problem and no log line. It now leaves exactly one WARN line
    /// naming the step, the account and the error's kind (never the
    /// provider's words or a URL), and a problem the account's status shows.
    #[tokio::test]
    async fn a_refused_sign_in_leaves_one_warning_and_a_problem() {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .with_writer(move || writer.clone())
            .finish();
        let _logging = tracing::subscriber::set_default(subscriber);

        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Denied(Some("Marta may not use keeper")));
        let d = account(&fake, "marta", None);
        let error = sign_in_at_fake(&p, &d)
            .await
            .expect_err("the provider refused");
        let problem = crate::org_account::state::step_failed(
            crate::org_account::state::AccountStep::SignIn,
            &d.id,
            error,
        );

        assert!(
            matches!(
                &problem,
                Some(crate::org_account::state::AccountProblem::SignInDenied { .. })
            ),
            "{problem:?}"
        );
        let text = String::from_utf8(captured.0.lock().expect("captured").clone()).expect("utf-8");
        let warnings: Vec<&str> = text
            .lines()
            .filter(|line| line.contains("account: a step did not finish"))
            .collect();
        assert_eq!(warnings.len(), 1, "{text}");
        let line = warnings[0];
        for field in [
            "WARN",
            r#"step="sign_in""#,
            r#"account="marta""#,
            r#"kind="denied""#,
        ] {
            assert!(line.contains(field), "{field} missing: {line}");
        }
        for private in ["Marta may not use keeper", "code=", "state="] {
            assert!(!line.contains(private), "{private} leaked: {line}");
        }
    }

    /// The `account: a step did not finish` lines a run wrote.
    fn step_warnings(captured: &Captured) -> Vec<String> {
        let text = String::from_utf8(captured.0.lock().expect("captured").clone()).expect("utf-8");
        text.lines()
            .filter(|line| line.contains("account: a step did not finish"))
            .map(str::to_owned)
            .collect()
    }

    #[tokio::test]
    async fn retry_during_actual_discovery_opens_no_obsolete_sign_in_sheet() {
        use crate::org_account::attempt::{Attempts, Run};

        let (release, held) = std::sync::mpsc::channel();
        let discovery = Arc::new(DiscoveryHold {
            arrived: tokio::sync::Notify::new(),
            release: Mutex::new(held),
        });
        let fake = start_fake(Behaviour {
            discovery_hold: Some(Arc::clone(&discovery)),
            ..Default::default()
        });
        let p = FakePlatform::new(&fake, Browser::Waits);
        let d = account(&fake, "superseded-discovery", None);
        let attempts = Attempts::default();
        let queued_at = attempts.epoch();
        let superseded = p.flows.cancel_latch();
        let client = http();
        let old = attempts.run_until(
            queued_at,
            Some(d.id.as_str()),
            superseded.cancelled(),
            sign_in_within(&p, &p.flows, &client, &d, FLOW_TIMEOUT),
        );
        let retry = async {
            discovery.arrived.notified().await;
            p.flows.cancel_all();
            let _ = release.send(());
        };
        let (ran, ()) = tokio::join!(old, retry);
        assert!(
            matches!(ran, Run::Cancelled),
            "retry must end the interactive attempt before discovery can open its sheet"
        );
        assert!(p.shown.lock().expect("shown").is_empty());
        assert!(p.session(&d.id).is_none());
        assert_eq!(attempts.epoch(), queued_at);
    }

    /// One interactive attempt through the account's attempt boundary, as
    /// the shell's converge runs its sign-in step: `Done` past the sign-in,
    /// or however the step ended the attempt.
    async fn re_setup(
        attempts: &crate::org_account::attempt::Attempts,
        p: &FakePlatform,
        d: &AccountDescriptor,
    ) -> crate::org_account::attempt::Run<crate::org_account::state::AccountAttemptVm> {
        use crate::org_account::state::{AccountAttemptVm, AccountStep};
        let queued_at = attempts.epoch();
        attempts
            .run(queued_at, Some(d.id.as_str()), async {
                match attempts
                    .step(
                        AccountStep::SignIn,
                        &d.id,
                        sign_in_within(p, &p.flows, &http(), d, FLOW_TIMEOUT),
                    )
                    .await
                {
                    Ok(_) => AccountAttemptVm::Done,
                    Err(ended) => ended.attempt,
                }
            })
            .await
    }

    /// R410 (R100E2-04), native close: Acme is already signed in on this
    /// device (a restored session) and the person sets it up again, then
    /// closes the platform's sign-in sheet — the flow answers `Cancelled`
    /// through the registry. The attempt is closed, never done; the sign-in
    /// step is logged exactly once, as closed; Acme's session stays as it
    /// was.
    #[tokio::test]
    async fn closing_the_sheet_of_a_re_setup_closes_the_attempt_with_one_warning() {
        use crate::org_account::attempt::Attempts;
        use crate::org_account::state::AccountAttemptVm;
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .with_writer(move || writer.clone())
            .finish();
        let _logging = tracing::subscriber::set_default(subscriber);
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Closed);
        let d = account(&fake, "acme", None);
        seed_session(&p, &d, Vec::new(), now_ms() + 3_600_000);

        let attempts = Attempts::default();
        let attempt = re_setup(&attempts, &p, &d).await.attempt();

        assert_eq!(attempt, AccountAttemptVm::Closed);
        let warnings = step_warnings(&captured);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        for field in [r#"step="sign_in""#, r#"account="acme""#, r#"kind="closed""#] {
            assert!(
                warnings[0].contains(field),
                "{field} missing: {}",
                warnings[0]
            );
        }
        assert_eq!(
            p.session("acme").expect("the restored session stays")["refresh_token"],
            "refresh-1"
        );
        assert!(fake.token_requests().is_empty(), "a code was exchanged");
    }

    /// R410 (R100E2-04), in-app Cancel: the same re-setup, but the sheet stays
    /// open and the person presses Cancel in keeper. The shell's interrupt
    /// stops the run and then ends the account's sheets — so the flow also
    /// answers `Cancelled`; the run is dropped at the sign-in step it was on
    /// and that step is logged exactly once, as closed. The attempt is closed,
    /// never done, and the session stays. A sign-in still queued when the
    /// person cancels never starts, logs nothing, and is closed too.
    #[tokio::test]
    async fn cancel_in_keeper_closes_a_re_setup_at_its_step_with_one_warning() {
        use crate::org_account::attempt::{Attempts, Run};
        use crate::org_account::state::AccountAttemptVm;
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .with_writer(move || writer.clone())
            .finish();
        let _logging = tracing::subscriber::set_default(subscriber);
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Waits);
        let d = account(&fake, "acme", None);
        seed_session(&p, &d, Vec::new(), now_ms() + 3_600_000);
        let attempts = Attempts::default();

        let cancel = async {
            while p.shown.lock().expect("shown").is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            // What the shell's `interrupt_all` does, in its order.
            attempts.interrupt();
            p.flows.cancel_all();
        };
        let (run, ()) = tokio::join!(re_setup(&attempts, &p, &d), cancel);

        assert_eq!(run, Run::Cancelled);
        assert_eq!(run.attempt(), AccountAttemptVm::Closed);
        let warnings = step_warnings(&captured);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        for field in [r#"step="sign_in""#, r#"account="acme""#, r#"kind="closed""#] {
            assert!(
                warnings[0].contains(field),
                "{field} missing: {}",
                warnings[0]
            );
        }
        assert_eq!(
            p.session("acme").expect("the restored session stays")["refresh_token"],
            "refresh-1"
        );

        // Queued behind something else when the person cancels: never starts.
        let queued_at = attempts.epoch();
        attempts.interrupt();
        let queued = attempts
            .run(queued_at, Some(d.id.as_str()), async {
                AccountAttemptVm::Done
            })
            .await;
        assert_eq!(queued, Run::NotStarted);
        assert_eq!(queued.attempt(), AccountAttemptVm::Closed);
        assert_eq!(step_warnings(&captured).len(), 1);
    }

    #[test]
    fn a_provider_s_words_are_bounded_to_one_clean_line() {
        assert_eq!(provider_words(" \n. "), None);
        assert_eq!(
            provider_words("Not\tallowed.\u{7}").as_deref(),
            Some("Not allowed")
        );
        let long = "a".repeat(PROVIDER_WORDS_MAX + 5);
        let bounded = provider_words(&long).expect("words");
        assert_eq!(bounded.chars().count(), PROVIDER_WORDS_MAX + 1);
        assert!(bounded.ends_with('…'));
    }

    #[tokio::test]
    async fn an_id_token_for_another_nonce_is_refused_and_nothing_is_stored() {
        let fake = start_fake(Behaviour {
            nonce_override: Some("replayed-nonce".to_owned()),
            ..Behaviour::default()
        });
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "nonce", None);

        let result = sign_in_at_fake(&p, &d).await;

        assert!(
            matches!(result, Err(AccountError::Refused(_))),
            "{result:?}"
        );
        assert!(p.session("nonce").is_none());
    }

    #[tokio::test]
    async fn an_at_hash_for_another_access_token_is_refused() {
        let fake = start_fake(Behaviour {
            access_token_override: Some("swapped-access".to_owned()),
            ..Behaviour::default()
        });
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "at-hash", None);

        let result = sign_in_at_fake(&p, &d).await;

        assert!(
            matches!(result, Err(AccountError::Refused(_))),
            "{result:?}"
        );
        assert!(p.session("at-hash").is_none());
    }

    #[tokio::test]
    async fn an_unknown_kid_refetches_the_key_set_once() {
        let fake = start_fake(Behaviour {
            stale_jwks_first: true,
            ..Behaviour::default()
        });
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "kid", None);

        sign_in_at_fake(&p, &d)
            .await
            .expect("the rotated key verifies");

        assert_eq!(fake.seen.lock().expect("seen").jwks_fetches, 2);
    }

    #[tokio::test]
    async fn a_loopback_redirect_is_served_by_keeper_and_opened_in_the_browser() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "loopback", Some("http://127.0.0.1/callback"));

        sign_in_at_fake(&p, &d).await.expect("loopback sign-in");

        let shown = p.shown.lock().expect("shown").clone();
        assert_eq!(
            shown[0].1, None,
            "a loopback redirect never reaches the auth session"
        );
        let sent = Url::parse(&one(&Url::parse(&shown[0].0).expect("url"), "redirect_uri"))
            .expect("redirect");
        assert_ne!(sent.port(), None, "the bound port is in the redirect");
        assert_eq!(
            fake.token_requests()[0]
                .get("redirect_uri")
                .map(String::as_str),
            Some(sent.as_str()),
            "the exchange repeats the exact redirect"
        );
    }

    // ---- access_token ----------------------------------------------------

    #[tokio::test]
    async fn a_refresh_persists_the_rotated_refresh_token_before_returning() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "rotate", None);
        seed_expired_session(&p, &d);

        let token = access_token(&p, &http(), &d).await.expect("refreshed");

        assert_eq!(token, "access-2");
        let request = &fake.token_requests()[0];
        assert_eq!(
            request.get("refresh_token").map(String::as_str),
            Some("refresh-1")
        );
        let session = p.session("rotate").expect("session");
        assert_eq!(session["refresh_token"], "refresh-2");
        assert_eq!(session["access_token"], "access-2");
        // Fresh now: a second call does not touch the provider.
        access_token(&p, &http(), &d).await.expect("cached");
        assert_eq!(fake.refreshes(), 1);
    }

    #[tokio::test]
    async fn concurrent_callers_share_one_refresh() {
        let fake = start_fake(Behaviour {
            refresh_delay: Duration::from_millis(300),
            ..Behaviour::default()
        });
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "single-flight", None);
        seed_expired_session(&p, &d);
        let client = http();

        let (a, b) = tokio::join!(access_token(&p, &client, &d), access_token(&p, &client, &d));

        assert_eq!(a.expect("first"), "access-2");
        assert_eq!(b.expect("second"), "access-2");
        assert_eq!(
            fake.refreshes(),
            1,
            "two refreshes would replay a rotated token"
        );
    }

    #[tokio::test]
    async fn a_minted_token_is_always_a_new_refresh_even_for_concurrent_callers() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "minted", None);
        seed_session(&p, &d, Vec::new(), now_ms() + 3_600_000);
        let client = http();

        // The stored token is fresh: the ordinary call takes it as it is.
        assert_eq!(
            access_token(&p, &client, &d).await.expect("cached"),
            "access-1"
        );
        assert_eq!(fake.refreshes(), 0);
        assert_eq!(
            minted_access_token(&p, &client, &d).await.expect("minted"),
            "access-2"
        );
        assert_eq!(fake.refreshes(), 1, "a fresh stored token is not reused");
        // The one just minted is fresh too, and still not handed out again.
        let (a, b) = tokio::join!(
            minted_access_token(&p, &client, &d),
            minted_access_token(&p, &client, &d)
        );
        a.expect("first");
        b.expect("second");
        assert_eq!(fake.refreshes(), 3, "each caller gets its own refresh");
    }

    // ---- the agents server's sign-in (109.6, R1096-05/-06) -----------------

    /// The agents server's sign-in view of `p`'s account, as keeper runs it.
    fn org(p: &Arc<FakePlatform>, d: &AccountDescriptor) -> crate::hq::signin::OrgAccount {
        let d = d.clone();
        crate::hq::signin::OrgAccount::new(
            Arc::clone(p) as Arc<dyn Platform>,
            move || Some(d.clone()),
            || Ok(http()),
        )
    }

    fn sign_in_mark(org: &crate::hq::signin::OrgAccount) -> Option<String> {
        use crate::hq::signin::{AccountState, SignIn};
        match org.account() {
            AccountState::SignedIn(account) => account.sign_in,
            other => panic!("not signed in: {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_second_sign_in_is_a_new_agents_sign_in_though_auth_time_is_the_same() {
        // The fake provider states `auth_time` 1000 at every sign-in, as an
        // identity provider completing it from its own live session may.
        let fake = start_fake(Behaviour::default());
        let p = Arc::new(FakePlatform::new(&fake, Browser::Honest));
        let d = account(&fake, "again", None);
        let org = org(&p, &d);

        sign_in_at_fake(&p, &d).await.expect("signs in");
        let first = sign_in_mark(&org);
        sign_in_at_fake(&p, &d).await.expect("signs in again");
        let second = sign_in_mark(&org);

        assert!(first.is_some());
        assert_ne!(first, second, "a new interactive sign-in");
    }

    /// An agents server that takes any sign-in of `d`'s account and records
    /// the sign-in tokens it is given at enrolment and attestation.
    fn agents_server(d: &AccountDescriptor) -> crate::forges::testing::Fake {
        use crate::forges::testing::{serve, Reply};
        let config = serde_json::json!({
            "issuer": d.auth.issuer,
            "clientId": d.auth.client_id,
            "scopes": ["openid"],
        })
        .to_string();
        serve(move |seen| match seen.path.as_str() {
            "/auth/config" => Reply::json(200, &config),
            "/devices/enroll" => Reply::json(
                200,
                r#"{"token":"device-token-1","clientId":"ana.1","actorId":"ana","databases":[{"name":"alpha"}],"adopted":false}"#,
            ),
            "/databases" => Reply::json(
                200,
                r#"{"actorId":"ana","clientId":"ana.1","databases":[{"name":"alpha"}]}"#,
            ),
            "/clients/self/attest" => Reply::json(
                200,
                r#"{"clientId":"ana.1","attestedAtMs":1,"lapsesAtMs":604800001}"#,
            ),
            _ => Reply::json(404, "{}"),
        })
    }

    fn agents_hq(p: &Arc<FakePlatform>, d: &AccountDescriptor) -> Arc<crate::hq::Hq> {
        crate::hq::Hq::new(
            Arc::clone(p) as Arc<dyn Platform>,
            Arc::new(crate::hq::port::fake::FakeOpener::default()),
            Arc::new(crate::hq::routes::HttpRoutes::new()),
            Arc::new(org(p, d)),
            "keeper".to_owned(),
            || crate::hq::DeviceHosts::same("hesperia"),
        )
    }

    fn asked(server: &crate::forges::testing::Fake, path: &str) -> Vec<String> {
        server
            .requests()
            .into_iter()
            .filter(|seen| seen.path == path)
            .map(|seen| seen.header("authorization").unwrap_or_default().to_owned())
            .collect()
    }

    #[tokio::test]
    async fn a_refresh_without_auth_time_neither_moves_the_agents_sign_in_nor_spins_enrolment() {
        let fake = start_fake(Behaviour {
            // A refreshed ID token without `auth_time` (OIDC Core §2 makes it
            // optional unless asked for).
            refresh_claims: Some(serde_json::json!({ "auth_time": null })),
            ..Behaviour::default()
        });
        let p = Arc::new(FakePlatform::new(&fake, Browser::Honest));
        let d = account(&fake, "no-auth-time", None);
        seed_session(&p, &d, Vec::new(), now_ms() + 3_600_000);
        let before = sign_in_mark(&org(&p, &d));
        let agents = agents_server(&d);
        let hq = agents_hq(&p, &d);
        hq.start().await.expect("starts");
        hq.set_server(&agents.base).await.expect("sets");
        for _ in 0..100 {
            if !asked(&agents, "/devices/enroll").is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;

        assert_eq!(asked(&agents, "/devices/enroll"), ["Bearer access-2"]);
        assert_eq!(fake.refreshes(), 1, "one mint, no loop");
        assert_eq!(asked(&agents, "/auth/config").len(), 1);
        assert_eq!(
            sign_in_mark(&org(&p, &d)),
            before,
            "a refresh is the same sign-in"
        );
        hq.shutdown().await;
    }

    #[tokio::test]
    async fn a_refresh_without_auth_time_still_lets_the_attestation_go() {
        let fake = start_fake(Behaviour {
            refresh_claims: Some(serde_json::json!({ "auth_time": null })),
            ..Behaviour::default()
        });
        let p = Arc::new(FakePlatform::new(&fake, Browser::Honest));
        let d = account(&fake, "attest-no-auth-time", None);
        seed_session(&p, &d, Vec::new(), now_ms() + 3_600_000);
        let agents = agents_server(&d);
        let server = crate::hq::server::ServerUrl::parse(&agents.base)
            .expect("parses")
            .expect("an address");
        crate::registry::set_agents_server_url(&p.data_dir().expect("dir"), Some(&server))
            .expect("stores");
        let day = 24 * 60 * 60 * 1000;
        // Signed in six and a half days ago: the attestation is due.
        p.keychain_set(
            &format!("agents-server/{}", server.as_str()),
            &serde_json::json!({
                "token": "device-token-a",
                "actorId": "ana",
                "clientId": "ana.1",
                "name": "keeper",
                "bound": { "attestedAtMs": now_ms() - 13 * day / 2, "windowMs": 7 * day },
            })
            .to_string(),
        )
        .expect("keychain");
        let hq = agents_hq(&p, &d);
        hq.start().await.expect("starts");
        for _ in 0..100 {
            if !asked(&agents, "/clients/self/attest").is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        assert_eq!(asked(&agents, "/clients/self/attest"), ["Bearer access-2"]);
        assert_eq!(fake.refreshes(), 1);
        hq.shutdown().await;
    }

    #[tokio::test]
    async fn a_rejected_refresh_needs_sign_in_and_an_unreachable_provider_does_not() {
        let fake = start_fake(Behaviour {
            reject_refresh: true,
            ..Behaviour::default()
        });
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "dead-grant", None);
        seed_expired_session(&p, &d);
        let dead = access_token(&p, &http(), &d).await;
        assert!(
            matches!(dead, Err(AccountError::NeedsSignIn(_))),
            "{dead:?}"
        );

        // Nothing listens on a port just released.
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .expect("port")
            .port();
        let offline = Fake {
            issuer: format!("http://127.0.0.1:{port}"),
            seen: Arc::default(),
        };
        let d = account(&offline, "offline", None);
        seed_expired_session(&p, &d);
        let unreachable = access_token(&p, &http(), &d).await;
        assert!(
            matches!(unreachable, Err(AccountError::Unreachable(_))),
            "{unreachable:?}"
        );
        assert!(
            p.session("offline").is_some(),
            "being offline signs nobody out"
        );
    }

    #[tokio::test]
    async fn no_session_needs_sign_in() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "nobody", None);
        let result = access_token(&p, &http(), &d).await;
        assert!(
            matches!(result, Err(AccountError::NeedsSignIn(_))),
            "{result:?}"
        );
        assert!(fake.token_requests().is_empty());
    }

    // ---- forge_connect ---------------------------------------------------

    const FORGE_SCOPE: &str = "openid profile read:user write:repository";

    fn forge_account(fake: &Fake, id: &str) -> AccountDescriptor {
        forge_account_scoped(fake, id, FORGE_SCOPE)
    }

    fn forge_account_scoped(fake: &Fake, id: &str, scope: &str) -> AccountDescriptor {
        let i = &fake.issuer;
        descriptor::parse_json(&format!(
            r#"{{"version":1,"id":"{id}","name":"Acme",
                "auth":{{"issuer":"{i}","client_id":"{CLIENT_ID}"}},
                "config":{{"url":"{i}/c.git","api_base":"{i}/api/v1",
                  "auth":{{"mode":"oauth","client_id":"forge-client","scope":"{scope}",
                           "authorize_url":"{i}/authorize","token_url":"{i}/token",
                           "signin_url":"{i}/user/oauth2/acme?redirect_to={{authorize_path_and_query}}"}}}}}}"#
        ))
        .expect("a valid oauth-mode descriptor")
    }

    #[tokio::test]
    async fn forge_connect_goes_through_the_signin_url_and_stores_the_forge_token() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = forge_account(&fake, "forge");

        forge_connect_within(&p, &p.flows, &http(), &d, "tgorka", "hesperia", ARRIVES)
            .await
            .expect("connected");

        let sheet = p.shown.lock().expect("shown")[0].0.clone();
        let link = p.links.lock().expect("links")[0]
            .clone()
            .expect("the link on screen");
        assert_eq!(
            link.url, sheet,
            "the link to copy by hand is the page the sheet or browser was given"
        );
        assert_eq!(
            forge_reads(&sheet).len(),
            1,
            "the forge reads one parameter, redirect_to"
        );
        let shown = Url::parse(&sheet).expect("url");
        assert_eq!(shown.path(), "/user/oauth2/acme");
        let authorize = shown.join(&one(&shown, "redirect_to")).expect("inner");
        assert_eq!(
            one(&authorize, "scope"),
            FORGE_SCOPE,
            "the scope string is sent as written"
        );
        assert_eq!(one(&authorize, "client_id"), "forge-client");
        assert_eq!(
            one(&authorize, "redirect_uri"),
            "keeper://oauth/forge/forge/callback"
        );
        assert!(fake.token_requests()[0].contains_key("code_verifier"));
        assert!(session::forge_connected(&p, &d));
        assert_eq!(
            forge_token(&p, &http(), &d).await.expect("token"),
            "access-1"
        );
    }

    #[tokio::test]
    async fn a_forge_signed_in_as_someone_else_is_refused_and_its_token_discarded() {
        let fake = start_fake(Behaviour {
            forge_login: Some("ana".to_owned()),
            ..Behaviour::default()
        });
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = forge_account(&fake, "forge-ana");

        let result =
            forge_connect_within(&p, &p.flows, &http(), &d, "tgorka", "hesperia", ARRIVES).await;

        match result {
            Err(AccountError::Refused(sentence)) => assert_eq!(
                sentence,
                "The forge signed you in as ana, but your keeper account is tgorka."
            ),
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(!session::forge_connected(&p, &d));
    }

    // ---- sign-out and the providers' credential -------------------------

    #[tokio::test]
    async fn sign_out_revokes_the_refresh_token_deletes_the_items_and_returns_the_logout_request() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "bye", None);
        sign_in_at_fake(&p, &d).await.expect("sign in");
        p.keychain_set(&forge_key("bye"), "{}")
            .expect("a forge item");

        let logout = sign_out(&p, &http(), &d).await.expect("signed out");

        let logout = logout.expect("the provider publishes end_session");
        assert_eq!(logout.callback_scheme, "keeper");
        let url = Url::parse(&logout.url).expect("url");
        assert_eq!(url.path(), "/logout");
        assert_eq!(one(&url, "client_id"), CLIENT_ID);
        assert!(!one(&url, "id_token_hint").is_empty());
        assert_eq!(
            one(&url, "post_logout_redirect_uri"),
            "keeper://oauth/bye/signed-out"
        );
        assert!(logout.state.len() >= 16);
        assert_eq!(one(&url, "state"), logout.state);
        let revocations = fake.seen.lock().expect("seen").revocations.clone();
        assert_eq!(revocations.len(), 1);
        assert_eq!(
            revocations[0].get("token").map(String::as_str),
            Some("refresh-1")
        );
        assert!(p.session("bye").is_none());
        assert!(p.keychain_get(&forge_key("bye")).expect("read").is_none());
    }

    #[tokio::test]
    async fn a_provider_opted_into_the_account_sends_the_account_token() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "providers", None);
        sign_in_at_fake(&p, &d).await.expect("sign in");
        crate::providers::save_provider_token(&p, "prov", "keychain-token").expect("save");

        let before = crate::providers::resolve_credential(&p, &http(), Some(&d), "prov").await;
        assert_eq!(before.expect("keychain"), Some("keychain-token".to_owned()));

        crate::registry::set_bots_provider_credential_source(
            &p.data_dir,
            "prov",
            Some("account"),
            Some("providers"),
        )
        .expect("opt in");
        let after = crate::providers::resolve_credential(&p, &http(), Some(&d), "prov").await;
        assert_eq!(after.expect("account"), Some("access-1".to_owned()));

        // Another account now configured: the row is not its to answer.
        let other = account(&fake, "globex", None);
        let foreign = crate::providers::resolve_credential(&p, &http(), Some(&other), "prov").await;
        assert_eq!(
            foreign.expect("keychain"),
            Some("keychain-token".to_owned())
        );
    }

    #[tokio::test]
    async fn no_account_resolves_a_bot_credential_without_touching_the_database() {
        let fake = start_fake(Behaviour::default());
        let mut p = FakePlatform::new(&fake, Browser::Honest);
        crate::providers::save_provider_token(&p, "prov", "keychain-token").expect("save");
        // A data dir no database can open: any registry read would fail.
        let file = p.data_dir.join("not-a-dir");
        std::fs::write(&file, "x").expect("file");
        let real = std::mem::replace(&mut p.data_dir, file);

        let token = crate::providers::resolve_credential(&p, &http(), None, "prov").await;

        p.data_dir = real;
        assert_eq!(token.expect("keychain"), Some("keychain-token".to_owned()));
    }

    // ---- fix-wave regressions -------------------------------------------

    #[tokio::test]
    async fn an_item_from_another_sign_in_setup_under_the_same_id_is_never_used() {
        let a = start_fake(Behaviour::default());
        let b = start_fake(Behaviour::default());
        let p = FakePlatform::new(&a, Browser::Honest);
        let at_a = account(&a, "same-id", None);
        let at_b = account(&b, "same-id", None);
        sign_in_at_fake(&p, &at_a).await.expect("sign in at A");

        // Fresh token, same id, another issuer: nothing is handed over.
        let result = access_token(&p, &http(), &at_b).await;
        assert!(
            matches!(result, Err(AccountError::NeedsSignIn(_))),
            "{result:?}"
        );
        assert!(
            p.session("same-id").is_none(),
            "the foreign item is deleted"
        );

        // The identity read on restore follows the same rule.
        sign_in_at_fake(&p, &at_a)
            .await
            .expect("sign in at A again");
        assert!(matches!(
            session::identity(&p, &at_b),
            Err(AccountError::NeedsSignIn(_))
        ));

        // Sign-out under B sends A's refresh token nowhere, and still deletes.
        sign_in_at_fake(&p, &at_a)
            .await
            .expect("sign in at A again");
        sign_out(&p, &http(), &at_b).await.expect("signed out");
        assert!(a.seen.lock().expect("seen").revocations.is_empty());
        assert!(b.seen.lock().expect("seen").revocations.is_empty());
        assert!(p.session("same-id").is_none());
        assert!(b.token_requests().is_empty(), "B never saw a token");
    }

    #[tokio::test]
    async fn a_forge_item_for_another_repository_is_not_connected() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = forge_account(&fake, "forge-moved");
        forge_connect_within(&p, &p.flows, &http(), &d, "tgorka", "hesperia", ARRIVES)
            .await
            .expect("connected");
        let mut moved = d.clone();
        moved.config.url = format!("{}/elsewhere.git", fake.issuer);

        assert!(!session::forge_connected(&p, &moved));
        let result = forge_token(&p, &http(), &moved).await;
        assert!(
            matches!(result, Err(AccountError::NeedsSignIn(_))),
            "{result:?}"
        );
        assert!(p
            .keychain_get(&forge_key("forge-moved"))
            .expect("read")
            .is_none());
    }

    #[tokio::test]
    async fn a_different_person_signing_in_drops_the_previous_forge_token() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = forge_account(&fake, "handover");
        sign_in_at_fake(&p, &d).await.expect("sign in");
        forge_connect_within(&p, &p.flows, &http(), &d, "tgorka", "hesperia", ARRIVES)
            .await
            .expect("connected");

        // The same person again keeps the forge.
        sign_in_at_fake(&p, &d).await.expect("sign in again");
        assert!(session::forge_connected(&p, &d));

        // Someone else was signed in before: their forge token goes.
        let mut other = load_session(&p, "handover")
            .expect("read")
            .expect("session");
        other.sub = "someone-else".to_owned();
        store_session(&p, "handover", &other).expect("store");
        sign_in_at_fake(&p, &d).await.expect("sign in as sub-1");
        assert!(!session::forge_connected(&p, &d));
    }

    #[tokio::test]
    async fn a_rotated_refresh_token_is_kept_when_the_refreshed_id_token_fails_verification() {
        let fake = start_fake(Behaviour {
            refresh_claims: Some(serde_json::json!({})),
            refresh_kid: Some("rotated-away"),
            ..Behaviour::default()
        });
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "rotate-fail", None);
        seed_expired_session(&p, &d);

        let result = access_token(&p, &http(), &d).await;

        assert!(
            matches!(result, Err(AccountError::NeedsSignIn(_))),
            "{result:?}"
        );
        let session = p.session("rotate-fail").expect("session");
        assert_eq!(
            session["refresh_token"], "refresh-2",
            "the provider already retired refresh-1"
        );
        assert_eq!(
            session["access_token"], "access-1",
            "the unverified token is not used"
        );
    }

    fn role_account(fake: &Fake, id: &str) -> AccountDescriptor {
        descriptor::parse_json(&format!(
            r#"{{"version":1,"id":"{id}","name":"Acme",
                "auth":{{"issuer":"{}","client_id":"{CLIENT_ID}",
                         "roles_claim":"groups","required_role":"keeper"}},
                "config":{{"url":"https://git.acme.dev/c.git"}}}}"#,
            fake.issuer
        ))
        .expect("a valid test descriptor")
    }

    #[tokio::test]
    async fn a_refresh_that_no_longer_carries_the_required_role_is_refused_and_remembered() {
        let fake = start_fake(Behaviour {
            refresh_claims: Some(serde_json::json!({ "groups": ["other"] })),
            ..Behaviour::default()
        });
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = role_account(&fake, "role-gone");
        seed_session(&p, &d, vec!["keeper".to_owned()], now_ms() - 1);

        let result = access_token(&p, &http(), &d).await;

        assert!(
            matches!(&result, Err(AccountError::Refused(m)) if m.contains("keeper role")),
            "{result:?}"
        );
        let session = p.session("role-gone").expect("session");
        assert_eq!(session["roles"], serde_json::json!(["other"]));
        assert!(
            matches!(session::identity(&p, &d), Err(AccountError::Refused(_))),
            "a restore must not install the account's layers"
        );
    }

    #[tokio::test]
    async fn a_refresh_that_still_carries_the_role_passes_and_stores_the_new_token() {
        let fake = start_fake(Behaviour {
            refresh_claims: Some(serde_json::json!({ "groups": ["keeper", "admin"] })),
            ..Behaviour::default()
        });
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = role_account(&fake, "role-kept");
        seed_session(&p, &d, vec!["keeper".to_owned()], now_ms() - 1);

        assert_eq!(
            access_token(&p, &http(), &d).await.expect("refreshed"),
            "access-2"
        );
        let session = p.session("role-kept").expect("session");
        assert_eq!(session["roles"], serde_json::json!(["admin", "keeper"]));
    }

    #[tokio::test]
    async fn stored_roles_are_held_to_the_descriptor_s_current_required_role() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = role_account(&fake, "role-restore");
        // Signed in before the descriptor required a role; still fresh.
        seed_session(&p, &d, Vec::new(), now_ms() + 3_600_000);

        assert!(matches!(
            session::identity(&p, &d),
            Err(AccountError::Refused(_))
        ));
        let token = access_token(&p, &http(), &d).await;
        assert!(matches!(token, Err(AccountError::Refused(_))), "{token:?}");
        assert!(fake.token_requests().is_empty());
    }

    fn audience_account(fake: &Fake, id: &str) -> AccountDescriptor {
        descriptor::parse_json(&format!(
            r#"{{"version":1,"id":"{id}","name":"Acme",
                "auth":{{"issuer":"{}","client_id":"{CLIENT_ID}","trusted_audiences":["project"]}},
                "config":{{"url":"https://git.acme.dev/c.git"}}}}"#,
            fake.issuer
        ))
        .expect("a valid test descriptor")
    }

    #[tokio::test]
    async fn a_refreshed_id_token_must_keep_the_audience_and_auth_time() {
        for (id, patch) in [
            (
                "aud-changed",
                serde_json::json!({ "aud": [CLIENT_ID, "project"] }),
            ),
            (
                "auth-time-changed",
                serde_json::json!({ "auth_time": 2_000 }),
            ),
            ("sub-changed", serde_json::json!({ "sub": "sub-2" })),
        ] {
            let fake = start_fake(Behaviour {
                refresh_claims: Some(patch),
                ..Behaviour::default()
            });
            let p = FakePlatform::new(&fake, Browser::Honest);
            let d = audience_account(&fake, id);
            seed_expired_session(&p, &d);

            let result = access_token(&p, &http(), &d).await;

            assert!(
                matches!(result, Err(AccountError::NeedsSignIn(_))),
                "{id}: {result:?}"
            );
        }

        // The same claims again (auth_time kept, nonce absent) are fine.
        let fake = start_fake(Behaviour {
            refresh_claims: Some(serde_json::json!({})),
            ..Behaviour::default()
        });
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = audience_account(&fake, "unchanged");
        seed_expired_session(&p, &d);
        assert_eq!(
            access_token(&p, &http(), &d).await.expect("refreshed"),
            "access-2"
        );
    }

    /// A provider behind a network that drops the packets (a tailnet ACL
    /// without a grant) accepts nothing and refuses nothing. The sign-in
    /// stops at `REACH_TIMEOUT` with a sentence naming the host and port,
    /// not at the browser's five-minute timeout with none — before the flow
    /// exists, so no browser opens and no link is shown. Paused time: the
    /// provider never answers, so the budget is all that ends it.
    #[tokio::test(start_paused = true)]
    async fn a_provider_that_never_answers_is_named_with_its_port_before_any_browser() {
        // Accepts the connection (the kernel does) and never answers.
        let silent = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = silent.local_addr().expect("addr");
        let fake = Fake {
            issuer: format!("http://{address}"),
            seen: Arc::default(),
        };
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "acme", None);

        // Bounded well past the budget: without it the sign-in would wait out
        // the browser's timeout.
        let error = tokio::time::timeout(
            Duration::from_secs(60),
            sign_in_within(&p, &p.flows, &http(), &d, FLOW_TIMEOUT),
        )
        .await
        .expect("the sign-in ended on its own budget")
        .expect_err("nobody answered");

        assert_eq!(
            error.to_string(),
            format!(
                "Can't reach {address}: it did not answer within 10 seconds. \
                 Check that this device is on a network (or VPN) that can reach it."
            )
        );
        assert!(matches!(error, AccountError::NoAnswer(_)), "{error:?}");
        assert!(
            p.shown.lock().expect("shown").is_empty(),
            "no browser opened"
        );
        assert!(p.links.lock().expect("links").is_empty(), "no link shown");
        drop(silent);
    }

    /// The repository's forge leg has the same budget when its forge names
    /// an issuer to discover: the sentence names the forge's host and port.
    #[tokio::test(start_paused = true)]
    async fn a_forge_that_never_answers_is_named_with_its_port_before_any_browser() {
        let silent = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = silent.local_addr().expect("addr");
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Honest);
        let i = &fake.issuer;
        let d = descriptor::parse_json(&format!(
            r#"{{"version":1,"id":"forge","name":"Acme",
                "auth":{{"issuer":"{i}","client_id":"{CLIENT_ID}"}},
                "config":{{"url":"{i}/c.git","api_base":"{i}/api/v1",
                  "auth":{{"mode":"oauth","client_id":"forge-client","scope":"{FORGE_SCOPE}",
                           "issuer":"http://{address}"}}}}}}"#
        ))
        .expect("a valid oauth-mode descriptor");

        let error = tokio::time::timeout(
            Duration::from_secs(60),
            forge_connect_within(&p, &p.flows, &http(), &d, "tgorka", "hesperia", ARRIVES),
        )
        .await
        .expect("the connection ended on its own budget")
        .expect_err("nobody answered");

        assert!(matches!(error, AccountError::NoAnswer(_)), "{error:?}");
        assert_eq!(
            error.to_string(),
            crate::oauth::no_answer(&address.to_string())
        );
        assert!(p.shown.lock().expect("shown").is_empty());
        assert!(p.links.lock().expect("links").is_empty());
        drop(silent);
    }

    /// R541-03: the issuer answers its discovery document, and the key set
    /// it names lives on another host and port, which takes the request and
    /// never answers. The sentence names the key set's server — the request
    /// that stalled — not the issuer that answered. Time is paused once the
    /// key set is asked: the budget is all that ends it.
    #[tokio::test]
    async fn a_key_set_that_never_answers_is_named_not_the_issuer_that_did() {
        let held = TcpListener::bind("127.0.0.1:0").expect("bind the key set");
        let keys = held.local_addr().expect("addr");
        let (asked_tx, asked) = tokio::sync::oneshot::channel::<()>();
        let (release, released) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = held.accept() else {
                return;
            };
            let _ = read_request(&mut stream);
            let _ = asked_tx.send(());
            let _ = released.recv();
        });
        let fake = start_fake(Behaviour {
            discovery_patch: Some(("jwks_uri", format!("http://{keys}/jwks"))),
            ..Behaviour::default()
        });
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "keys", None);
        let client = http();

        let sign_in = tokio::time::timeout(
            Duration::from_secs(60),
            sign_in_within(&p, &p.flows, &client, &d, FLOW_TIMEOUT),
        );
        let hold = async {
            asked.await.expect("the key set was asked");
            tokio::time::pause();
        };
        let (result, ()) = tokio::join!(sign_in, hold);
        let error = result
            .expect("the sign-in ended on its own budget")
            .expect_err("the key set never answered");

        assert!(matches!(error, AccountError::NoAnswer(_)), "{error:?}");
        assert_eq!(
            error.to_string(),
            crate::oauth::no_answer(&keys.to_string())
        );
        assert!(
            p.shown.lock().expect("shown").is_empty(),
            "no browser opened"
        );
        assert!(p.links.lock().expect("links").is_empty(), "no link shown");
        drop(release);
    }

    #[tokio::test]
    async fn a_discovery_server_error_is_unreachable_not_a_policy_refusal() {
        let fake = start_fake(Behaviour {
            discovery_status: Some("503 Service Unavailable"),
            ..Behaviour::default()
        });
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "flaky", None);

        let result = sign_in_at_fake(&p, &d).await;

        assert!(
            matches!(result, Err(AccountError::Unreachable(_))),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn a_discovered_endpoint_that_is_not_https_is_refused_before_the_browser_opens() {
        for (field, value) in [
            ("authorization_endpoint", "file:///etc/passwd".to_owned()),
            ("token_endpoint", "http://evil.example/token".to_owned()),
            (
                "end_session_endpoint",
                "smb://evil.example/share".to_owned(),
            ),
        ] {
            let fake = start_fake(Behaviour {
                discovery_patch: Some((field, value)),
                ..Behaviour::default()
            });
            let p = FakePlatform::new(&fake, Browser::Honest);
            let d = account(&fake, "insecure", None);

            let result = sign_in_at_fake(&p, &d).await;

            assert!(
                matches!(result, Err(AccountError::Refused(_))),
                "{field}: {result:?}"
            );
            assert!(p.shown.lock().expect("shown").is_empty(), "{field}");
            assert!(fake.token_requests().is_empty(), "{field}");
        }
    }

    #[tokio::test]
    async fn a_token_without_expires_in_is_refreshed_after_ten_minutes() {
        let fake = start_fake(Behaviour {
            omit_expires_in: true,
            ..Behaviour::default()
        });
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = account(&fake, "no-expiry", None);

        sign_in_at_fake(&p, &d).await.expect("sign in");

        let expires = p.session("no-expiry").expect("session")["access_expires_ms"]
            .as_i64()
            .expect("an expiry is recorded");
        let ahead = expires - now_ms();
        assert!(
            (9 * 60_000..=10 * 60_000).contains(&ahead),
            "{ahead} ms ahead"
        );
    }

    #[tokio::test]
    async fn a_forced_forge_refresh_replaces_an_unexpired_token() {
        let fake = start_fake(Behaviour::default());
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = forge_account(&fake, "forge-forced");
        forge_connect_within(&p, &p.flows, &http(), &d, "tgorka", "hesperia", ARRIVES)
            .await
            .expect("connected");
        assert_eq!(
            forge_token(&p, &http(), &d).await.expect("cached"),
            "access-1"
        );

        assert_eq!(
            forge_token_refreshed(&p, &http(), &d)
                .await
                .expect("refreshed"),
            "access-2"
        );
        assert_eq!(
            forge_token(&p, &http(), &d).await.expect("stored"),
            "access-2"
        );
    }

    /// A forge that refuses the refresh token has killed it — another device's
    /// sign-in to the same Forgejo grant does exactly that. The dead item must
    /// not keep reading as "connected", or the next sign-in skips the forge leg.
    #[tokio::test]
    async fn a_refused_forge_refresh_forgets_the_forge_token() {
        let fake = start_fake(Behaviour {
            reject_refresh: true,
            ..Behaviour::default()
        });
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = forge_account(&fake, "forge-dead");
        forge_connect_within(&p, &p.flows, &http(), &d, "tgorka", "hesperia", ARRIVES)
            .await
            .expect("connected");
        assert!(
            session::forge_connected(&p, &d),
            "connected before the refresh"
        );

        let refused = forge_token_refreshed(&p, &http(), &d).await;

        assert!(
            matches!(refused, Err(AccountError::NeedsSignIn(_))),
            "{refused:?}"
        );
        assert!(
            !session::forge_connected(&p, &d),
            "a refused refresh token is forgotten, so the next sign-in reconnects"
        );
    }

    // ---- several devices of one person -----------------------------------

    /// A forge scope whose token may register an application.
    const DEVICE_SCOPE: &str = "openid profile write:repository write:user";

    fn forgejo() -> Fake {
        start_fake(Behaviour {
            forgejo: true,
            ..Behaviour::default()
        })
    }

    async fn connect_as(p: &FakePlatform, d: &AccountDescriptor, device: &str) {
        forge_connect_within(p, &p.flows, &http(), d, "tgorka", device, ARRIVES)
            .await
            .unwrap_or_else(|e| panic!("{device} connected: {e:?}"));
    }

    fn app_names(fake: &Fake) -> Vec<String> {
        let seen = fake.seen.lock().expect("seen");
        seen.apps
            .iter()
            .map(|app| app["name"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    /// Two devices of one person, each with its own keychain, against one
    /// Forgejo: each holds a grant of its own application, so neither's
    /// sign-in nor refresh retires the other's refresh token — under one
    /// shared grant, kalypso's sign-in would have ended hesperia's.
    #[tokio::test]
    async fn two_devices_hold_working_forge_credentials_at_once() {
        let fake = forgejo();
        let hesperia = FakePlatform::new(&fake, Browser::Honest);
        let kalypso = FakePlatform::new(&fake, Browser::Honest);
        let d = forge_account_scoped(&fake, "two-devices", DEVICE_SCOPE);

        connect_as(&hesperia, &d, "hesperia").await;
        connect_as(&kalypso, &d, "kalypso").await;

        for round in 0..3 {
            for (name, p) in [("hesperia", &hesperia), ("kalypso", &kalypso)] {
                let token = forge_token_refreshed(p, &http(), &d).await;
                assert!(token.is_ok(), "{name}, round {round}: {token:?}");
                assert!(session::forge_connected(p, &d), "{name}, round {round}");
            }
        }

        let names = app_names(&fake);
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names[0].starts_with("keeper (hesperia, "), "{names:?}");
        assert!(names[1].starts_with("keeper (kalypso, "), "{names:?}");
        for app in &fake.seen.lock().expect("seen").apps {
            assert_eq!(
                app["redirect_uris"],
                serde_json::json!(["keeper://oauth/two-devices/forge/callback"])
            );
            assert_eq!(app["confidential_client"], false, "a public client");
        }
        let client = |p: &FakePlatform| {
            load_forge(p, &d.id)
                .expect("read")
                .and_then(|f| f.device_app)
                .map(|app| app.client_id)
        };
        assert_eq!(client(&hesperia).as_deref(), Some("device-1"));
        assert_eq!(client(&kalypso).as_deref(), Some("device-2"));
        for refresh in fake
            .token_requests()
            .iter()
            .filter(|r| r.get("grant_type").map(String::as_str) == Some("refresh_token"))
        {
            assert!(
                refresh
                    .get("client_id")
                    .is_some_and(|c| c.starts_with("device-")),
                "a refresh under the shared client: {refresh:?}"
            );
        }
    }

    /// The bootstrap asks for the descriptor's scope byte for byte (Forgejo
    /// refuses any other against an existing grant); the device's own
    /// application asks for it without `write:user`, and only its token is
    /// kept.
    #[tokio::test]
    async fn the_kept_token_is_the_device_s_own_without_write_user() {
        let fake = forgejo();
        let p = FakePlatform::new(&fake, Browser::Honest);
        let d = forge_account_scoped(&fake, "scopes", DEVICE_SCOPE);

        connect_as(&p, &d, "hesperia").await;

        let authorizations: Vec<(String, String)> = p
            .shown
            .lock()
            .expect("shown")
            .iter()
            .map(|(url, _)| {
                let shown = Url::parse(url).expect("url");
                let inner = shown.join(&one(&shown, "redirect_to")).expect("inner");
                (one(&inner, "client_id"), one(&inner, "scope"))
            })
            .collect();
        assert_eq!(
            authorizations,
            vec![
                ("forge-client".to_owned(), DEVICE_SCOPE.to_owned()),
                (
                    "device-1".to_owned(),
                    "openid profile write:repository read:user".to_owned()
                ),
            ]
        );
        let stored = load_forge(&p, &d.id).expect("read").expect("item");
        assert_eq!(stored.access_token, "fa.device-1.1.-");
        assert_eq!(stored.refresh_token.as_deref(), Some("fr.device-1.1"));
    }

    /// A scope without `write:user` registers nothing: the device keeps the
    /// shared client, another device's sign-in ends its connection, the
    /// refusal forgets the item, and the next sign-in reconnects.
    #[tokio::test]
    async fn without_write_user_the_devices_share_one_grant_and_the_loser_can_reconnect() {
        let fake = forgejo();
        let hesperia = FakePlatform::new(&fake, Browser::Honest);
        let kalypso = FakePlatform::new(&fake, Browser::Honest);
        let d = forge_account(&fake, "shared-grant");

        connect_as(&hesperia, &d, "hesperia").await;
        connect_as(&kalypso, &d, "kalypso").await;

        assert!(app_names(&fake).is_empty());
        let stored = load_forge(&hesperia, &d.id).expect("read").expect("item");
        assert!(stored.device_app.is_none());
        let lost = forge_token_refreshed(&hesperia, &http(), &d).await;
        assert!(
            matches!(lost, Err(AccountError::NeedsSignIn(_))),
            "{lost:?}"
        );
        assert!(!session::forge_connected(&hesperia, &d));
        forge_token_refreshed(&kalypso, &http(), &d)
            .await
            .expect("the last device to sign in holds the grant");

        connect_as(&hesperia, &d, "hesperia").await;
        assert!(forge_token_refreshed(&hesperia, &http(), &d).await.is_ok());
    }

    /// A person who closes the second sheet ends the connection, and the
    /// application registered for it is deleted with the bootstrap token.
    #[tokio::test]
    async fn closing_the_device_s_own_sign_in_deletes_the_application_it_registered() {
        let fake = forgejo();
        let p = FakePlatform::new(&fake, Browser::HonestThenClosed);
        let d = forge_account_scoped(&fake, "closed-second", DEVICE_SCOPE);

        let result =
            forge_connect_within(&p, &p.flows, &http(), &d, "tgorka", "hesperia", ARRIVES).await;

        assert!(matches!(result, Err(AccountError::Cancelled)), "{result:?}");
        assert_eq!(app_names(&fake).len(), 1);
        assert_eq!(fake.seen.lock().expect("seen").deleted_apps, vec!["1"]);
        assert!(!session::forge_connected(&p, &d));
    }

    #[test]
    fn the_device_scope_narrows_write_user_only_beside_another_api_scope() {
        assert_eq!(
            device_scope("openid profile write:repository write:user"),
            "openid profile write:repository read:user"
        );
        assert_eq!(
            device_scope("openid profile read:user write:user write:repository"),
            "openid profile read:user write:repository"
        );
        // Without another API scope, dropping it would mean every scope.
        assert_eq!(
            device_scope("openid profile write:user"),
            "openid profile write:user"
        );
        assert_eq!(device_scope("openid profile"), "openid profile");
        assert_eq!(
            device_scope("openid profile read:user write:repository"),
            "openid profile read:user write:repository"
        );
    }

    // ---- pure helpers ----------------------------------------------------

    /// makistack's descriptor's `signin_url`, as published to the devices.
    const MAKISTACK_SIGNIN: &str =
        "https://electra.siren-alsephina.ts.net/git/user/oauth2/zitadel?redirect_to={authorize_path_and_query}";

    /// The query of a filled `signin_url`, split the way a forge splits it:
    /// as a form (Go's `url.ParseQuery`, which Forgejo uses).
    fn forge_reads(filled: &str) -> Vec<(String, String)> {
        let (_, query) = filled.split_once('?').expect("a query");
        url::form_urlencoded::parse(query.as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    /// The request kalypso sent at 08:22:28Z on 2026-10-28 (Forgejo's router
    /// log; `state`, `code_challenge` and `nonce` replaced). Unencoded, the
    /// forge kept `redirect_to=/git/login/oauth/authorize?response_type=code`
    /// and answered the callback with "Client ID not registered". Built by
    /// keeper from the same template, it is one `redirect_to` carrying the
    /// whole authorization request.
    #[test]
    fn the_request_from_the_forge_log_comes_out_as_one_encoded_redirect_to() {
        let inner = "/git/login/oauth/authorize?response_type=code\
             &client_id=232f4981-7dcc-4d3c-9ded-39af2a8a851c&state=S7a-_b&code_challenge=C9x-_y\
             &code_challenge_method=S256&redirect_uri=keeper%3A%2F%2Foauth%2Fforgejo%2Fcallback\
             &scope=openid+profile+write%3Arepository&nonce=N3c-_d";
        let authorize =
            Url::parse(&format!("https://electra.siren-alsephina.ts.net{inner}")).expect("url");

        let filled = fill_signin_url(MAKISTACK_SIGNIN, &authorize);

        assert_eq!(
            filled,
            "https://electra.siren-alsephina.ts.net/git/user/oauth2/zitadel?redirect_to=\
             %2Fgit%2Flogin%2Foauth%2Fauthorize%3Fresponse_type%3Dcode\
             %26client_id%3D232f4981-7dcc-4d3c-9ded-39af2a8a851c%26state%3DS7a-_b\
             %26code_challenge%3DC9x-_y%26code_challenge_method%3DS256\
             %26redirect_uri%3Dkeeper%253A%252F%252Foauth%252Fforgejo%252Fcallback\
             %26scope%3Dopenid%2Bprofile%2Bwrite%253Arepository%26nonce%3DN3c-_d"
        );
        assert_eq!(
            forge_reads(&filled),
            vec![("redirect_to".to_owned(), inner.to_owned())],
            "the forge sees one parameter, and the authorization request whole inside it"
        );
    }

    /// `&`, `#`, `%`, `+`, `=`, `?`, `/` and a space inside the authorization
    /// request come back out of `redirect_to` byte for byte — read as a form
    /// (Forgejo) or as plain RFC 3986 percent-encoding — and the parameter
    /// they sit in reads back as it was sent.
    #[test]
    fn reserved_characters_in_the_authorize_request_round_trip_through_redirect_to() {
        let state = "a&b#c%d+e=f?g/h i~j*k";
        let mut authorize =
            Url::parse("https://git.acme.dev/git/login/oauth/authorize").expect("url");
        authorize
            .query_pairs_mut()
            .append_pair("client_id", "c")
            .append_pair("state", state);
        let inner = format!(
            "{}?{}",
            authorize.path(),
            authorize.query().expect("a query")
        );

        let filled = fill_signin_url(
            "https://git.acme.dev/git/user/oauth2/acme-id?redirect_to={authorize_path_and_query}",
            &authorize,
        );

        let (_, raw) = filled
            .split_once("?redirect_to=")
            .expect("the placeholder's spot");
        assert!(
            raw.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~%".contains(&b)),
            "only unreserved characters and escapes: {raw}"
        );
        assert_eq!(
            forge_reads(&filled),
            vec![("redirect_to".to_owned(), inner.clone())]
        );
        assert_eq!(
            percent_encoding::percent_decode_str(raw)
                .decode_utf8()
                .expect("utf-8"),
            inner,
            "a forge decoding RFC 3986 rather than a form reads the same request"
        );
        let back = authorize.join(&inner).expect("the inner request");
        assert_eq!(one(&back, "state"), state);
        assert_eq!(one(&back, "client_id"), "c");
    }
}
