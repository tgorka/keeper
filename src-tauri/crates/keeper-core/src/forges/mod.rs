//! Browse your repositories and add them as drives (Epic 86).
//!
//! Where repositories come from ([`source`]), how keeper gets a token for
//! each ([`tokens`], [`broker`], [`device_flow`]), what a listing says
//! ([`listing`], [`github`], [`forgejo`]) and how each repository relates to
//! the drives already here ([`mark`]). Not under `org_account`: GitHub works
//! without an account.
//!
//! Tokens live in the keychain (`forge/<id>/<client id>/session`) or in this process's
//! memory, never on disk, over IPC or in a log line. Every token goes only
//! to its own forge's origin or to the descriptor's broker, and only over
//! `https` (loopback for tests).

pub mod broker;
pub mod device_flow;
pub mod forgejo;
pub mod github;
pub mod listing;
pub mod mark;
pub mod source;
pub mod tokens;
pub mod vm;

#[cfg(test)]
pub(crate) mod testing;

pub use source::{
    credential_for, find, forge_credential_id, remote_on_source, sources, ForgeKind, ForgeSource,
    TokenVia, ACCOUNT_FORGE_ID, BUILTIN_GITHUB_CLIENT_ID, GITHUB_ID,
};
pub use tokens::{token_origins, ForgeError};

/// Forget everything this process remembers on behalf of whoever is signed
/// in or connected — every listing and the broker's grants and tokens — so
/// nothing of one identity is served to the next. Sign-in, sign-out, Forget
/// account and a new descriptor call it.
pub fn forget_identity() {
    listing::forget_all();
    broker::forget();
}

use std::time::Duration;

/// Every forge and broker request gets this long.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// `https`, or plain `http` to a loopback host (a test server): the only
/// URLs a token is sent to.
pub(crate) fn token_safe(url: &url::Url) -> bool {
    match url.scheme() {
        "https" => url.host().is_some(),
        "http" => match url.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
            None => false,
        },
        _ => false,
    }
}

/// The host of `url`, or the text itself when it has none.
pub(crate) fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| url.to_owned())
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Send a request that carries a secret (a token, a device code): refused
/// before it leaves when its address may not receive one, and refused after
/// when the response came from anywhere else (a redirect the client
/// followed). A network failure is `Unreachable`, naming the host.
pub(crate) async fn send(
    http: &reqwest::Client,
    request: reqwest::RequestBuilder,
) -> Result<reqwest::Response, ForgeError> {
    let request = request
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| ForgeError::Internal(format!("could not build a request: {e}")))?;
    let host = request.url().host_str().unwrap_or_default().to_owned();
    if !token_safe(request.url()) {
        return Err(ForgeError::Refused(format!(
            "keeper sends a token to {host} only over https."
        )));
    }
    let origin = request.url().origin();
    let response = http
        .execute(request)
        .await
        .map_err(|_| ForgeError::Unreachable(format!("Can't reach {host}.")))?;
    if response.url().origin() != origin || response.status().is_redirection() {
        return Err(ForgeError::Refused(format!(
            "{host} sent keeper somewhere else; keeper does not follow it with a token."
        )));
    }
    Ok(response)
}

/// Read a response body, at most 8 MiB: a page of repositories is ~300 KiB.
pub(crate) async fn body(response: reqwest::Response) -> Result<Vec<u8>, ForgeError> {
    const MAX_BODY: usize = 8 * 1024 * 1024;
    let host = response.url().host_str().unwrap_or_default().to_owned();
    let mut response = response;
    let mut out = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ForgeError::Unreachable(format!("Can't reach {host}.")))?
    {
        if out.len() + chunk.len() > MAX_BODY {
            return Err(ForgeError::Refused(format!(
                "{host} sent more than keeper reads in one answer."
            )));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}
