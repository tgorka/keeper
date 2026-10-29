//! The server's routes outside syncular's own protocol: at the root,
//! discovery (`GET /databases`), pairing (`POST /pair/start`,
//! `POST /pair/redeem`), sign-in enrolment (`GET /auth/config`,
//! `POST /devices/enroll`, `POST /clients/self/attest`,
//! `GET /clients/self/enrolments`), revocation (`POST /clients/{id}/revoke`)
//! and this device's push target (`PUT`/`DELETE /clients/self/push-target`);
//! in a database's mount, the one item a push names
//! (`GET /<database>/push/item/{kind}/{id}`). Each request carries one
//! credential as a bearer and nothing else of it — the device's token, a
//! pairing code, or at the enrol and attest routes alone the account's
//! sign-in token (S-ID-1), never in a URL (S-ID-2); no response body is ever
//! logged, because a redeem's or an enrolment's body is the token.

use serde::Deserialize;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::hq::pairing::PairingCode;
use crate::hq::push::{PushTarget, PushedItem};
use crate::hq::server::ServerUrl;

/// How long one root request may take. Discovery runs at every start, so a
/// firewalled server must not hold the status on *connecting* for minutes.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// What a request to a root route came back as.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouteError {
    /// 401: the token (or a redeemed code) is unknown, expired or revoked.
    #[error("the server does not know this device")]
    Unauthorized,
    /// 403 with the server's code.
    #[error("the server refused: {0}")]
    Forbidden(String),
    /// A sign-in route's refusal (400, 401, 403, 404, 409 or 503) with the
    /// server's code: which one decides what keeper does next.
    #[error("the server refused ({status}): {code}")]
    Refused { status: u16, code: String },
    /// Connection, TLS or timeout: nothing was decided.
    #[error("the server could not be reached: {0}")]
    Unreachable(String),
    /// Any other answer, or a body keeper cannot read.
    #[error("the server answered something keeper cannot read: {0}")]
    Unexpected(String),
}

/// The databases a token opens and whom it was issued to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    pub actor_id: String,
    pub client_id: String,
    pub databases: Vec<String>,
}

/// A redeemed code: the token (shown once) and what it opens.
#[derive(Clone, PartialEq, Eq)]
pub struct Redeemed {
    pub token: String,
    pub discovery: Discovery,
}

impl std::fmt::Debug for Redeemed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Redeemed")
            .field("token", &"<redacted>")
            .field("discovery", &self.discovery)
            .finish()
    }
}

/// A fresh code from `POST /pair/start`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issued {
    pub code: PairingCode,
    pub expires_at_ms: i64,
}

/// What `GET /auth/config` names: the sign-in the server takes at its enrol
/// and attest routes (109.5). keeper compares it with its own account before
/// any token leaves the device (S-ID-2).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfig {
    pub issuer: String,
    pub client_id: String,
    /// The scopes a token must have been asked with to carry the server's
    /// audience and role.
    pub scopes: Vec<String>,
}

/// An enrolment's answer: a device token (shown once), what it opens, and
/// whether it adopted the device token sent with it (same client id).
#[derive(Clone, PartialEq, Eq)]
pub struct Enrolled {
    pub token: String,
    pub discovery: Discovery,
    pub adopted: bool,
}

impl std::fmt::Debug for Enrolled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Enrolled")
            .field("token", &"<redacted>")
            .field("discovery", &self.discovery)
            .field("adopted", &self.adopted)
            .finish()
    }
}

/// An attestation's answer, on the server's clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attested {
    pub attested_at_ms: i64,
    pub lapses_at_ms: i64,
}

/// How a device came to hold its credential, as a notice names it (S-ID-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnrolmentKind {
    /// A new device, by signing in.
    Enrolled,
    /// A paired device, signed in and kept under its client id.
    Adopted,
    /// A device that redeemed a pairing code.
    Paired,
}

/// One "new device" notice of the person's, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Enrolment {
    pub client_id: String,
    pub name: String,
    pub host: Option<String>,
    pub kind: EnrolmentKind,
    pub at_ms: i64,
    /// The notice is this device's own.
    #[serde(rename = "self")]
    pub this_device: bool,
}

#[derive(Deserialize)]
struct EnrolmentsBody {
    enrolments: Vec<Enrolment>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DatabaseRow {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveryBody {
    actor_id: String,
    client_id: String,
    databases: Vec<DatabaseRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RedeemBody {
    token: String,
    actor_id: String,
    client_id: String,
    databases: Vec<DatabaseRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnrolBody {
    token: String,
    actor_id: String,
    client_id: String,
    databases: Vec<DatabaseRow>,
    adopted: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssuedBody {
    code: String,
    expires_at_ms: i64,
}

/// A database name keeper puts in a path and a URL: lower-case letters,
/// digits, `-` and `_`, starting with a letter or digit, at most 63 long.
pub fn is_database_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && name.len() <= 63
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// An id keeper puts in a file name: what the server mints (`actor`,
/// `actor.hex`) and nothing that walks a path.
fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn discovery_of(
    actor_id: String,
    client_id: String,
    rows: Vec<DatabaseRow>,
) -> Result<Discovery, RouteError> {
    if !is_id(&actor_id) || !is_id(&client_id) {
        return Err(RouteError::Unexpected(
            "an id keeper cannot store".to_owned(),
        ));
    }
    let mut databases: Vec<String> = Vec::new();
    for row in rows {
        if !is_database_name(&row.name) {
            return Err(RouteError::Unexpected(format!(
                "a database named {:?}",
                row.name
            )));
        }
        if !databases.contains(&row.name) {
            databases.push(row.name);
        }
    }
    Ok(Discovery {
        actor_id,
        client_id,
        databases,
    })
}

/// A root route's answer, as a future the caller can drop to cancel it.
pub type Answer<'a, T> = Pin<Box<dyn Future<Output = Result<T, RouteError>> + Send + 'a>>;

/// The server's root routes. [`HttpRoutes`] is the one implementation keeper
/// runs; tests answer with their own.
pub trait Routes: Send + Sync {
    /// `GET /databases`: whom the token was issued to and what it opens.
    fn discover<'a>(&'a self, server: &'a ServerUrl, token: &'a str) -> Answer<'a, Discovery>;
    /// `POST /pair/redeem`: the code is the credential; `name` is how the
    /// person's other devices list this one; `host` binds its T4 identity.
    fn redeem<'a>(
        &'a self,
        server: &'a ServerUrl,
        code: &'a PairingCode,
        name: &'a str,
        host: &'a str,
    ) -> Answer<'a, Redeemed>;
    /// `POST /pair/start`: a code another device of this person can redeem.
    fn start<'a>(&'a self, server: &'a ServerUrl, token: &'a str) -> Answer<'a, Issued>;
    /// `POST /clients/{id}/revoke`: ends `client_id`'s access everywhere.
    fn revoke<'a>(
        &'a self,
        server: &'a ServerUrl,
        token: &'a str,
        client_id: &'a str,
    ) -> Answer<'a, ()>;
    /// `PUT /clients/self/push-target` (`DELETE` for `None`): where the
    /// server's push reaction posts for this device, on every database its
    /// token opens.
    fn set_push_target<'a>(
        &'a self,
        server: &'a ServerUrl,
        token: &'a str,
        target: Option<&'a PushTarget>,
    ) -> Answer<'a, ()>;
    /// `GET /<database>/push/item/{kind}/{id}`: the answer's bytes.
    fn push_item<'a>(
        &'a self,
        server: &'a ServerUrl,
        token: &'a str,
        pushed: &'a PushedItem,
    ) -> Answer<'a, Vec<u8>>;
    /// `GET /auth/config`, unauthenticated: the sign-in the server trusts.
    fn auth_config<'a>(&'a self, server: &'a ServerUrl) -> Answer<'a, AuthConfig>;
    /// `POST /devices/enroll`: the account's sign-in token is the bearer,
    /// once; `device_token` is a paired device's own, sent so the server
    /// adopts it (same client id, new token) rather than minting a device.
    fn enroll<'a>(
        &'a self,
        server: &'a ServerUrl,
        sign_in_token: &'a str,
        name: &'a str,
        host: &'a str,
        device_token: Option<&'a str>,
    ) -> Answer<'a, Enrolled>;
    /// `POST /clients/self/attest`: a fresh sign-in token keeps the device
    /// the sign-in enrolled from lapsing.
    fn attest<'a>(
        &'a self,
        server: &'a ServerUrl,
        sign_in_token: &'a str,
        device_token: &'a str,
    ) -> Answer<'a, Attested>;
    /// `GET /clients/self/enrolments`: the person's newest "new device"
    /// notices, by the device token.
    fn enrolments<'a>(
        &'a self,
        server: &'a ServerUrl,
        token: &'a str,
    ) -> Answer<'a, Vec<Enrolment>>;
}

/// The root routes over HTTP, with no redirect followed.
pub struct HttpRoutes {
    http: reqwest::Client,
}

impl HttpRoutes {
    pub fn new() -> HttpRoutes {
        HttpRoutes {
            http: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }
}

impl Default for HttpRoutes {
    fn default() -> Self {
        HttpRoutes::new()
    }
}

impl Routes for HttpRoutes {
    fn set_push_target<'a>(
        &'a self,
        server: &'a ServerUrl,
        token: &'a str,
        target: Option<&'a PushTarget>,
    ) -> Answer<'a, ()> {
        Box::pin(set_push_target(&self.http, server, token, target))
    }
    fn push_item<'a>(
        &'a self,
        server: &'a ServerUrl,
        token: &'a str,
        pushed: &'a PushedItem,
    ) -> Answer<'a, Vec<u8>> {
        Box::pin(push_item(&self.http, server, token, pushed))
    }
    fn discover<'a>(&'a self, server: &'a ServerUrl, token: &'a str) -> Answer<'a, Discovery> {
        Box::pin(discover(&self.http, server, token))
    }
    fn redeem<'a>(
        &'a self,
        server: &'a ServerUrl,
        code: &'a PairingCode,
        name: &'a str,
        host: &'a str,
    ) -> Answer<'a, Redeemed> {
        Box::pin(redeem(&self.http, server, code, name, host))
    }
    fn start<'a>(&'a self, server: &'a ServerUrl, token: &'a str) -> Answer<'a, Issued> {
        Box::pin(start(&self.http, server, token))
    }
    fn revoke<'a>(
        &'a self,
        server: &'a ServerUrl,
        token: &'a str,
        client_id: &'a str,
    ) -> Answer<'a, ()> {
        Box::pin(revoke(&self.http, server, token, client_id))
    }
    fn auth_config<'a>(&'a self, server: &'a ServerUrl) -> Answer<'a, AuthConfig> {
        Box::pin(auth_config(&self.http, server))
    }
    fn enroll<'a>(
        &'a self,
        server: &'a ServerUrl,
        sign_in_token: &'a str,
        name: &'a str,
        host: &'a str,
        device_token: Option<&'a str>,
    ) -> Answer<'a, Enrolled> {
        Box::pin(enroll(
            &self.http,
            server,
            sign_in_token,
            name,
            host,
            device_token,
        ))
    }
    fn attest<'a>(
        &'a self,
        server: &'a ServerUrl,
        sign_in_token: &'a str,
        device_token: &'a str,
    ) -> Answer<'a, Attested> {
        Box::pin(attest(&self.http, server, sign_in_token, device_token))
    }
    fn enrolments<'a>(
        &'a self,
        server: &'a ServerUrl,
        token: &'a str,
    ) -> Answer<'a, Vec<Enrolment>> {
        Box::pin(enrolments(&self.http, server, token))
    }
}

async fn send(request: reqwest::RequestBuilder) -> Result<reqwest::Response, RouteError> {
    let response = request
        .send()
        .await
        .map_err(|error| RouteError::Unreachable(error.without_url().to_string()))?;
    let status = response.status().as_u16();
    match status {
        200..=299 => Ok(response),
        401 => Err(RouteError::Unauthorized),
        403 => {
            let code = response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|body| body["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| "forbidden".to_owned());
            Err(RouteError::Forbidden(code))
        }
        other => Err(RouteError::Unexpected(format!("HTTP {other}"))),
    }
}

async fn body<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, RouteError> {
    let bytes = response
        .bytes()
        .await
        .map_err(|error| RouteError::Unreachable(error.without_url().to_string()))?;
    serde_json::from_slice(&bytes).map_err(|error| RouteError::Unexpected(error.to_string()))
}

/// `GET /databases`: whom the token was issued to and the databases it opens.
async fn discover(
    http: &reqwest::Client,
    server: &ServerUrl,
    token: &str,
) -> Result<Discovery, RouteError> {
    let response = send(http.get(server.route("/databases")).bearer_auth(token)).await?;
    let found: DiscoveryBody = body(response).await?;
    discovery_of(found.actor_id, found.client_id, found.databases)
}

/// `POST /pair/redeem`: the code is the credential; `name` is how the
/// person's other devices list this one; `host` is bound at enrollment.
async fn redeem(
    http: &reqwest::Client,
    server: &ServerUrl,
    code: &PairingCode,
    name: &str,
    host: &str,
) -> Result<Redeemed, RouteError> {
    let host = checked_host(host)?;
    let response = send(
        http.post(server.route("/pair/redeem"))
            .bearer_auth(code.as_str())
            .json(&serde_json::json!({ "name": name, "host": host })),
    )
    .await?;
    let found: RedeemBody = body(response).await?;
    if found.token.is_empty() {
        return Err(RouteError::Unexpected("an empty token".to_owned()));
    }
    Ok(Redeemed {
        token: found.token,
        discovery: discovery_of(found.actor_id, found.client_id, found.databases)?,
    })
}

/// This device's host as the server's naming rule takes it (`device-name.ts`,
/// for pairing and enrolment alike): [`crate::device_name::host_rule`], the
/// rule Settings › This device's name checks a name with too.
fn checked_host(host: &str) -> Result<&str, RouteError> {
    crate::device_name::host_rule(host)
        .map_err(|_| RouteError::Unexpected("this device has no valid host name".to_owned()))
}

/// A sign-in route's request: a success, or the server's refusal with its
/// code ([`RouteError::Refused`]) — an old server without the route answers
/// 404 too, and is told apart from nothing else.
async fn send_signin(request: reqwest::RequestBuilder) -> Result<reqwest::Response, RouteError> {
    let response = request
        .send()
        .await
        .map_err(|error| RouteError::Unreachable(error.without_url().to_string()))?;
    let status = response.status().as_u16();
    match status {
        200..=299 => Ok(response),
        400 | 401 | 403 | 404 | 409 | 503 => {
            let code = response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|body| body["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("http-{status}"));
            Err(RouteError::Refused { status, code })
        }
        other => Err(RouteError::Unexpected(format!("HTTP {other}"))),
    }
}

/// `GET /auth/config`: no credential goes with it.
async fn auth_config(http: &reqwest::Client, server: &ServerUrl) -> Result<AuthConfig, RouteError> {
    let response = send_signin(http.get(server.route("/auth/config"))).await?;
    body(response).await
}

/// `POST /devices/enroll` with the sign-in token as the bearer and the
/// device token, when there is one, in the body (never a URL, S-ID-2).
async fn enroll(
    http: &reqwest::Client,
    server: &ServerUrl,
    sign_in_token: &str,
    name: &str,
    host: &str,
    device_token: Option<&str>,
) -> Result<Enrolled, RouteError> {
    let host = checked_host(host)?;
    let mut request = serde_json::json!({ "name": name, "host": host });
    if let Some(device_token) = device_token {
        request["deviceToken"] = device_token.into();
    }
    let response = send_signin(
        http.post(server.route("/devices/enroll"))
            .bearer_auth(sign_in_token)
            .json(&request),
    )
    .await?;
    let found: EnrolBody = body(response).await?;
    if found.token.is_empty() {
        return Err(RouteError::Unexpected("an empty token".to_owned()));
    }
    Ok(Enrolled {
        token: found.token,
        discovery: discovery_of(found.actor_id, found.client_id, found.databases)?,
        adopted: found.adopted,
    })
}

/// `POST /clients/self/attest` with the sign-in token as the bearer.
async fn attest(
    http: &reqwest::Client,
    server: &ServerUrl,
    sign_in_token: &str,
    device_token: &str,
) -> Result<Attested, RouteError> {
    let response = send_signin(
        http.post(server.route("/clients/self/attest"))
            .bearer_auth(sign_in_token)
            .json(&serde_json::json!({ "deviceToken": device_token })),
    )
    .await?;
    body(response).await
}

/// `GET /clients/self/enrolments` with the device token.
async fn enrolments(
    http: &reqwest::Client,
    server: &ServerUrl,
    token: &str,
) -> Result<Vec<Enrolment>, RouteError> {
    let response = send_signin(
        http.get(server.route("/clients/self/enrolments"))
            .bearer_auth(token),
    )
    .await?;
    let found: EnrolmentsBody = body(response).await?;
    Ok(found.enrolments)
}

/// `POST /pair/start`: a code another device of this person can redeem.
async fn start(
    http: &reqwest::Client,
    server: &ServerUrl,
    token: &str,
) -> Result<Issued, RouteError> {
    let response = send(
        http.post(server.route("/pair/start"))
            .bearer_auth(token)
            .json(&serde_json::json!({})),
    )
    .await?;
    let found: IssuedBody = body(response).await?;
    let code = crate::hq::pairing::parse(&found.code)
        .map_err(|_| RouteError::Unexpected("a code keeper cannot read".to_owned()))?
        .code;
    Ok(Issued {
        code,
        expires_at_ms: found.expires_at_ms,
    })
}

/// `POST /clients/{id}/revoke`: ends `client_id`'s access on every database.
async fn revoke(
    http: &reqwest::Client,
    server: &ServerUrl,
    token: &str,
    client_id: &str,
) -> Result<(), RouteError> {
    if !is_id(client_id) {
        return Err(RouteError::Unexpected("not a device id".to_owned()));
    }
    send(
        http.post(server.route(&format!("/clients/{client_id}/revoke")))
            .bearer_auth(token),
    )
    .await?;
    Ok(())
}

/// `PUT /clients/self/push-target` with `target`, or `DELETE` it for none.
async fn set_push_target(
    http: &reqwest::Client,
    server: &ServerUrl,
    token: &str,
    target: Option<&PushTarget>,
) -> Result<(), RouteError> {
    let route = server.route("/clients/self/push-target");
    let request = match target {
        Some(target) => http.put(route).json(&target.body()),
        None => http.delete(route),
    };
    send(request.bearer_auth(token)).await?;
    Ok(())
}

/// `GET /<database>/push/item/{kind}/{id}`: the answer's bytes, which
/// [`crate::hq::push::item_row`] reads.
async fn push_item(
    http: &reqwest::Client,
    server: &ServerUrl,
    token: &str,
    pushed: &PushedItem,
) -> Result<Vec<u8>, RouteError> {
    let url = pushed
        .url(server)
        .ok_or_else(|| RouteError::Unexpected("an item keeper cannot address".to_owned()))?;
    let response = send(http.get(url).bearer_auth(token)).await?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| RouteError::Unreachable(error.without_url().to_string()))?;
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_invalid_host_is_refused_before_redeeming_a_code() {
        let server = ServerUrl::parse("http://127.0.0.1:1")
            .expect("url")
            .expect("server");
        let code = crate::hq::pairing::parse("0123456789ABCDEFGHJKMNPQ")
            .expect("code")
            .code;
        for host in [
            String::new(),
            "  ".to_owned(),
            "x".repeat(201),
            "😀".repeat(101),
        ] {
            assert!(
                matches!(
                    redeem(&reqwest::Client::new(), &server, &code, "keeper", &host).await,
                    Err(RouteError::Unexpected(_))
                ),
                "invalid host must be refused locally"
            );
        }
    }

    #[test]
    fn a_database_name_is_safe_in_a_path_and_a_url() {
        for ok in ["alpha", "beta", "a", "db-2", "x_y"] {
            assert!(is_database_name(ok), "{ok}");
        }
        for bad in ["", "..", "a/b", "A", "-a", "a b", "a?b", &"a".repeat(64)] {
            assert!(!is_database_name(bad), "{bad}");
        }
    }

    #[test]
    fn discovery_refuses_a_name_or_an_id_it_cannot_store() {
        let row = |name: &str| DatabaseRow {
            name: name.to_owned(),
        };
        let found = discovery_of(
            "ana".to_owned(),
            "ana.0a1b2c3d".to_owned(),
            vec![row("alpha"), row("beta"), row("alpha")],
        )
        .expect("reads");
        assert_eq!(found.databases, ["alpha", "beta"]);
        assert!(discovery_of("t".to_owned(), "t.1".to_owned(), vec![row("../x")]).is_err());
        assert!(discovery_of("../t".to_owned(), "t.1".to_owned(), vec![]).is_err());
        assert!(discovery_of("t".to_owned(), ".t".to_owned(), vec![]).is_err());
    }

    #[test]
    fn a_redeemed_token_never_prints() {
        let redeemed = Redeemed {
            token: "0123456789abcdef".to_owned(),
            discovery: Discovery {
                actor_id: "t".to_owned(),
                client_id: "t.1".to_owned(),
                databases: vec![],
            },
        };
        assert!(!format!("{redeemed:?}").contains("01234567"));
    }
}
