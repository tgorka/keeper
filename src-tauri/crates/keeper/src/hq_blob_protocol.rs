//! The `keeper-hq-blob://` custom URI-scheme protocol handler (story 107.1,
//! FR-858; AD-429, AD-433): the only way an agents-session attachment's bytes
//! reach the webview. A chip's URL (`keeper_core::hq::attachments::blob_url`)
//! names a database and a blob; the handler serves it from that database's
//! replica cache, downloading it on the first open, and answers as the
//! `keeper-media://` handler does — 200, 206 for a `Range`, 416 for one that
//! cannot be met, 404 for anything it will not serve.
//!
//! A fifth handler rather than a wider `keeper-media://`, which resolves
//! against the Matrix media cache: the two caches, their owners and their
//! authorization have nothing in common.
//!
//! **Nothing is decided here.** This crate does not build on Linux, so:
//!
//! | question | answered by |
//! |---|---|
//! | is this URL well formed, which database and blob | `keeper_core::hq::attachments::parse_blob_url` |
//! | does an attachment this replica holds name it; its bytes, cached or downloaded | `Hq::open_blob` (`attachments::open_blob`) |
//! | which `Content-Type` it may be served as (never one that runs) | `attachments::served_mime` |
//! | `Range`, statuses | `crate::media_protocol` |
//!
//! Every refusal is one 404; why is in the log, never the blob id.

use keeper_core::hq::attachments::parse_blob_url;
use tauri::http::{header, Request, Response};
use tauri::{AppHandle, Runtime, UriSchemeResponder};

use crate::media_protocol;

/// Entry point from the registered async URI-scheme protocol.
pub fn handle<R: Runtime>(
    _app: AppHandle<R>,
    request: &Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let uri = request.uri().to_string();
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let Some((database, blob_id)) = parse_blob_url(&uri) else {
        tracing::debug!("keeper-hq-blob: unresolvable URL");
        responder.respond(media_protocol::not_found());
        return;
    };
    let Some(hq) = crate::hq_ipc::service() else {
        tracing::debug!("keeper-hq-blob: the agents server is not started yet");
        responder.respond(media_protocol::not_found());
        return;
    };
    // A download waits on the replica's owner thread: off the webview's.
    tauri::async_runtime::spawn_blocking(move || {
        let response = match hq.open_blob(&database, &blob_id) {
            Ok(opened) => nosniff(match range.as_deref() {
                Some(range) => media_protocol::partial_or_full(opened.bytes, opened.mime, range),
                None => media_protocol::full_response(opened.bytes, opened.mime),
            }),
            Err(error) => {
                tracing::info!(%database, %error, "keeper-hq-blob: not served");
                media_protocol::not_found()
            }
        };
        responder.respond(response);
    });
}

/// The type served is the only type: the webview never sniffs another.
fn nosniff(mut response: Response<Vec<u8>>) -> Response<Vec<u8>> {
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        header::HeaderValue::from_static("nosniff"),
    );
    response
}
