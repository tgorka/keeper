//! The mautrix **bridgev2 provisioning** transport (Story 6.3, FR-26, AD-16).
//!
//! Drives the HTTP+JSON provisioning login state machine, authenticated with the
//! account's own Matrix access token as `Authorization: Bearer` (the bridgev2
//! `AllowMatrixAuth` mode — never a shared admin secret). The base URL is resolved
//! by a **data-driven, ordered probe**: [`resolve_candidates`] expands the
//! `provisioning.json` templates for the account's server, and [`Provisioning::connect`]
//! probes each `…/v3/login/flows` in order — the first that authenticates wins.
//!
//! Endpoints (relative to the resolved base, which ends `/_matrix/provision`):
//! - `GET  …/v3/login/flows`
//! - `POST …/v3/login/start/{flowID}`
//! - `POST …/v3/login/step/{loginID}/{stepID}/{stepType}`
//! - `POST …/v3/login/cancel/{loginID}`
//!
//! `display_and_wait` is a server long-poll, so the step request uses a generous
//! timeout. The token never leaves this module — only rendered
//! [`crate::vm::BridgeLoginVm`] state reaches the frontend.

use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

use serde::Deserialize;

use crate::bridges::data;
use crate::bridges::transport::{BridgeTransport, LoginFlow, LoginStepResponse};
use crate::error::BridgeError;

/// The long-poll timeout for a `display_and_wait` step (and every step call, for
/// simplicity). Generous: the server holds the request open until the peer acts or
/// the QR rotates.
const STEP_TIMEOUT: Duration = Duration::from_secs(120);
/// The shorter timeout for the base-URL probe and the flows/start/cancel calls.
const SHORT_TIMEOUT: Duration = Duration::from_secs(30);

/// Expand the ordered `provisioning.json` candidate templates for `server` into
/// concrete base URLs by substituting the `{server}` placeholder. Pure and
/// unit-tested; the impure probe ([`Provisioning::connect`]) tries each in order.
///
/// Trailing slashes are trimmed so the caller can always join `…/v3/login/…`
/// segments without a double slash.
pub fn resolve_candidates(server: &str, doc: &data::ProvisioningDoc) -> Vec<String> {
    doc.candidates
        .iter()
        .map(|template| {
            template
                .replace("{server}", server)
                .trim_end_matches('/')
                .to_owned()
        })
        .collect()
}

/// Classify the "no candidate connected" outcome of the base-URL probe (pure,
/// unit-tested): if any resolved candidate answered with a genuine server error
/// (`saw_transport_error`), that is a real transport failure surfaced as
/// [`BridgeError::Provisioning`] (`Err`); otherwise every candidate was simply
/// absent / unauthenticated, so there is no provisioning API here — `Ok(())` (the
/// caller maps this to `Ok(None)` and falls back to the Bridge Bot driver). The
/// `network` name is woven into the honest error copy.
fn classify_no_connection(saw_transport_error: bool, network: &str) -> Result<(), BridgeError> {
    if saw_transport_error {
        Err(BridgeError::Provisioning(format!(
            "Couldn't reach a provisioning API for {network}."
        )))
    } else {
        Ok(())
    }
}

/// The bridgev2 `GET /v3/login/flows` response shape.
#[derive(Debug, Deserialize)]
struct FlowsResponse {
    #[serde(default)]
    flows: Vec<LoginFlow>,
}

/// A live provisioning transport bound to one resolved base URL + bearer token.
#[derive(Clone)]
pub struct Provisioning {
    http: reqwest::Client,
    /// The resolved provisioning base URL (ends `…/_matrix/provision`, no trailing
    /// slash).
    base_url: String,
    /// The account's Matrix access token — sent only as a Bearer header, never
    /// logged, never returned.
    token: String,
    /// The flows discovered during the base-URL probe (so `login_flows` need not
    /// re-fetch).
    flows: Vec<LoginFlow>,
}

impl Provisioning {
    /// Probe the ordered candidate base URLs for `server` and connect to the first
    /// whose `…/v3/login/flows` authenticates with `token`.
    ///
    /// Three outcomes (Story 6.4): `Ok(Some(_))` = a candidate authenticated and
    /// this transport is bound to it; `Ok(None)` = no candidate resolved a
    /// provisioning base URL at all (the bridge exposes no provisioning API keeper
    /// can reach), which the caller treats as the signal to fall back to the Bridge
    /// Bot driver; `Err(_)` = a genuine transport error on a resolved candidate (a
    /// reachable endpoint that failed for a real reason), which the caller surfaces
    /// rather than silently falling back. The `network` name is woven into any
    /// message so the copy is honest per-network.
    pub async fn connect(
        server: &str,
        token: &str,
        network: &str,
    ) -> Result<Option<Self>, BridgeError> {
        let doc = data::provisioning()?;
        let candidates = resolve_candidates(server, doc);
        let http = reqwest::Client::builder()
            .timeout(SHORT_TIMEOUT)
            .build()
            .map_err(|e| BridgeError::Provisioning(format!("could not build HTTP client: {e}")))?;

        // Track the strongest signal seen so probe outcomes classify honestly: a
        // reachable-but-failing candidate (e.g. a 5xx from the provisioning app) is a
        // real error, whereas every candidate being merely absent/unauthenticated is
        // "no provisioning API" → bot fallback (`Ok(None)`).
        let mut saw_transport_error = false;

        for base in &candidates {
            let url = format!("{base}/v3/login/flows");
            match http.get(&url).bearer_auth(token).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let flows = match resp.json::<FlowsResponse>().await {
                        Ok(body) => body.flows,
                        Err(e) => {
                            tracing::debug!(base = %base, error = %e, "provisioning flows body unparseable; trying next candidate");
                            continue;
                        }
                    };
                    tracing::info!(base = %base, network = %network, "provisioning base URL resolved");
                    return Ok(Some(Self {
                        http,
                        base_url: base.clone(),
                        token: token.to_owned(),
                        flows,
                    }));
                }
                Ok(resp) => {
                    // A resolved endpoint that answered with a server error (5xx) is a
                    // genuine transport failure, not an "endpoint absent" — distinguish
                    // it so it never masquerades as bot-fallback. An auth/4xx answer is
                    // the ordinary "not a provisioning API here" case that degrades.
                    if resp.status().is_server_error() {
                        saw_transport_error = true;
                    }
                    tracing::debug!(base = %base, status = %resp.status(), "provisioning candidate did not authenticate; trying next");
                }
                Err(e) => {
                    // A per-candidate transport error (connect/DNS/TLS/timeout) is
                    // expected when a candidate host simply doesn't exist under this
                    // deployment; it degrades to the next candidate and, if nothing
                    // resolves, to the bot fallback (`Ok(None)`).
                    tracing::debug!(base = %base, error = %e, "provisioning candidate unreachable; trying next");
                }
            }
        }

        match classify_no_connection(saw_transport_error, network) {
            Ok(()) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// POST a login-step body and parse the next [`LoginStepResponse`]. A non-2xx
    /// surfaces the response body verbatim as [`BridgeError::Provisioning`].
    async fn post_step(
        &self,
        url: &str,
        body: &BTreeMap<String, String>,
        timeout: Duration,
    ) -> Result<LoginStepResponse, BridgeError> {
        let resp = self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .timeout(timeout)
            .json(body)
            .send()
            .await
            .map_err(|e| BridgeError::Provisioning(format!("provisioning request failed: {e}")))?;
        parse_step_response(resp).await
    }
}

/// Parse a provisioning response into a [`LoginStepResponse`], surfacing a non-2xx
/// body verbatim (the bridge's own error message) as [`BridgeError::Provisioning`].
///
/// Split out from the request so the verbatim-error contract stays testable.
async fn parse_step_response(resp: reqwest::Response) -> Result<LoginStepResponse, BridgeError> {
    let status = resp.status();
    if !status.is_success() {
        // Surface the bridge's own error text verbatim (bridgev2 returns a JSON
        // `{"error": "..."}` body; fall back to the raw text if it isn't JSON).
        let text = resp.text().await.unwrap_or_default();
        let message = extract_error_message(&text)
            .unwrap_or_else(|| format!("provisioning step failed ({status})"));
        return Err(BridgeError::Provisioning(message));
    }
    resp.json::<LoginStepResponse>()
        .await
        .map_err(|e| BridgeError::Provisioning(format!("provisioning response unparseable: {e}")))
}

/// The maximum length (chars) of a surfaced bridge error message. A bridge could
/// return an arbitrarily large body; capping it keeps an unbounded message from
/// reaching the VM/DOM verbatim.
const MAX_ERROR_MESSAGE_CHARS: usize = 2000;

/// Truncate a surfaced error message to [`MAX_ERROR_MESSAGE_CHARS`] on a char
/// boundary (`chars().take(..)` never splits a codepoint).
fn cap_message(msg: &str) -> String {
    msg.chars()
        .take(MAX_ERROR_MESSAGE_CHARS)
        .collect::<String>()
}

/// Pull the bridge's own error message out of a non-2xx body: prefer a JSON
/// `{"error": "..."}` field, else the trimmed raw text if non-empty. The returned
/// message is capped to [`MAX_ERROR_MESSAGE_CHARS`] so an oversized body never
/// reaches the VM/DOM verbatim. Pure and unit-tested so the verbatim-error contract
/// can't silently regress.
pub fn extract_error_message(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct ErrorBody {
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        message: Option<String>,
    }
    if let Ok(parsed) = serde_json::from_str::<ErrorBody>(body) {
        if let Some(msg) = parsed.error.or(parsed.message) {
            if !msg.trim().is_empty() {
                return Some(cap_message(&msg));
            }
        }
    }
    let trimmed = body.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(cap_message(trimmed))
    }
}

impl BridgeTransport for Provisioning {
    fn login_flows(&self) -> impl Future<Output = Result<Vec<LoginFlow>, BridgeError>> + Send {
        // The flows were fetched during the base-URL probe; hand them back without
        // a second round-trip.
        let flows = self.flows.clone();
        async move { Ok(flows) }
    }

    fn login_start(
        &self,
        flow_id: &str,
    ) -> impl Future<Output = Result<LoginStepResponse, BridgeError>> + Send {
        let url = format!("{}/v3/login/start/{flow_id}", self.base_url);
        let empty = BTreeMap::new();
        async move { self.post_step(&url, &empty, SHORT_TIMEOUT).await }
    }

    fn login_step(
        &self,
        login_id: &str,
        step_id: &str,
        step_type: &str,
        body: &BTreeMap<String, String>,
    ) -> impl Future<Output = Result<LoginStepResponse, BridgeError>> + Send {
        let url = format!(
            "{}/v3/login/step/{login_id}/{step_id}/{step_type}",
            self.base_url
        );
        let body = body.clone();
        async move { self.post_step(&url, &body, STEP_TIMEOUT).await }
    }

    fn login_cancel(&self, login_id: &str) -> impl Future<Output = ()> + Send {
        let url = format!("{}/v3/login/cancel/{login_id}", self.base_url);
        async move {
            // Best-effort: log and swallow — cancel must never surface an error.
            match self
                .http
                .post(&url)
                .bearer_auth(&self.token)
                .timeout(SHORT_TIMEOUT)
                .send()
                .await
            {
                Ok(resp) => {
                    tracing::debug!(status = %resp.status(), "provisioning login cancelled");
                }
                Err(e) => {
                    tracing::debug!(error = %e, "provisioning cancel failed (best-effort)");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(candidates: &[&str]) -> data::ProvisioningDoc {
        data::ProvisioningDoc {
            version: 1,
            candidates: candidates.iter().map(|c| (*c).to_owned()).collect(),
        }
    }

    #[test]
    fn resolve_candidates_substitutes_server_and_trims_trailing_slash() {
        let d = doc(&[
            "https://{server}/_matrix/provision",
            "https://matrix.{server}/_matrix/provision/",
        ]);
        let resolved = resolve_candidates("example.org", &d);
        assert_eq!(
            resolved,
            vec![
                "https://example.org/_matrix/provision".to_owned(),
                "https://matrix.example.org/_matrix/provision".to_owned(),
            ]
        );
    }

    #[test]
    fn resolve_candidates_preserves_order() {
        let d = doc(&["a-{server}", "b-{server}", "c-{server}"]);
        let resolved = resolve_candidates("h", &d);
        assert_eq!(resolved, vec!["a-h", "b-h", "c-h"]);
    }

    #[test]
    fn resolve_candidates_uses_the_embedded_data_file() {
        // The real embedded data file must resolve to at least the primary
        // `https://{server}/_matrix/provision` candidate for a given server.
        let d = data::provisioning().expect("provisioning parses");
        let resolved = resolve_candidates("keeper.test", d);
        assert!(
            resolved
                .iter()
                .any(|c| c == "https://keeper.test/_matrix/provision"),
            "resolved candidates were: {resolved:?}"
        );
    }

    #[test]
    fn no_connection_without_transport_error_degrades_to_bot_fallback() {
        // Every candidate merely absent / unauthenticated → no provisioning API here,
        // so the caller should fall back to the bot driver (`Ok(())` → `Ok(None)`).
        assert!(classify_no_connection(false, "whatsapp").is_ok());
    }

    #[test]
    fn no_connection_with_transport_error_is_a_real_failure() {
        // A resolved candidate answered with a server error — a genuine transport
        // failure that must surface, never a silent bot fallback.
        match classify_no_connection(true, "whatsapp") {
            Err(BridgeError::Provisioning(msg)) => {
                assert!(msg.contains("whatsapp"), "message names the network: {msg}");
            }
            other => panic!("expected a Provisioning error, got: {other:?}"),
        }
    }

    #[test]
    fn extract_error_message_prefers_json_error_field() {
        let body = r#"{"error": "M_FORBIDDEN: already logged in"}"#;
        assert_eq!(
            extract_error_message(body).as_deref(),
            Some("M_FORBIDDEN: already logged in")
        );
    }

    #[test]
    fn extract_error_message_falls_back_to_message_then_raw_text() {
        let body = r#"{"message": "bridge is restarting"}"#;
        assert_eq!(
            extract_error_message(body).as_deref(),
            Some("bridge is restarting")
        );

        let raw = "plain text failure";
        assert_eq!(
            extract_error_message(raw).as_deref(),
            Some("plain text failure")
        );
    }

    #[test]
    fn extract_error_message_is_none_for_empty_body() {
        assert_eq!(extract_error_message(""), None);
        assert_eq!(extract_error_message("   "), None);
    }

    #[test]
    fn extract_error_message_caps_oversized_body() {
        // A raw-text body far larger than the cap must be truncated to ≤ the cap.
        let huge_raw = "x".repeat(MAX_ERROR_MESSAGE_CHARS + 5000);
        let capped = extract_error_message(&huge_raw).expect("non-empty body");
        assert_eq!(capped.chars().count(), MAX_ERROR_MESSAGE_CHARS);

        // The JSON `error` branch must be capped too.
        let huge_json = format!(
            r#"{{"error": "{}"}}"#,
            "y".repeat(MAX_ERROR_MESSAGE_CHARS + 5000)
        );
        let capped_json = extract_error_message(&huge_json).expect("non-empty json error");
        assert_eq!(capped_json.chars().count(), MAX_ERROR_MESSAGE_CHARS);
    }
}
