//! The `keeper-media://` custom URI-scheme protocol handler (Story 3.6, FR-13,
//! AD-4, NFR-9).
//!
//! The **exclusive** transport for decrypted media bytes into the webview: the
//! handler parses the request URI back into a [`MediaHandle`], runs the async SDK
//! fetch off the webview thread (`tauri::async_runtime::spawn`), and responds with
//! an `http::Response` — 200 (full body), 206 (a `Range`-sliced body with
//! `Content-Range`), 416 (an unsatisfiable range), or 404 (parse/resolve/fetch
//! failure). No media bytes ever traverse the IPC channel (AD-4).
//!
//! `get_media_content` is atomic in matrix-sdk 0.18 (whole `Vec<u8>`, no
//! byte-progress callback), so the handler fetches the full (SDK-cached,
//! decrypted) bytes and slices per `Range` in memory — the protocol is
//! Range-capable *from the cache*, exactly as the AC requires. Retry is
//! pure-frontend (re-set `src` with a cache-busting suffix → re-request →
//! re-fetch on a cache miss).
//!
//! **iOS (Story 12.4):** the same protocol runs on iOS via wry →
//! `WKURLSchemeHandler` (registration is unconditional). There the Range slice
//! is capped at [`MAX_MEDIA_RANGE_CHUNK`] as a jetsam guard: an open-ended
//! `bytes=0-` would otherwise `.to_vec()` the whole body, allocating a second
//! full-size copy alongside the in-memory media and pushing peak past the iOS
//! jetsam limit. The cap bounds that copy; the webview transparently continues
//! with follow-up Range requests. A WebKit scheme task invalidated mid-fetch is
//! handled by the fire-and-forget `responder.respond(...)` (best-effort — on
//! an invalidated task it is a no-op, not a panic); on-device tolerance is
//! confirmed in Story 12.6. Retry-on-cold-cache is the same pure-frontend path.

use keeper_core::media::{parse_media_url, MediaBytes, MediaHandle};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager, Runtime, UriSchemeResponder};

use crate::ipc::AppState;

/// Entry point invoked from the registered async URI-scheme protocol. Parses the
/// handle from the URI, then spawns the async fetch + respond off-thread so the
/// SDK download/decrypt never blocks the webview thread.
pub fn handle<R: Runtime>(
    app: AppHandle<R>,
    request: &Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let uri = request.uri().to_string();
    // Parse the `Range` header (if any) up front — it is the only request datum
    // besides the URI the handler needs, so we needn't move the whole request.
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let Some(handle) = parse_media_url(&uri) else {
        // An unparsable / foreign handle → 404, never a panic or a blank.
        tracing::debug!("keeper-media: unresolvable handle");
        responder.respond(not_found());
        return;
    };

    // Run the async SDK fetch off the webview thread; respond from the task.
    tauri::async_runtime::spawn(async move {
        let response = build_response(&app, &handle, range.as_deref()).await;
        responder.respond(response);
    });
}

/// Fetch the media for `handle` and build the HTTP response, honoring an optional
/// `Range` header. Resolves state from the app handle; a resolve/fetch failure is
/// a 404 (the frontend retries once the media is resolvable).
async fn build_response<R: Runtime>(
    app: &AppHandle<R>,
    handle: &MediaHandle,
    range: Option<&str>,
) -> Response<Vec<u8>> {
    let state = app.state::<AppState>();
    let variant = handle.variant;
    let fetched = state
        .accounts
        .fetch_media(
            &handle.account_id,
            &handle.room_id,
            &handle.item_key,
            variant,
        )
        .await;
    let MediaBytes { bytes, mimetype } = match fetched {
        Ok(bytes) => bytes,
        Err(e) => {
            // Room id is logged by the core; here log only the failure class.
            tracing::debug!(error = %e, "keeper-media: fetch failed");
            return not_found();
        }
    };
    match range {
        Some(range) => partial_or_full(bytes, &mimetype, range),
        None => full_response(bytes, &mimetype),
    }
}

/// A 200 OK full-body response with `Content-Type` and `Accept-Ranges: bytes` so
/// the webview knows it may issue subsequent `Range` requests (video/audio seek).
fn full_response(bytes: Vec<u8>, mimetype: &str) -> Response<Vec<u8>> {
    let len = bytes.len();
    build(StatusCode::OK, mimetype, len)
        .header(header::CONTENT_LENGTH, len)
        .body(bytes)
        .unwrap_or_else(|_| not_found())
}

/// Per-response Range slice ceiling. iOS: a hard jetsam guard so one open-ended
/// `bytes=0-` cannot allocate a second full-size copy of large media. The 8 MiB
/// value sits well under the epic's 25 MB media bar, so legitimate large media
/// simply streams in successive capped chunks (`<video>`/`<audio>` reissue
/// forward Range requests). Desktop: `u64::MAX` — a no-op, keeping desktop Range
/// behavior byte-identical.
#[cfg(target_os = "ios")]
const MAX_MEDIA_RANGE_CHUNK: u64 = 8 * 1024 * 1024;
#[cfg(not(target_os = "ios"))]
const MAX_MEDIA_RANGE_CHUNK: u64 = u64::MAX;

/// Clamp an inclusive range `end` so the slice is at most `cap` bytes. Pure.
/// `saturating_sub` keeps it total even for a degenerate `cap` of 0 (no
/// underflow), though both `MAX_MEDIA_RANGE_CHUNK` values are far larger.
fn capped_range_end(start: u64, end: u64, cap: u64) -> u64 {
    end.min(start.saturating_add(cap.saturating_sub(1)))
}

/// Build a 206 Partial Content response for a satisfiable `Range`, a 200 for a
/// malformed range (serve the full body), or a 416 for an unsatisfiable range.
fn partial_or_full(bytes: Vec<u8>, mimetype: &str, range: &str) -> Response<Vec<u8>> {
    let total = bytes.len() as u64;
    match parse_range(range, total) {
        // A satisfiable byte range → 206 with the slice + Content-Range.
        RangeParse::Satisfiable { start, end } => {
            // Cap the slice at `MAX_MEDIA_RANGE_CHUNK` (iOS jetsam guard; a
            // `u64::MAX` no-op on desktop) before slicing, so both the 206 body
            // and the `Content-Range` header reflect the clamped end.
            let end = capped_range_end(start, end, MAX_MEDIA_RANGE_CHUNK);
            // `end` is inclusive; slice is `start..=end`.
            let slice = bytes
                .get(start as usize..=end as usize)
                .map(<[u8]>::to_vec)
                .unwrap_or_default();
            let slice_len = slice.len();
            build(StatusCode::PARTIAL_CONTENT, mimetype, slice_len)
                .header(header::CONTENT_LENGTH, slice_len)
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{end}/{total}"),
                )
                .body(slice)
                .unwrap_or_else(|_| not_found())
        }
        // A malformed / unparsable range → serve the full body (200), per the
        // I/O matrix ("Malformed Range → 200 full body or 416").
        RangeParse::Malformed => full_response(bytes, mimetype),
        // A well-formed but unsatisfiable range (start past the end) → 416.
        RangeParse::Unsatisfiable => Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_RANGE, format!("bytes */{total}"))
            .body(Vec::new())
            .unwrap_or_else(|_| not_found()),
    }
}

/// Start an `http::response::Builder` with the status, `Content-Type`, and
/// `Accept-Ranges: bytes` common to every media body response. `_len` is passed so
/// callers keep the length in scope; the header is set by the caller.
fn build(status: StatusCode, mimetype: &str, _len: usize) -> tauri::http::response::Builder {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mimetype)
        .header(header::ACCEPT_RANGES, "bytes")
}

/// A 404 Not Found with an empty body — the honest "handle unresolvable" response
/// the frontend turns into a retry affordance.
fn not_found() -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Vec::new())
        // A builder with only a status can't fail to build; this fallback keeps
        // the fn total without an `.expect()` in a production path.
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

/// The outcome of parsing a single-range `Range` header against a known total.
#[derive(Debug, PartialEq, Eq)]
enum RangeParse {
    /// A satisfiable inclusive byte range `start..=end`.
    Satisfiable { start: u64, end: u64 },
    /// The header was not a well-formed single `bytes=` range → serve full (200).
    Malformed,
    /// A well-formed range whose start is past the content end → 416.
    Unsatisfiable,
}

/// Parse an HTTP `Range` header value against `total` content length (Story 3.6).
///
/// Supports the single-range forms the webview's `<video>`/`<audio>` emit:
/// `bytes=start-end`, `bytes=start-` (to end), and `bytes=-suffix` (last N bytes).
/// A multi-range header, a non-`bytes` unit, or a malformed value returns
/// [`RangeParse::Malformed`] (→ serve the full body). A well-formed range whose
/// `start >= total` is [`RangeParse::Unsatisfiable`] (→ 416). Pure — unit-tested.
fn parse_range(raw: &str, total: u64) -> RangeParse {
    let Some(spec) = raw.trim().strip_prefix("bytes=") else {
        return RangeParse::Malformed;
    };
    // Only a single range is supported; a comma means a multi-range request.
    if spec.contains(',') {
        return RangeParse::Malformed;
    }
    let Some((start_s, end_s)) = spec.split_once('-') else {
        return RangeParse::Malformed;
    };
    let start_s = start_s.trim();
    let end_s = end_s.trim();

    // Empty content can never satisfy a range.
    if total == 0 {
        return RangeParse::Unsatisfiable;
    }
    let last = total - 1;

    match (start_s.is_empty(), end_s.is_empty()) {
        // `bytes=-N` → the last N bytes.
        (true, false) => {
            let Ok(suffix) = end_s.parse::<u64>() else {
                return RangeParse::Malformed;
            };
            if suffix == 0 {
                return RangeParse::Unsatisfiable;
            }
            let len = suffix.min(total);
            RangeParse::Satisfiable {
                start: total - len,
                end: last,
            }
        }
        // `bytes=start-` → start to the end.
        (false, true) => {
            let Ok(start) = start_s.parse::<u64>() else {
                return RangeParse::Malformed;
            };
            if start > last {
                return RangeParse::Unsatisfiable;
            }
            RangeParse::Satisfiable { start, end: last }
        }
        // `bytes=start-end` → an explicit inclusive range.
        (false, false) => {
            let (Ok(start), Ok(end)) = (start_s.parse::<u64>(), end_s.parse::<u64>()) else {
                return RangeParse::Malformed;
            };
            if start > end || start > last {
                return RangeParse::Unsatisfiable;
            }
            // Clamp the end to the last byte (a client may over-request).
            RangeParse::Satisfiable {
                start,
                end: end.min(last),
            }
        }
        // `bytes=-` (both empty) is malformed.
        (true, true) => RangeParse::Malformed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_range() {
        assert_eq!(
            parse_range("bytes=0-99", 1000),
            RangeParse::Satisfiable { start: 0, end: 99 }
        );
        assert_eq!(
            parse_range("bytes=100-199", 1000),
            RangeParse::Satisfiable {
                start: 100,
                end: 199
            }
        );
    }

    #[test]
    fn parses_open_ended_range() {
        assert_eq!(
            parse_range("bytes=500-", 1000),
            RangeParse::Satisfiable {
                start: 500,
                end: 999
            }
        );
    }

    #[test]
    fn parses_suffix_range() {
        assert_eq!(
            parse_range("bytes=-100", 1000),
            RangeParse::Satisfiable {
                start: 900,
                end: 999
            }
        );
        // A suffix larger than the content clamps to the whole body.
        assert_eq!(
            parse_range("bytes=-5000", 1000),
            RangeParse::Satisfiable { start: 0, end: 999 }
        );
    }

    #[test]
    fn clamps_over_requested_end() {
        assert_eq!(
            parse_range("bytes=990-5000", 1000),
            RangeParse::Satisfiable {
                start: 990,
                end: 999
            }
        );
    }

    #[test]
    fn malformed_ranges_serve_full() {
        assert_eq!(parse_range("items=0-99", 1000), RangeParse::Malformed);
        assert_eq!(parse_range("bytes=abc-def", 1000), RangeParse::Malformed);
        assert_eq!(
            parse_range("bytes=0-99,200-299", 1000),
            RangeParse::Malformed
        );
        assert_eq!(parse_range("garbage", 1000), RangeParse::Malformed);
        assert_eq!(parse_range("bytes=-", 1000), RangeParse::Malformed);
    }

    #[test]
    fn unsatisfiable_ranges() {
        // Start past the end.
        assert_eq!(
            parse_range("bytes=2000-3000", 1000),
            RangeParse::Unsatisfiable
        );
        assert_eq!(parse_range("bytes=1000-", 1000), RangeParse::Unsatisfiable);
        // Any range against empty content.
        assert_eq!(parse_range("bytes=0-99", 0), RangeParse::Unsatisfiable);
        // A zero-length suffix.
        assert_eq!(parse_range("bytes=-0", 1000), RangeParse::Unsatisfiable);
    }

    #[test]
    fn full_response_sets_accept_ranges_and_type() {
        let resp = full_response(vec![1, 2, 3, 4], "image/png");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .expect("content-type header"),
            "image/png"
        );
        assert_eq!(
            resp.headers()
                .get(header::ACCEPT_RANGES)
                .expect("accept-ranges header"),
            "bytes"
        );
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_LENGTH)
                .expect("content-length header"),
            "4"
        );
        assert_eq!(resp.body(), &vec![1, 2, 3, 4]);
    }

    #[test]
    fn partial_response_slices_and_sets_content_range() {
        let bytes = (0u8..=9).collect::<Vec<u8>>();
        let resp = partial_or_full(bytes, "video/mp4", "bytes=2-5");
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_RANGE)
                .expect("content-range header"),
            "bytes 2-5/10"
        );
        assert_eq!(resp.body(), &vec![2u8, 3, 4, 5]);
    }

    #[test]
    fn capped_range_end_clamps_saturates_and_noops() {
        // Over-long range clamps to exactly `cap` bytes (inclusive end).
        assert_eq!(capped_range_end(0, 100, 8), 7);
        assert_eq!(
            capped_range_end(10, u64::MAX, 8 * 1024 * 1024),
            10 + 8 * 1024 * 1024 - 1
        );
        // No-op when the range already fits within the cap.
        assert_eq!(capped_range_end(2, 5, 8 * 1024 * 1024), 5);
        // No-op when `cap == u64::MAX` (desktop byte-identical guarantee).
        assert_eq!(capped_range_end(0, 999, u64::MAX), 999);
        assert_eq!(capped_range_end(500, u64::MAX, u64::MAX), u64::MAX);
        // Saturates near `u64::MAX` without overflow.
        assert_eq!(
            capped_range_end(u64::MAX - 2, u64::MAX, 8 * 1024 * 1024),
            u64::MAX
        );
    }

    #[test]
    fn malformed_range_yields_full_body() {
        let bytes = vec![7u8; 8];
        let resp = partial_or_full(bytes.clone(), "application/octet-stream", "bytes=xyz");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.body(), &bytes);
    }

    #[test]
    fn unsatisfiable_range_yields_416() {
        let bytes = vec![7u8; 8];
        let resp = partial_or_full(bytes, "application/octet-stream", "bytes=100-200");
        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_RANGE)
                .expect("content-range header"),
            "bytes */8"
        );
    }

    #[test]
    fn not_found_is_404_empty() {
        let resp = not_found();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert!(resp.body().is_empty());
    }
}
