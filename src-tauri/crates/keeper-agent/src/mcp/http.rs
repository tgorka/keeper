//! The streamable HTTP client an MCP connection rides: the workspace's
//! `reqwest`, with every body a server sends read only up to
//! [`super::MESSAGE_MAX`] — a JSON answer, an error body, each event of an
//! event stream — so a server cannot make keeper hold more than that of
//! one message (R225). A non-2xx body is kept as the server's own words
//! ([`HttpFault::Answered`]), so a call's error reaches the model as
//! outside content, never as keeper's (R225).

use std::collections::HashMap;

use std::sync::Arc;

use futures_util::stream::{BoxStream, StreamExt};
use reqwest::header::{HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use rmcp::model::{
    ClientJsonRpcMessage, ClientRequest, ErrorData, JsonRpcMessage, ServerJsonRpcMessage,
};
use rmcp::transport::streamable_http_client::{
    SseError, StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
};
use sse_stream::{Sse, SseStream};

const SESSION_ID: &str = "Mcp-Session-Id";
const LAST_EVENT_ID: &str = "Last-Event-Id";
const EVENT_STREAM: &str = "text/event-stream";
const JSON: &str = "application/json";

/// What went wrong on the HTTP side of a connection.
#[derive(Debug)]
pub(super) enum HttpFault {
    /// The request did not complete: keeper's own diagnostic.
    Reqwest(reqwest::Error),
    /// The server answered with a non-2xx status and this body, its own
    /// words, cut at the bound.
    Answered { status: u16, body: String },
    /// A body or an event went past the bound; keeper read no further.
    TooLarge,
    /// A call keeper did not send: its [`super::Admission`] no longer
    /// held as its request was to be dispatched. Keeper's sentence.
    Unsent(String),
}

impl std::fmt::Display for HttpFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpFault::Reqwest(error) => write!(f, "{error}"),
            HttpFault::Answered { status, .. } => write!(f, "HTTP {status}"),
            HttpFault::TooLarge => f.write_str(&super::too_large()),
            HttpFault::Unsent(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for HttpFault {}

type Fault = StreamableHttpError<HttpFault>;

/// `reqwest` with the bound, counting each refusal in `refused`.
#[derive(Clone)]
pub(super) struct BoundedHttp {
    pub client: reqwest::Client,
    pub refused: Arc<super::Refusals>,
}

impl BoundedHttp {
    fn too_large(&self) -> Fault {
        self.refused.refuse();
        StreamableHttpError::Client(HttpFault::TooLarge)
    }

    /// The whole body, refused past the bound.
    async fn body(&self, mut response: reqwest::Response) -> Result<Vec<u8>, Fault> {
        if response
            .content_length()
            .is_some_and(|length| length > super::MESSAGE_MAX as u64)
        {
            return Err(self.too_large());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(reqwest_fault)? {
            if body.len() + chunk.len() > super::MESSAGE_MAX {
                return Err(self.too_large());
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    /// An event stream whose every event is refused past the bound.
    fn events(&self, response: reqwest::Response) -> BoxStream<'static, Result<Sse, SseError>> {
        let refused = Arc::clone(&self.refused);
        let chunks = futures_util::stream::unfold(
            (response, EventBound::default(), false),
            move |(mut response, mut bound, done)| {
                let refused = Arc::clone(&refused);
                async move {
                    if done {
                        return None;
                    }
                    match response.chunk().await {
                        Ok(Some(chunk)) if bound.admits(&chunk) => {
                            Some((Ok(chunk), (response, bound, false)))
                        }
                        Ok(Some(_)) => {
                            refused.refuse();
                            Some((Err(HttpFault::TooLarge), (response, bound, true)))
                        }
                        Ok(None) => None,
                        Err(error) => {
                            Some((Err(HttpFault::Reqwest(error)), (response, bound, true)))
                        }
                    }
                }
            },
        );
        SseStream::from_bytes_stream(chunks).boxed()
    }

    fn request(
        &self,
        builder: reqwest::RequestBuilder,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> reqwest::RequestBuilder {
        let mut builder = builder.header(ACCEPT, format!("{EVENT_STREAM}, {JSON}"));
        if let Some(token) = auth_header {
            builder = builder.bearer_auth(token);
        }
        for (name, value) in custom_headers {
            builder = builder.header(name, value);
        }
        if let Some(session) = session_id {
            builder = builder.header(SESSION_ID, session.as_ref());
        }
        builder
    }
}

fn reqwest_fault(error: reqwest::Error) -> Fault {
    StreamableHttpError::Client(HttpFault::Reqwest(error))
}

fn content_type(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(CONTENT_TYPE)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
}

/// How much of the event being read has arrived: an event ends at a blank
/// line (`\n\n`, `\r\r` or `\r\n\r\n`).
#[derive(Default)]
struct EventBound {
    event: usize,
    line_empty: bool,
    after_cr: bool,
}

impl EventBound {
    fn admits(&mut self, chunk: &[u8]) -> bool {
        for &byte in chunk {
            match byte {
                b'\n' if self.after_cr => {}
                b'\r' | b'\n' => {
                    if self.line_empty {
                        self.event = 0;
                    }
                    self.line_empty = true;
                }
                _ => {
                    self.line_empty = false;
                    self.event += 1;
                }
            }
            self.after_cr = byte == b'\r';
            if self.event > super::MESSAGE_MAX {
                return false;
            }
        }
        true
    }
}

impl StreamableHttpClient for BoundedHttp {
    type Error = HttpFault;

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, Fault> {
        let attached = session_id.is_some();
        // Built whole first: a call's final check is followed by nothing
        // but handing it to its send.
        let request = self
            .request(
                self.client.post(uri.as_ref()),
                session_id,
                auth_header,
                custom_headers,
            )
            .json(&message)
            .build()
            .map_err(reqwest_fault)?;
        let call = match &message {
            ClientJsonRpcMessage::Request(rpc) => match &rpc.request {
                ClientRequest::CallToolRequest(call) => {
                    Some(call.extensions.get::<super::Admission>())
                }
                _ => None,
            },
            _ => None,
        };
        let response = match call {
            Some(admission) => {
                #[cfg(test)]
                if let Some(boundary) = admission.and_then(|a| a.boundary.as_ref()) {
                    boundary.reach().await;
                }
                // The request handed to a send of its own under the hold of
                // the server's state that checks it, and awaited outside
                // it: what lands after the check lands after the call left
                // (R238, R267).
                let sending =
                    super::admitted(admission, || Ok(tokio::spawn(self.client.execute(request))))
                        .map_err(|why| StreamableHttpError::Client(HttpFault::Unsent(why)))?;
                sending.await.map_err(StreamableHttpError::TokioJoinError)?
            }
            None => self.client.execute(request).await,
        }
        .map_err(reqwest_fault)?;
        let status = response.status();
        if matches!(
            status,
            reqwest::StatusCode::ACCEPTED | reqwest::StatusCode::NO_CONTENT
        ) {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        // A 404 on a session the server forgot is its answer like any other
        // status: its words outside content, never a cue to start a session
        // again and resend the call in it.
        let kind = content_type(&response);
        let session = response
            .headers()
            .get(SESSION_ID)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let answers_request = matches!(message, ClientJsonRpcMessage::Request(_));
        if status.is_success() && response.content_length() == Some(0) && !answers_request {
            return Ok(StreamableHttpPostResponse::Accepted);
        }
        if !status.is_success() {
            let body = self.body(response).await?;
            let body = String::from_utf8_lossy(&body).into_owned();
            if let Some(legacy) = legacy_discover(&message, attached, status, &body) {
                return Ok(legacy);
            }
            if kind.as_deref().is_some_and(|kind| kind.starts_with(JSON)) {
                if let Ok(error @ JsonRpcMessage::Error(_)) =
                    serde_json::from_str::<ServerJsonRpcMessage>(&body)
                {
                    return Ok(StreamableHttpPostResponse::Json(error, session));
                }
            }
            return Err(StreamableHttpError::Client(HttpFault::Answered {
                status: status.as_u16(),
                body,
            }));
        }
        match kind.as_deref() {
            Some(kind) if kind.starts_with(EVENT_STREAM) => Ok(StreamableHttpPostResponse::Sse(
                self.events(response),
                session,
            )),
            Some(kind) if kind.starts_with(JSON) => {
                let body = self.body(response).await?;
                match serde_json::from_slice::<ServerJsonRpcMessage>(&body) {
                    Ok(parsed) => Ok(StreamableHttpPostResponse::Json(parsed, session)),
                    Err(_) if !answers_request => Ok(StreamableHttpPostResponse::Accepted),
                    Err(error) => Err(StreamableHttpError::Deserialize(error)),
                }
            }
            _ => Err(StreamableHttpError::UnexpectedContentType(kind)),
        }
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), Fault> {
        let response = self
            .request(
                self.client.delete(uri.as_ref()),
                Some(session_id),
                auth_header,
                custom_headers,
            )
            .send()
            .await
            .map_err(reqwest_fault)?;
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            return Ok(());
        }
        response.error_for_status().map_err(reqwest_fault)?;
        Ok(())
    }

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, Fault> {
        let mut builder = self.request(
            self.client.get(uri.as_ref()),
            session_id,
            auth_header,
            custom_headers,
        );
        if let Some(last) = last_event_id {
            builder = builder.header(LAST_EVENT_ID, last);
        }
        let response = builder.send().await.map_err(reqwest_fault)?;
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            return Err(StreamableHttpError::ServerDoesNotSupportSse);
        }
        let response = response.error_for_status().map_err(reqwest_fault)?;
        match content_type(&response) {
            Some(kind) if kind.starts_with(EVENT_STREAM) || kind.starts_with(JSON) => {
                Ok(self.events(response))
            }
            kind => Err(StreamableHttpError::UnexpectedContentType(kind)),
        }
    }
}

/// A legacy server's HTTP 4xx to `server/discover`, as the JSON-RPC error
/// rmcp's handshake reads as "legacy, initialize instead" (rmcp's own
/// adapter does the same).
fn legacy_discover(
    message: &ClientJsonRpcMessage,
    attached: bool,
    status: reqwest::StatusCode,
    body: &str,
) -> Option<StreamableHttpPostResponse> {
    if attached
        || !status.is_client_error()
        || matches!(
            status,
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        )
    {
        return None;
    }
    let ClientJsonRpcMessage::Request(request) = message else {
        return None;
    };
    if !matches!(request.request, ClientRequest::DiscoverRequest(_)) {
        return None;
    }
    let error = match serde_json::from_str::<ServerJsonRpcMessage>(body) {
        Ok(ServerJsonRpcMessage::Error(error)) => error.error,
        _ => {
            ErrorData::invalid_request(format!("server/discover rejected with HTTP {status}"), None)
        }
    };
    Some(StreamableHttpPostResponse::Json(
        ServerJsonRpcMessage::error(error, Some(request.id.clone())),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R225: an event is bounded whatever its line endings, and a
    /// stream of events each under the bound runs on.
    #[test]
    fn each_event_is_bounded() {
        let max = crate::mcp::MESSAGE_MAX;
        let mut bound = EventBound::default();
        let line = format!("data: {}\r\n\r\n", "x".repeat(max - 10));
        for _ in 0..4 {
            assert!(bound.admits(line.as_bytes()), "events under the bound");
        }
        let mut lines = EventBound::default();
        let half = format!("data: {}\r\n", "x".repeat(max / 2));
        assert!(lines.admits(half.as_bytes()));
        assert!(
            !lines.admits(half.as_bytes()),
            "two lines of one event are one event"
        );
        let mut endless = EventBound::default();
        assert!(!endless.admits(&vec![b'x'; max + 1]));
    }
}
