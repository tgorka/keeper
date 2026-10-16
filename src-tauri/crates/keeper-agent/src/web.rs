//! `web_search` on a host (story 101.4, R419): one engine the host names —
//! agentd's `[web_search] url`, the Mac's from Settings › Agents — asked
//! over SearXNG's JSON API. Every decision — the URL, the query, the bounds,
//! what the model reads and how it is labelled — is
//! `keeper_core::agents::web_search`'s; this module makes the request, within
//! [`TIMEOUT_SECS`] and [`ANSWER_BYTES`], following no redirect.

use std::time::Duration;

use keeper_core::agents::web_search::{
    self as core_web, Query, WebResult, ANSWER_BYTES, TIMEOUT_SECS,
};
use keeper_core::bots::url::BaseUrl;

/// A host's web search engine and the client that reaches it.
#[derive(Debug)]
pub struct WebSearch {
    engine: BaseUrl,
    client: reqwest::Client,
}

impl WebSearch {
    /// The engine at `engine`, its client bounded in time and following no
    /// redirect: an answer from anywhere but the engine is not read.
    pub fn new(engine: BaseUrl) -> Result<WebSearch, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| {
                format!("keeper could not make a client for the web search engine: {error}")
            })?;
        Ok(WebSearch { engine, client })
    }

    /// The engine this host names.
    pub fn engine(&self) -> &BaseUrl {
        &self.engine
    }

    /// The engine's results for `query`, or the sentence the model reads
    /// — logged at WARN with the engine, never the query (R381).
    pub async fn search(&self, query: &Query) -> Result<Vec<WebResult>, String> {
        let searched = self.ask(query).await;
        if let Err(sentence) = &searched {
            tracing::warn!(engine = %core_web::engine(&self.engine), %sentence, "agents: a web search failed");
        }
        searched
    }

    async fn ask(&self, query: &Query) -> Result<Vec<WebResult>, String> {
        let unreachable =
            |error: reqwest::Error| core_web::unreachable(&self.engine, error.is_timeout());
        let mut response = self
            .client
            .get(core_web::request_url(&self.engine, query))
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(unreachable)?;
        if let Some(refusal) = core_web::status_refusal(&self.engine, response.status().as_u16()) {
            return Err(refusal);
        }
        if response
            .content_length()
            .is_some_and(|length| length > ANSWER_BYTES as u64)
        {
            return Err(core_web::too_long(&self.engine));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(unreachable)? {
            if body.len() + chunk.len() > ANSWER_BYTES {
                return Err(core_web::too_long(&self.engine));
            }
            body.extend_from_slice(&chunk);
        }
        core_web::parse_answer(&self.engine, &body)
    }
}

/// The engine `url` names, or why none is used — logged at WARN, so a host
/// whose operator named one that does not work says so (R381).
pub fn engine_from(url: Option<&BaseUrl>) -> Option<std::sync::Arc<WebSearch>> {
    let url = url?;
    match WebSearch::new(url.clone()) {
        Ok(engine) => Some(std::sync::Arc::new(engine)),
        Err(sentence) => {
            tracing::warn!(%sentence, "agents: this host offers no web search");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// A one-request HTTP server answering `status` with `body`, its
    /// length declared; the request line it read comes back on the handle.
    async fn serve(status: &str, body: Vec<u8>) -> (String, tokio::task::JoinHandle<String>) {
        serve_with(status, "", body, true).await
    }

    /// [`serve`] with `headers` (each ending `\r\n`), and the body's length
    /// declared only when `declared` — else it runs to the connection's end.
    async fn serve_with(
        status: &str,
        headers: &str,
        body: Vec<u8>,
        declared: bool,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let at = format!("http://{}", listener.local_addr().expect("addr"));
        let (status, headers) = (status.to_owned(), headers.to_owned());
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut read = vec![0u8; 8192];
            let n = socket.read(&mut read).await.expect("read");
            let request = String::from_utf8_lossy(&read[..n]).to_string();
            let length = if declared {
                format!("content-length: {}\r\n", body.len())
            } else {
                String::new()
            };
            let head = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\n{length}{headers}connection: close\r\n\r\n"
            );
            // A client that stops reading at its bound closes the socket.
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(&body).await;
            request.lines().next().unwrap_or_default().to_owned()
        });
        (at, handle)
    }

    fn query(text: &str) -> Query {
        Query::parse(&json!({ "query": text })).expect("a query")
    }

    /// 101.4 #1, #4: the engine is asked `/search?q=…&format=json` and its
    /// SearXNG answer comes back as results; a 403 (JSON not enabled), a
    /// redirect and an answer past the bound are sentences, never results.
    #[tokio::test]
    async fn an_engine_is_asked_once_and_its_answer_bounded() {
        let body = serde_json::to_vec(&json!({
            "query": "keeper matrix",
            "results": [{"url": "https://example.org/a", "title": "A", "content": "about a"}]
        }))
        .expect("json");
        let (at, request) = serve("200 OK", body).await;
        let engine = WebSearch::new(core_web::parse_url(&format!("{at}/searx")).expect("url"))
            .expect("client");
        let results = engine
            .search(&query("keeper matrix"))
            .await
            .expect("results");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "A");
        assert_eq!(
            request.await.expect("served"),
            "GET /searx/search?q=keeper+matrix&format=json HTTP/1.1"
        );

        let (at, _) = serve("403 Forbidden", b"Forbidden".to_vec()).await;
        let engine = WebSearch::new(core_web::parse_url(&at).expect("url")).expect("client");
        let refused = engine.search(&query("x")).await.expect_err("403");
        assert!(refused.contains("settings.yml must list json"), "{refused}");

        // A redirect is the engine's answer, never followed: the place it
        // names — which would answer with results — is never asked.
        let elsewhere = serde_json::to_vec(&json!({
            "results": [{"url": "https://example.org/b", "title": "B"}]
        }))
        .expect("json");
        let (there, asked_there) = serve("200 OK", elsewhere).await;
        let (at, _) = serve_with(
            "302 Found",
            &format!("location: {there}/search?q=x&format=json\r\n"),
            Vec::new(),
            true,
        )
        .await;
        let engine = WebSearch::new(core_web::parse_url(&at).expect("url")).expect("client");
        let refused = engine.search(&query("x")).await.expect_err("a redirect");
        assert!(refused.contains("answered HTTP 302"), "{refused}");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!asked_there.is_finished(), "the redirect was followed");
        asked_there.abort();

        // Past the bound, declared or not: a SearXNG answer padded past
        // 2 MiB is refused, its length announced up front or only found
        // as it streams in.
        let padded = |declared| {
            let mut padded = br#"{"results": [], "pad": ""#.to_vec();
            padded.resize(padded.len() + ANSWER_BYTES, b'x');
            padded.extend(br#""}"#);
            serve_with("200 OK", "", padded, declared)
        };
        for declared in [true, false] {
            let (at, _) = padded(declared).await;
            let engine = WebSearch::new(core_web::parse_url(&at).expect("url")).expect("client");
            let refused = engine.search(&query("x")).await.expect_err("too long");
            assert!(refused.contains("more than 2 MiB"), "{declared}: {refused}");
        }
    }

    /// 101.4 #4, live: a real SearXNG (`KEEPER_LIVE_SEARXNG_URL`, JSON on —
    /// run on delectra against a `searxng/searxng` container) answers a
    /// query with results keeper reads: at most ten, each an http(s) URL
    /// with a title, rendered under the not-instructions sentence.
    #[tokio::test]
    #[ignore = "needs a live SearXNG at KEEPER_LIVE_SEARXNG_URL"]
    async fn a_live_searxng_answers_with_results() {
        let url = std::env::var("KEEPER_LIVE_SEARXNG_URL").expect("KEEPER_LIVE_SEARXNG_URL");
        let engine = WebSearch::new(core_web::parse_url(&url).expect("url")).expect("client");
        let asked = query("matrix protocol");
        let results = engine.search(&asked).await.expect("results");
        assert!(!results.is_empty(), "the engine found nothing");
        assert!(results.len() <= core_web::MAX_RESULTS);
        for result in &results {
            assert!(result.url.starts_with("http"), "{result:?}");
        }
        let text = core_web::render(&asked, &results);
        assert!(text.starts_with(core_web::RESULTS_ARE_DATA));
        println!("{text}");
    }

    /// Nobody listening is a sentence naming the engine, not a hang.
    #[tokio::test]
    async fn an_engine_nobody_answers_for_is_a_sentence() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let at = format!("http://{}", listener.local_addr().expect("addr"));
        drop(listener);
        let engine = WebSearch::new(core_web::parse_url(&at).expect("url")).expect("client");
        let refused = engine.search(&query("x")).await.expect_err("nobody");
        assert!(refused.contains("could not be reached"), "{refused}");
    }
}
