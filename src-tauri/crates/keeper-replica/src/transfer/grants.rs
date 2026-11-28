//! HTTP client for file-transfer grants and store discovery (story 115.3).

use std::fmt;
use std::time::Duration;

use keeper_ports::transfer::{
    Completion, DownloadReference, HeldPart, PartUrl, SignedUrl, StoreOrigin, TransferGrants,
    UploadAsk, UploadGrant,
};
use keeper_ports::{ReplicaError, CREDENTIAL_REFUSED};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const TRANSPORT_FAILED: &str = "transport.failed";
const UNEXPECTED_STATUS: &str = "files.unexpected_status";

/// HTTP implementation of [`TransferGrants`] against a database mount.
pub struct HttpGrants {
    base: String,
    token: String,
    agent: ureq::Agent,
}

impl HttpGrants {
    /// Build a grants client for `base_url` (database mount, with or without
    /// trailing slash) using bearer `token` and per-request `timeout`.
    pub fn new(base_url: &str, token: &str, timeout: Duration) -> Result<HttpGrants, ReplicaError> {
        let base = normalize_http_base(base_url, "grant base url")?;
        Ok(HttpGrants {
            base,
            token: token.to_owned(),
            agent: build_agent(timeout),
        })
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }

    fn post_json<T: Serialize>(&self, path: &str, body: &T) -> Result<Value, ReplicaError> {
        let bytes = serde_json::to_vec(body)
            .map_err(|err| ReplicaError::Client(format!("encode grant request: {err}")))?;
        let url = format!("{}{path}", self.base);
        let response = self
            .agent
            .post(&url)
            .header("authorization", &self.auth_header())
            .header("content-type", "application/json")
            .send(bytes.as_slice());
        decode_json_response(response)
    }

    fn get_json(&self, path: &str) -> Result<Value, ReplicaError> {
        let url = format!("{}{path}", self.base);
        let response = self
            .agent
            .get(&url)
            .header("authorization", &self.auth_header())
            .call();
        decode_json_response(response)
    }
}

impl fmt::Debug for HttpGrants {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpGrants")
            .field("base", &self.base)
            .field("token", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl TransferGrants for HttpGrants {
    fn grant_upload(&self, ask: &UploadAsk<'_>) -> Result<UploadGrant, ReplicaError> {
        let value = self.post_json("/files/upload-grant", ask)?;
        parse_upload_grant(value)
    }

    fn part_urls(&self, plan_id: &str, numbers: &[u32]) -> Result<Vec<PartUrl>, ReplicaError> {
        validate_plan_id(plan_id)?;
        let body = PartUrlsRequest { numbers };
        let value = self.post_json(&format!("/files/plans/{plan_id}/part-urls"), &body)?;
        parse_part_urls(value)
    }

    fn held_parts(&self, plan_id: &str) -> Result<Vec<HeldPart>, ReplicaError> {
        validate_plan_id(plan_id)?;
        let value = self.get_json(&format!("/files/plans/{plan_id}/parts"))?;
        parse_held_parts(value)
    }

    fn complete(&self, plan_id: &str) -> Result<Completion, ReplicaError> {
        validate_plan_id(plan_id)?;
        let value = self.post_json(&format!("/files/plans/{plan_id}/complete"), &EmptyBody {})?;
        parse_completion(value)
    }

    fn grant_download(
        &self,
        content_id: &str,
        reference: Option<DownloadReference<'_>>,
    ) -> Result<SignedUrl, ReplicaError> {
        let value = match reference {
            Some(reference) => self.post_json(
                "/files/download-grant",
                &FileDownloadRequest {
                    content_id,
                    reference,
                },
            )?,
            None => self.post_json(
                "/files/download-grant",
                &DownloadGrantRequest { content_id },
            )?,
        };
        parse_signed_url(value)
    }
}

/// Download-only [`TransferGrants`] against the generic Syncular blob route
/// (`GET <base>/blobs/<blobId>`).
pub struct BlobRouteGrants {
    base: String,
    token: String,
    agent: ureq::Agent,
}

impl BlobRouteGrants {
    /// Build a blob-route grants client for `base_url` (database mount, with or without
    /// trailing slash) using bearer `token` and per-request `timeout`.
    pub fn new(
        base_url: &str,
        token: &str,
        timeout: Duration,
    ) -> Result<BlobRouteGrants, ReplicaError> {
        let base = normalize_http_base(base_url, "grant base url")?;
        Ok(BlobRouteGrants {
            base,
            token: token.to_owned(),
            agent: build_agent(timeout),
        })
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }
}

impl fmt::Debug for BlobRouteGrants {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlobRouteGrants")
            .field("base", &self.base)
            .field("token", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl TransferGrants for BlobRouteGrants {
    fn grant_upload(&self, _ask: &UploadAsk<'_>) -> Result<UploadGrant, ReplicaError> {
        Err(ReplicaError::Client(
            "model blobs are only downloaded".to_owned(),
        ))
    }

    fn part_urls(&self, _plan_id: &str, _numbers: &[u32]) -> Result<Vec<PartUrl>, ReplicaError> {
        Err(ReplicaError::Client(
            "model blobs are only downloaded".to_owned(),
        ))
    }

    fn held_parts(&self, _plan_id: &str) -> Result<Vec<HeldPart>, ReplicaError> {
        Err(ReplicaError::Client(
            "model blobs are only downloaded".to_owned(),
        ))
    }

    fn complete(&self, _plan_id: &str) -> Result<Completion, ReplicaError> {
        Err(ReplicaError::Client(
            "model blobs are only downloaded".to_owned(),
        ))
    }

    fn grant_download(
        &self,
        content_id: &str,
        reference: Option<DownloadReference<'_>>,
    ) -> Result<SignedUrl, ReplicaError> {
        if reference.is_some() {
            return Err(ReplicaError::Client(
                "file references require a files grant".to_owned(),
            ));
        }
        validate_blob_id(content_id)?;
        let url = format!("{}/blobs/{content_id}", self.base);
        let response = self
            .agent
            .get(&url)
            .header("authorization", &self.auth_header())
            .call();
        let value = decode_json_response(response)?;
        parse_blob_signed_url(value)
    }
}

/// Ask `GET <server_url>/databases` for the object store origin announced for
/// `database`. Returns `Ok(None)` when the database is absent or its `store`
/// field is absent or unparsable.
pub fn announced_store(
    server_url: &str,
    database: &str,
    token: &str,
    timeout: Duration,
) -> Result<Option<StoreOrigin>, ReplicaError> {
    let base = normalize_http_base(server_url, "server url")?;
    let agent = build_agent(timeout);
    let url = format!("{base}/databases");
    let auth = format!("Bearer {token}");
    let response = agent.get(&url).header("authorization", &auth).call();
    let value = decode_json_response(response)?;
    parse_announced_store(value, database)
}

fn build_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(timeout))
        .timeout_recv_response(Some(timeout))
        .timeout_per_read(Some(timeout))
        .timeout_per_write(Some(timeout))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .new_agent()
}

fn normalize_http_base(raw: &str, label: &str) -> Result<String, ReplicaError> {
    let trimmed = raw.trim().trim_end_matches('/');
    let parsed = url::Url::parse(trimmed)
        .map_err(|err| ReplicaError::Client(format!("invalid {label}: {err}")))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none_or(str::is_empty) {
        return Err(ReplicaError::Client(format!(
            "invalid {label}: expected http or https URL with host"
        )));
    }
    Ok(trimmed.to_owned())
}

fn validate_plan_id(plan_id: &str) -> Result<(), ReplicaError> {
    if plan_id.is_empty()
        || plan_id.len() > 128
        || plan_id == "."
        || plan_id == ".."
        || !plan_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(ReplicaError::Client("invalid upload plan id".to_owned()));
    }
    Ok(())
}

fn validate_blob_id(content_id: &str) -> Result<(), ReplicaError> {
    let Some(hex) = content_id.strip_prefix("sha256:") else {
        return Err(ReplicaError::Client("invalid blob id".to_owned()));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        return Err(ReplicaError::Client("invalid blob id".to_owned()));
    }
    Ok(())
}

fn decode_json_response(
    result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<Value, ReplicaError> {
    let response = result.map_err(|err| ReplicaError::Unheard {
        code: TRANSPORT_FAILED.to_owned(),
        message: format!("grant request failed: {err}"),
        retryable: true,
    })?;
    let status = response.status().as_u16();
    let body_result = response.into_body().read_to_string();

    if status == 401 || status == 403 {
        let message = body_result
            .ok()
            .and_then(|text| parse_error_body(&text))
            .and_then(|err| err.message)
            .filter(|msg| !msg.is_empty())
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(ReplicaError::Remote {
            code: CREDENTIAL_REFUSED.to_owned(),
            message,
            retryable: false,
        });
    }

    if !(200..300).contains(&status) {
        let parsed = body_result.ok().and_then(|text| parse_error_body(&text));
        let code = parsed
            .as_ref()
            .and_then(|err| err.code.as_deref())
            .filter(|code| !code.is_empty())
            .unwrap_or(UNEXPECTED_STATUS)
            .to_owned();
        let message = parsed
            .and_then(|err| err.message)
            .filter(|msg| !msg.is_empty())
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(ReplicaError::Remote {
            code,
            message,
            retryable: status >= 500,
        });
    }

    let body = body_result
        .map_err(|err| ReplicaError::Client(format!("unreadable grant response body: {err}")))?;
    serde_json::from_str(&body)
        .map_err(|err| ReplicaError::Client(format!("invalid grant response JSON: {err}")))
}

#[derive(Deserialize)]
struct ErrorBody {
    code: Option<String>,
    message: Option<String>,
}

fn parse_error_body(text: &str) -> Option<ErrorBody> {
    serde_json::from_str(text).ok()
}

#[derive(Serialize)]
struct PartUrlsRequest<'a> {
    numbers: &'a [u32],
}

#[derive(Serialize)]
struct EmptyBody {}

#[derive(Serialize)]
struct FileDownloadRequest<'a> {
    content_id: &'a str,
    #[serde(flatten)]
    reference: DownloadReference<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadGrantRequest<'a> {
    content_id: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignedUrlWire {
    url: String,
    expires_at_ms: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BlobSignedUrlWire {
    url: String,
    #[serde(alias = "expiresAtMs")]
    url_expires_at_ms: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartUrlWire {
    number: u32,
    url: String,
    expires_at_ms: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartsGrantWire {
    plan_id: String,
    parts: Vec<PartUrlWire>,
}

#[derive(Deserialize)]
struct PartUrlsResponse {
    parts: Vec<PartUrlWire>,
}

#[derive(Deserialize)]
struct HeldPartWire {
    number: u32,
    size: u64,
}

#[derive(Deserialize)]
struct HeldPartsResponse {
    parts: Vec<HeldPartWire>,
}

#[derive(Deserialize)]
struct CompletionResponse {
    status: String,
}

fn parse_upload_grant(value: Value) -> Result<UploadGrant, ReplicaError> {
    if value.get("present").and_then(Value::as_bool) == Some(true) {
        return Ok(UploadGrant::Present);
    }
    if value.get("url").is_some() && value.get("parts").is_none() {
        let plan_id = value
            .get("planId")
            .map(|id| {
                let id = id
                    .as_str()
                    .ok_or_else(|| ReplicaError::Client("invalid upload plan id".to_owned()))?;
                validate_plan_id(id)?;
                Ok::<_, ReplicaError>(id.to_owned())
            })
            .transpose()?;
        return Ok(UploadGrant::Single {
            signed: parse_signed_url(value)?,
            plan_id,
        });
    }
    if value.get("planId").is_some() {
        let wire: PartsGrantWire = serde_json::from_value(value)
            .map_err(|err| ReplicaError::Client(format!("invalid parts upload grant: {err}")))?;
        validate_plan_id(&wire.plan_id)?;
        let urls = convert_part_urls(wire.parts)?;
        return Ok(UploadGrant::Parts {
            plan_id: wire.plan_id,
            urls,
        });
    }
    Err(ReplicaError::Client(
        "unrecognized upload grant response".to_owned(),
    ))
}

fn parse_signed_url(value: Value) -> Result<SignedUrl, ReplicaError> {
    let wire: SignedUrlWire = serde_json::from_value(value)
        .map_err(|err| ReplicaError::Client(format!("invalid signed url response: {err}")))?;
    if wire.url.is_empty() {
        return Err(ReplicaError::Client("signed url is empty".to_owned()));
    }
    Ok(SignedUrl {
        url: wire.url,
        expires_at_ms: wire.expires_at_ms,
    })
}

fn parse_blob_signed_url(value: Value) -> Result<SignedUrl, ReplicaError> {
    let wire: BlobSignedUrlWire = serde_json::from_value(value)
        .map_err(|err| ReplicaError::Client(format!("invalid signed url response: {err}")))?;
    if wire.url.is_empty() {
        return Err(ReplicaError::Client("signed url is empty".to_owned()));
    }
    Ok(SignedUrl {
        url: wire.url,
        expires_at_ms: wire.url_expires_at_ms,
    })
}

fn parse_part_urls(value: Value) -> Result<Vec<PartUrl>, ReplicaError> {
    let wire: PartUrlsResponse = serde_json::from_value(value)
        .map_err(|err| ReplicaError::Client(format!("invalid part urls response: {err}")))?;
    convert_part_urls(wire.parts)
}

fn convert_part_urls(parts: Vec<PartUrlWire>) -> Result<Vec<PartUrl>, ReplicaError> {
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        if part.url.is_empty() {
            return Err(ReplicaError::Client("part signed url is empty".to_owned()));
        }
        out.push(PartUrl {
            number: part.number,
            url: SignedUrl {
                url: part.url,
                expires_at_ms: part.expires_at_ms,
            },
        });
    }
    Ok(out)
}

fn parse_held_parts(value: Value) -> Result<Vec<HeldPart>, ReplicaError> {
    let wire: HeldPartsResponse = serde_json::from_value(value)
        .map_err(|err| ReplicaError::Client(format!("invalid held parts response: {err}")))?;
    Ok(wire
        .parts
        .into_iter()
        .map(|part| HeldPart {
            number: part.number,
            size: part.size,
        })
        .collect())
}

fn parse_completion(value: Value) -> Result<Completion, ReplicaError> {
    let wire: CompletionResponse = serde_json::from_value(value)
        .map_err(|err| ReplicaError::Client(format!("invalid completion response: {err}")))?;
    match wire.status.as_str() {
        "placed" => Ok(Completion::Placed),
        "verifying" => Ok(Completion::Verifying),
        other => Err(ReplicaError::Client(format!(
            "unknown completion status: {other}"
        ))),
    }
}

fn parse_announced_store(
    value: Value,
    database: &str,
) -> Result<Option<StoreOrigin>, ReplicaError> {
    let databases = value
        .get("databases")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ReplicaError::Client("databases response missing databases array".to_owned())
        })?;
    let store = databases
        .iter()
        .find(|entry| entry.get("name").and_then(Value::as_str) == Some(database))
        .and_then(|entry| entry.get("store"))
        .and_then(Value::as_str)
        .and_then(StoreOrigin::parse);
    Ok(store)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parse_upload_grant_present() {
        let grant =
            parse_upload_grant(json!({ "present": true })).expect("present grant should parse");
        assert_eq!(grant, UploadGrant::Present);
    }

    #[test]
    fn parse_upload_grant_single() {
        let grant = parse_upload_grant(json!({
            "url": "https://store.example/files/db/sha256/abc?sig=1",
            "expiresAtMs": 1_700_000_000_000_i64
        }))
        .expect("single grant should parse");
        assert_eq!(
            grant,
            UploadGrant::Single {
                plan_id: None,
                signed: SignedUrl {
                    url: "https://store.example/files/db/sha256/abc?sig=1".to_owned(),
                    expires_at_ms: 1_700_000_000_000,
                }
            }
        );
    }

    #[test]
    fn parse_upload_grant_parts() {
        let grant = parse_upload_grant(json!({
            "planId": "plan_01.a-Z",
            "parts": [
                {
                    "number": 1,
                    "url": "https://store.example/p1?sig=a",
                    "expiresAtMs": 100
                },
                {
                    "number": 2,
                    "url": "https://store.example/p2?sig=b",
                    "expiresAtMs": 200
                }
            ]
        }))
        .expect("parts grant should parse");
        assert_eq!(
            grant,
            UploadGrant::Parts {
                plan_id: "plan_01.a-Z".to_owned(),
                urls: vec![
                    PartUrl {
                        number: 1,
                        url: SignedUrl {
                            url: "https://store.example/p1?sig=a".to_owned(),
                            expires_at_ms: 100,
                        },
                    },
                    PartUrl {
                        number: 2,
                        url: SignedUrl {
                            url: "https://store.example/p2?sig=b".to_owned(),
                            expires_at_ms: 200,
                        },
                    },
                ],
            }
        );
    }

    #[test]
    fn parse_upload_grant_rejects_unknown_shape() {
        let err = parse_upload_grant(json!({ "present": false }))
            .expect_err("unknown grant shape should fail");
        assert!(matches!(err, ReplicaError::Client(_)));
    }

    #[test]
    fn plan_id_validation() {
        assert!(validate_plan_id("plan_123-abc.XYZ").is_ok());
        assert!(validate_plan_id(&"a".repeat(128)).is_ok());

        for bad in [
            "",
            ".",
            "..",
            "../escape",
            "a/b",
            "a?b=1",
            "a#b",
            "has space",
            &"a".repeat(129),
        ] {
            let err = validate_plan_id(bad).expect_err("invalid planId should fail");
            assert!(matches!(err, ReplicaError::Client(_)), "for {bad:?}");
        }
    }

    #[test]
    fn debug_redacts_token() {
        let grants = HttpGrants::new(
            "http://127.0.0.1:8080/main/",
            "super-secret-token",
            Duration::from_secs(5),
        )
        .expect("valid base url");
        let rendered = format!("{grants:?}");
        assert!(!rendered.contains("super-secret-token"));
        assert!(rendered.contains("REDACTED"));
        assert!(rendered.contains("http://127.0.0.1:8080/main"));
    }

    #[test]
    fn parse_announced_store_cases() {
        let body = json!({
            "databases": [
                { "name": "main", "store": "https://store.example:8443/ignored?sig=1" },
                { "name": "nostore" },
                { "name": "bad", "store": "https://user:pass@store.example" }
            ]
        });
        assert_eq!(
            parse_announced_store(body.clone(), "main")
                .expect("main should parse")
                .map(|o| o.as_str().to_owned()),
            Some("https://store.example:8443".to_owned())
        );
        assert_eq!(
            parse_announced_store(body.clone(), "nostore").expect("nostore"),
            None
        );
        assert_eq!(
            parse_announced_store(body.clone(), "bad").expect("bad"),
            None
        );
        assert_eq!(
            parse_announced_store(body, "missing").expect("missing"),
            None
        );
    }
}
