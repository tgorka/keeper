//! OpenAI-shaped embeddings through the configured provider (AD-264).
//! The caller supplies the shared `http::client`; no new host or model download.

use serde::{Deserialize, Serialize};

use super::http;
use super::quirks::{quirks, Support};
use super::Endpoint;

pub const EMBEDDINGS_PATH: &str = "/v1/embeddings";
pub const EMBED_BATCH_MAX: usize = 32;
pub const EMBEDDINGS_UNSUPPORTED: &str = "The provider did not answer /v1/embeddings for this model; search is words only until a model that embeds is chosen.";

pub struct EmbedRequest<'a> {
    pub model: &'a str,
    pub inputs: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("The embeddings provider refused the token (HTTP {status}).")]
    Unauthorized { status: u16 },
    #[error("{EMBEDDINGS_UNSUPPORTED} (HTTP {status})")]
    Unsupported { status: u16 },
    #[error("The embeddings provider is temporarily unavailable (HTTP {status}); retry later.")]
    Transient { status: u16 },
    #[error("Malformed embeddings response: {0}")]
    Malformed(String),
    #[error("Embeddings transport failed: {0}")]
    Transport(String),
}

impl EmbedError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Transient { .. } | Self::Transport(_))
    }
}

fn family(model: &str) -> &str {
    let name = model.rsplit_once('/').map_or(model, |(_, name)| name);
    name.split_once(':').map_or(name, |(name, _)| name)
}

/// Research §6.3: E5 and nomic require distinct retrieval-task prefixes.
pub fn query_prefix(model: &str) -> &'static str {
    let name = family(model).to_ascii_lowercase();
    if name.starts_with("e5-") || name.starts_with("multilingual-e5-") {
        "query: "
    } else if name.starts_with("nomic-") {
        "search_query: "
    } else {
        ""
    }
}

pub fn passage_prefix(model: &str) -> &'static str {
    match query_prefix(model) {
        "query: " => "passage: ",
        "search_query: " => "search_document: ",
        _ => "",
    }
}

#[derive(Serialize)]
struct RequestBody<'a> {
    model: &'a str,
    input: &'a [String],
}

fn request_body<'a>(req: &'a EmbedRequest<'_>) -> Result<RequestBody<'a>, EmbedError> {
    if req.inputs.is_empty() || req.inputs.len() > EMBED_BATCH_MAX {
        return Err(EmbedError::Malformed(format!(
            "a batch must contain 1 to {EMBED_BATCH_MAX} inputs"
        )));
    }
    Ok(RequestBody {
        model: req.model,
        input: &req.inputs,
    })
}

fn require_support(support: Support) -> Result<(), EmbedError> {
    if support == Support::No {
        return Err(EmbedError::Unsupported { status: 501 });
    }
    Ok(())
}

fn require_success(status: u16) -> Result<(), EmbedError> {
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err(EmbedError::Unauthorized { status }),
        404 | 405 | 501 => Err(EmbedError::Unsupported { status }),
        408 | 429 | 500..=599 => Err(EmbedError::Transient { status }),
        _ => Err(EmbedError::Malformed(format!(
            "unexpected HTTP status {status}"
        ))),
    }
}

#[derive(Deserialize)]
struct ResponseBody {
    data: Vec<ResponseRow>,
}

#[derive(Deserialize)]
struct ResponseRow {
    index: usize,
    embedding: Vec<f32>,
}

fn parse_response(bytes: &[u8], expected: usize) -> Result<Vec<Vec<f32>>, EmbedError> {
    // Do not include serde's error text: a malformed value may contain note text.
    let body: ResponseBody = serde_json::from_slice(bytes)
        .map_err(|_| EmbedError::Malformed("expected indexed data[].embedding vectors".into()))?;
    if body.data.len() != expected || expected == 0 {
        return Err(EmbedError::Malformed(
            "vector count differs from input count".into(),
        ));
    }
    let mut vectors: Vec<Option<Vec<f32>>> = (0..expected).map(|_| None).collect();
    let mut dimensions = None;
    for row in body.data {
        if row.index >= expected || vectors[row.index].is_some() {
            return Err(EmbedError::Malformed(
                "duplicate or out-of-range index".into(),
            ));
        }
        let dim = row.embedding.len();
        if dim == 0
            || dimensions.is_some_and(|expected| expected != dim)
            || row.embedding.iter().any(|value| !value.is_finite())
        {
            return Err(EmbedError::Malformed(
                "empty, nonfinite or inconsistent vector dimensions".into(),
            ));
        }
        dimensions = Some(dim);
        vectors[row.index] = Some(row.embedding);
    }
    vectors
        .into_iter()
        .map(|vector| vector.ok_or_else(|| EmbedError::Malformed("missing vector index".into())))
        .collect()
}

/// POST one bounded batch; the caller chooses query/passage prefixes and stores
/// normalized vectors. A refusal never masquerades as an empty successful list.
pub async fn embed(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    req: EmbedRequest<'_>,
) -> Result<Vec<Vec<f32>>, EmbedError> {
    require_support(quirks(endpoint.kind).embeddings)?;
    let body = request_body(&req)?;
    let request = http::authorize(
        client.post(endpoint.url(EMBEDDINGS_PATH)),
        endpoint.token.as_deref(),
    )
    .map_err(|error| EmbedError::Transport(error.to_string()))?
    .json(&body);
    let response = request
        .send()
        .await
        .map_err(|error| EmbedError::Transport(error.without_url().to_string()))?;
    require_success(response.status().as_u16())?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| EmbedError::Transport(error.without_url().to_string()))?;
    parse_response(&bytes, req.inputs.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_uses_string_array_and_enforces_batch_cap() {
        let req = EmbedRequest {
            model: "bge-m3",
            inputs: vec!["first".into(), "second".into()],
        };
        assert_eq!(
            serde_json::to_value(request_body(&req).expect("body")).expect("json"),
            json!({"model":"bge-m3", "input":["first", "second"]})
        );
        for count in [0, 33] {
            let req = EmbedRequest {
                model: "m",
                inputs: vec!["x".into(); count],
            };
            assert!(request_body(&req).is_err());
        }
        let req = EmbedRequest {
            model: "m",
            inputs: vec!["x".into(); 32],
        };
        assert_eq!(request_body(&req).expect("cap").input.len(), 32);
    }

    #[test]
    fn response_is_ordered_by_index_not_arrival() {
        let bytes =
            br#"{"data":[{"index":1,"embedding":[0.0,1.0]},{"index":0,"embedding":[1.0,0.0]}]}"#;
        assert_eq!(
            parse_response(bytes, 2).expect("vectors"),
            vec![vec![1.0, 0.0], vec![0.0, 1.0]]
        );
    }

    #[test]
    fn malformed_vectors_are_refused_not_partially_returned() {
        for body in [
            json!({}),
            json!({"data": []}),
            json!({"data": [{"index":0,"embedding":[]}, {"index":1,"embedding":[]}]}),
            json!({"data": [{"index":0,"embedding":[1.0]}, {"index":1,"embedding":[1.0,2.0]}]}),
            json!({"data": [{"index":0,"embedding":[1.0]}, {"index":0,"embedding":[2.0]}]}),
            json!({"data": [{"index":0,"embedding":[1.0]}, {"index":2,"embedding":[2.0]}]}),
        ] {
            assert!(matches!(
                parse_response(&serde_json::to_vec(&body).expect("json"), 2),
                Err(EmbedError::Malformed(_))
            ));
        }
        assert!(parse_response(b"not json", 1).is_err());
        assert!(parse_response(br#"{"data":[{"index":0,"embedding":[1e100]}]}"#, 1).is_err());
    }

    #[test]
    fn provider_errors_distinguish_retry_from_configuration() {
        for status in [401, 403] {
            let error = require_success(status).expect_err("refused token");
            assert!(matches!(error, EmbedError::Unauthorized { .. }));
            assert!(!error.is_retryable());
        }
        for status in [404, 405, 501] {
            let error = require_success(status).expect_err("unsupported");
            assert!(matches!(error, EmbedError::Unsupported { .. }));
            assert!(!error.is_retryable());
        }
        for status in [408, 429, 500, 502, 503] {
            let error = require_success(status).expect_err("temporary");
            assert!(matches!(error, EmbedError::Transient { .. }));
            assert!(error.is_retryable());
        }
        assert!(EmbedError::Transport("offline".into()).is_retryable());
        assert!(!require_success(301)
            .expect_err("unexpected redirect")
            .is_retryable());
    }

    #[test]
    fn prefixes_follow_model_family_not_the_provider() {
        for model in [
            "intfloat/multilingual-e5-small",
            "e5-base:latest",
            "Intfloat/E5-base",
        ] {
            assert_eq!(query_prefix(model), "query: ");
            assert_eq!(passage_prefix(model), "passage: ");
        }
        assert_eq!(
            query_prefix("nomic-ai/Nomic-Embed-Text-v1.5"),
            "search_query: "
        );
        assert_eq!(
            passage_prefix("nomic-embed-text:latest"),
            "search_document: "
        );
        for model in ["bge-m3", "embeddinggemma", "some5-model"] {
            assert_eq!(query_prefix(model), "");
            assert_eq!(passage_prefix(model), "");
        }
    }
}
