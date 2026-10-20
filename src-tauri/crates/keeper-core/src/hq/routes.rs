//! The server's root routes, outside every database's mount: discovery
//! (`GET /databases`), pairing (`POST /pair/start`, `POST /pair/redeem`) and
//! revocation (`POST /clients/{id}/revoke`). Each request carries the device's
//! token as a bearer credential and nothing else of it; no response body is
//! ever logged, because a redeem's body is the token.

use serde::Deserialize;
use std::time::Duration;

use crate::hq::pairing::PairingCode;
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

/// The client every root request goes through.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
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
pub async fn discover(
    http: &reqwest::Client,
    server: &ServerUrl,
    token: &str,
) -> Result<Discovery, RouteError> {
    let response = send(http.get(server.route("/databases")).bearer_auth(token)).await?;
    let found: DiscoveryBody = body(response).await?;
    discovery_of(found.actor_id, found.client_id, found.databases)
}

/// `POST /pair/redeem`: the code is the credential; `name` is how the
/// person's other devices list this one.
pub async fn redeem(
    http: &reqwest::Client,
    server: &ServerUrl,
    code: &PairingCode,
    name: &str,
) -> Result<Redeemed, RouteError> {
    let response = send(
        http.post(server.route("/pair/redeem"))
            .bearer_auth(code.as_str())
            .json(&serde_json::json!({ "name": name })),
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

/// `POST /pair/start`: a code another device of this person can redeem.
pub async fn start(
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
pub async fn revoke(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_database_name_is_safe_in_a_path_and_a_url() {
        for ok in ["tgdrive", "neuradrive", "a", "db-2", "x_y"] {
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
            "tgorka".to_owned(),
            "tgorka.0a1b2c3d".to_owned(),
            vec![row("tgdrive"), row("neuradrive"), row("tgdrive")],
        )
        .expect("reads");
        assert_eq!(found.databases, ["tgdrive", "neuradrive"]);
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
