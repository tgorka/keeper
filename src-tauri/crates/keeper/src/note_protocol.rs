//! The `keeper-note://` custom URI-scheme protocol handler (FR-109, FR-110,
//! FR-111, AD-59).
//!
//! Vault-local assets — pasted images, embedded attachments, linked files —
//! reach the webview over this scheme and never as base64 over IPC (AD-4,
//! AD-58). It is `media_protocol.rs`'s recipe: an async responder that answers
//! 200 / 206 / 416 / 404, `Range`-capable with a per-request slice cap, and
//! fire-and-forget on `responder.respond(...)` so a WebKit scheme task
//! invalidated mid-read is a no-op rather than a panic.
//!
//! It adds the thing media does not need, and the reason this is a second
//! handler rather than an overload of the first (`keeper-media://` resolves
//! against the Matrix media cache, and one bug there is worse than two
//! handlers):
//!
//! **Containment.** A note is agent-authored text, so `![](../../../../etc/passwd)`
//! is not a hypothetical — it is one line an autonomous writer can emit by
//! accident. The vault root comes from the **in-memory registry, by id**, never
//! from the URL, so the worst a forged URL can do is name a vault that does not
//! exist. Then, in order: percent-decode and refuse an absolute path, a NUL or a
//! `.git` / `.keeper` / `.obsidian` component; `canonicalize` the join and
//! require the result to be a descendant of the root's canonical form, which was
//! computed **once** at vault registration — so `..`, `..%2f`, a symlink
//! pointing out of the vault and NFC/NFD variants all collapse before the
//! comparison, and the hot path is one `canonicalize` plus one `starts_with`.
//!
//! Two more refusals ride along, because containment alone is not enough:
//! non-regular files (fifos, devices) are refused, and `Content-Type` comes from
//! an extension allow-list of image / video / audio / pdf types and never from
//! sniffing, served with `X-Content-Type-Options: nosniff` — so a `.md` file
//! cannot be coaxed into rendering as HTML inside the webview. Remote `http(s)`
//! image sources in a note body are not fetched at all (that is the editor's
//! rule, not this handler's): a note that auto-fetches a URL an agent wrote is a
//! tracking pixel, and it would falsify the NFR-11 egress claim.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Runtime, UriSchemeResponder};

use crate::notes_vault::{self, Vault, KEEPER_DIR, OBSIDIAN_DIR};

/// The URI scheme this handler serves.
pub const SCHEME: &str = "keeper-note";

/// Per-response slice ceiling.
///
/// An open-ended `bytes=0-` against a 2 GB video would otherwise allocate 2 GB to
/// answer one request. 8 MiB is far above any inline asset and well below
/// anything that hurts; `<video>` and `<audio>` reissue forward Range requests,
/// so large media simply streams in successive capped chunks.
const MAX_RANGE_CHUNK: u64 = 8 * 1024 * 1024;

/// Entry point invoked from the registered async URI-scheme protocol.
///
/// Resolution is synchronous and cheap (a registry lookup, one `canonicalize`),
/// but the read is not — a large asset on a cold or network-mounted volume can
/// block for a while — so the read and the respond run off the webview thread on
/// the blocking pool.
pub fn handle<R: Runtime>(
    app: AppHandle<R>,
    request: &Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let uri = request.uri().to_string();
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let _ = app;

    let Some((vault_id, rel)) = parse_note_url(&uri) else {
        tracing::debug!("keeper-note: unparsable URL");
        responder.respond(not_found());
        return;
    };
    // The registry is the containment: an unknown id resolves to no root at all.
    let Some(vault) = notes_vault::vault(&vault_id) else {
        tracing::debug!("keeper-note: no such vault");
        responder.respond(not_found());
        return;
    };
    let Some(path) = contained_read(&vault, &rel) else {
        // Logged without the path: a refused request is often an agent-authored
        // traversal attempt, and echoing it into the log is how a log becomes a
        // reflection surface.
        tracing::debug!(vault = %vault_id, "keeper-note: refused a path outside the vault");
        responder.respond(not_found());
        return;
    };
    let Some(mimetype) = mime_for(&rel) else {
        // Not on the allow-list: refused rather than served as
        // `application/octet-stream`, because "download whatever it is" is not
        // what an `<img>` asked for.
        tracing::debug!(vault = %vault_id, "keeper-note: extension is not on the allow-list");
        responder.respond(not_found());
        return;
    };

    tauri::async_runtime::spawn(async move {
        let response = tokio::task::spawn_blocking(move || read_response(&path, mimetype, range))
            .await
            .unwrap_or_else(|error| {
                tracing::debug!(%error, "keeper-note: read task failed");
                not_found()
            });
        responder.respond(response);
    });
}

/// Resolve a vault-relative path for reading, or `None` when it is not inside the
/// vault.
///
/// The canonicalizing half of the AD-59 check. `notes_vault::contained` is the
/// lexical half — every component a plain name — and this adds what only a
/// resolved path can answer: a symlink inside the vault pointing out of it passes
/// every string test and fails here.
pub fn contained_read(vault: &Vault, rel: &str) -> Option<PathBuf> {
    let lexical = notes_vault::contained(vault, rel).ok()?;
    // `vault.root` was canonicalized once, at registration.
    let canonical = lexical.canonicalize().ok()?;
    if !canonical.starts_with(&vault.root) {
        return None;
    }
    // A fifo or a device would block or lie about its length; only a regular
    // file is servable.
    let meta = std::fs::symlink_metadata(&canonical).ok()?;
    meta.is_file().then_some(canonical)
}

/// Parse `keeper-note://<vault_id>/<url-encoded vault-relative path>`.
///
/// Hand-rolled rather than via the `url` crate, which `keeper` does not depend
/// on: the grammar is a scheme, a host and percent-encoded path segments, and
/// the parse has to be conservative anyway. Two spellings are accepted because
/// Tauri's own rewriting differs by platform — the `<scheme>://<host>/<path>`
/// form macOS, Linux and iOS see, and the `http://<scheme>.localhost/<host>/<path>`
/// form Windows and Android use.
///
/// Returns `(vault_id, vault-relative path)`, both percent-decoded. Any query or
/// fragment is discarded: this scheme has no parameters, so a URL that carries
/// some is being probed rather than used.
fn parse_note_url(raw: &str) -> Option<(String, String)> {
    let rest = raw
        .strip_prefix(&format!("{SCHEME}://"))
        .or_else(|| raw.strip_prefix(&format!("http://{SCHEME}.localhost/")))
        .or_else(|| raw.strip_prefix(&format!("https://{SCHEME}.localhost/")))?;
    // A query or fragment ends the path. `split` always yields at least one
    // element, so the `?` is unreachable in practice and keeps this total.
    let rest = rest.split(['?', '#']).next()?;

    let mut segments = rest.split('/');
    let vault_id = decode(segments.next()?)?;
    if vault_id.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for segment in segments {
        // An empty segment is `//` or a trailing slash: neither names a file, and
        // silently collapsing them would make two URLs mean one path.
        if segment.is_empty() {
            return None;
        }
        parts.push(decode(segment)?);
    }
    if parts.is_empty() {
        return None;
    }
    let rel = parts.join("/");
    // Belt and braces: the component rules below are also enforced by
    // `notes_vault::contained`, but refusing here keeps the reason legible and
    // keeps a NUL out of a `Path` on platforms that would truncate at it.
    if rel.contains('\0')
        || rel
            .split('/')
            .any(|part| part == ".." || part == "." || part == KEEPER_DIR || part == OBSIDIAN_DIR)
    {
        return None;
    }
    Some((vault_id, rel))
}

/// Percent-decode one segment to UTF-8, or `None` when the bytes are not UTF-8.
fn decode(segment: &str) -> Option<String> {
    percent_encoding::percent_decode_str(segment)
        .decode_utf8()
        .ok()
        .map(std::borrow::Cow::into_owned)
}

/// The `Content-Type` for a vault-relative path, or `None` when its extension is
/// not on the allow-list.
///
/// An allow-list rather than sniffing, and rather than a fallback to
/// `application/octet-stream`: the webview asked for an image, a video, a sound
/// or a PDF, and anything else it can be handed is a way to make a vault file
/// render as something it is not.
fn mime_for(rel: &str) -> Option<&'static str> {
    Some(match notes_vault::extension(rel)?.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        // Served as an image, never as a document: an inline `<img src>` cannot
        // run script from an SVG, and `nosniff` plus the CSP keep it that way.
        "svg" => "image/svg+xml",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        _ => return None,
    })
}

/// Read the file (or the requested slice of it) and build the response.
///
/// Unlike `media_protocol.rs`, which slices a `Vec<u8>` the SDK already
/// materialized, this seeks: a vault asset is a file on disk, so a Range request
/// reads only the bytes it asked for and a 200 reads only up to the cap.
fn read_response(path: &Path, mimetype: &str, range: Option<String>) -> Response<Vec<u8>> {
    let Ok(mut file) = std::fs::File::open(path) else {
        return not_found();
    };
    let Ok(meta) = file.metadata() else {
        return not_found();
    };
    let total = meta.len();

    let Some(range) = range else {
        // No `Range`: serve from the start, capped. The cap is honest rather than
        // silent — `Accept-Ranges` plus a short `Content-Length` is exactly the
        // shape a media element continues from.
        let end = capped_end(0, total.saturating_sub(1));
        return match read_slice(&mut file, 0, end) {
            Some(bytes) if end.saturating_add(1) >= total => full_response(bytes, mimetype),
            Some(bytes) => partial_response(bytes, mimetype, 0, end, total),
            None => not_found(),
        };
    };

    match parse_range(&range, total) {
        RangeParse::Satisfiable { start, end } => {
            let end = capped_end(start, end);
            match read_slice(&mut file, start, end) {
                Some(bytes) => partial_response(bytes, mimetype, start, end, total),
                None => not_found(),
            }
        }
        // A malformed range serves the full body, per the same I/O matrix
        // `keeper-media://` follows.
        RangeParse::Malformed => {
            let end = capped_end(0, total.saturating_sub(1));
            match read_slice(&mut file, 0, end) {
                Some(bytes) => full_response(bytes, mimetype),
                None => not_found(),
            }
        }
        RangeParse::Unsatisfiable => Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_RANGE, format!("bytes */{total}"))
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .body(Vec::new())
            .unwrap_or_else(|_| not_found()),
    }
}

/// Clamp an inclusive `end` so the slice is at most [`MAX_RANGE_CHUNK`] bytes.
fn capped_end(start: u64, end: u64) -> u64 {
    end.min(start.saturating_add(MAX_RANGE_CHUNK.saturating_sub(1)))
}

/// Read the inclusive byte range `start..=end`, or `None` on any IO failure.
fn read_slice(file: &mut std::fs::File, start: u64, end: u64) -> Option<Vec<u8>> {
    if file.seek(SeekFrom::Start(start)).is_err() {
        return None;
    }
    let wanted = end.saturating_sub(start).saturating_add(1);
    // `take` bounds the read even if the file grew between the `metadata` call
    // and here, so a file being written cannot make this allocate without limit.
    let mut bytes = Vec::new();
    file.take(wanted).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// A 200 with `Accept-Ranges`, so the webview knows it may seek.
fn full_response(bytes: Vec<u8>, mimetype: &str) -> Response<Vec<u8>> {
    let len = bytes.len();
    base(StatusCode::OK, mimetype)
        .header(header::CONTENT_LENGTH, len)
        .body(bytes)
        .unwrap_or_else(|_| not_found())
}

/// A 206 with `Content-Range`.
fn partial_response(
    bytes: Vec<u8>,
    mimetype: &str,
    start: u64,
    end: u64,
    total: u64,
) -> Response<Vec<u8>> {
    let len = bytes.len();
    base(StatusCode::PARTIAL_CONTENT, mimetype)
        .header(header::CONTENT_LENGTH, len)
        .header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{total}"),
        )
        .body(bytes)
        .unwrap_or_else(|_| not_found())
}

/// The headers every served body carries.
///
/// `nosniff` is load-bearing rather than decorative: without it a vault file
/// whose extension says `image/png` but whose first bytes look like markup can be
/// re-interpreted by the webview, which would turn an agent-authored note into
/// an HTML injection point.
fn base(status: StatusCode, mimetype: &str) -> tauri::http::response::Builder {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mimetype)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
}

/// A 404 with an empty body — the honest "no such asset" answer, and the single
/// answer every refusal collapses to, so a probe cannot tell "outside the vault"
/// from "not there".
fn not_found() -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Vec::new())
        // A builder carrying only a status and one static header cannot fail to
        // build; the fallback keeps the function total without an `.expect()`.
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

/// The outcome of parsing a single-range `Range` header against a known total.
#[derive(Debug, PartialEq, Eq)]
enum RangeParse {
    /// A satisfiable inclusive byte range `start..=end`.
    Satisfiable { start: u64, end: u64 },
    /// Not a well-formed single `bytes=` range → serve the full body.
    Malformed,
    /// Well-formed, but its start is past the content end → 416.
    Unsatisfiable,
}

/// Parse an HTTP `Range` header value against `total`.
///
/// The single-range forms `<video>`/`<audio>` emit: `bytes=start-end`,
/// `bytes=start-` and `bytes=-suffix`. A multi-range header, a non-`bytes` unit
/// or a malformed value is [`RangeParse::Malformed`]. Pure — unit-tested.
fn parse_range(raw: &str, total: u64) -> RangeParse {
    let Some(spec) = raw.trim().strip_prefix("bytes=") else {
        return RangeParse::Malformed;
    };
    // A multi-range request would need a multipart body; keeper serves one range.
    if spec.contains(',') {
        return RangeParse::Malformed;
    }
    let Some((start, end)) = spec.split_once('-') else {
        return RangeParse::Malformed;
    };
    let (start, end) = (start.trim(), end.trim());
    match (start.is_empty(), end.is_empty()) {
        // `bytes=-N`: the last N bytes.
        (true, false) => {
            let Ok(suffix) = end.parse::<u64>() else {
                return RangeParse::Malformed;
            };
            if suffix == 0 || total == 0 {
                return RangeParse::Unsatisfiable;
            }
            RangeParse::Satisfiable {
                start: total.saturating_sub(suffix),
                end: total - 1,
            }
        }
        // `bytes=N-` or `bytes=N-M`.
        (false, _) => {
            let Ok(start) = start.parse::<u64>() else {
                return RangeParse::Malformed;
            };
            if start >= total {
                return RangeParse::Unsatisfiable;
            }
            let end = if end.is_empty() {
                total - 1
            } else {
                match end.parse::<u64>() {
                    Ok(end) => end.min(total - 1),
                    Err(_) => return RangeParse::Malformed,
                }
            };
            if end < start {
                return RangeParse::Unsatisfiable;
            }
            RangeParse::Satisfiable { start, end }
        }
        // `bytes=-` names nothing.
        (true, true) => RangeParse::Malformed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_parses_into_a_vault_id_and_a_relative_path() {
        assert_eq!(
            parse_note_url("keeper-note://01VAULT/attachments/a%20b.png"),
            Some(("01VAULT".to_owned(), "attachments/a b.png".to_owned()))
        );
        // The Windows/Android spelling Tauri rewrites to.
        assert_eq!(
            parse_note_url("http://keeper-note.localhost/01VAULT/shot.png"),
            Some(("01VAULT".to_owned(), "shot.png".to_owned()))
        );
        // A query or fragment is discarded — this scheme has no parameters.
        assert_eq!(
            parse_note_url("keeper-note://01VAULT/shot.png?v=2"),
            Some(("01VAULT".to_owned(), "shot.png".to_owned()))
        );
        assert_eq!(
            parse_note_url("keeper-note://01VAULT/shot.png#frag"),
            Some(("01VAULT".to_owned(), "shot.png".to_owned()))
        );
    }

    #[test]
    fn a_url_that_could_escape_is_refused_before_any_filesystem_call() {
        // Traversal, plain and percent-encoded — `%2E%2E` decodes to `..`, which
        // is why the component check runs after decoding and not before.
        assert!(parse_note_url("keeper-note://01VAULT/../secrets.png").is_none());
        assert!(parse_note_url("keeper-note://01VAULT/%2E%2E/secrets.png").is_none());
        assert!(parse_note_url("keeper-note://01VAULT/a/./b.png").is_none());
        // keeper's and Obsidian's own directories.
        assert!(parse_note_url("keeper-note://01VAULT/.keeper/index.json").is_none());
        assert!(parse_note_url("keeper-note://01VAULT/.obsidian/workspace.json").is_none());
        // A NUL.
        assert!(parse_note_url("keeper-note://01VAULT/shot%00.png").is_none());
        // No vault, no path, empty segments.
        assert!(parse_note_url("keeper-note:///shot.png").is_none());
        assert!(parse_note_url("keeper-note://01VAULT").is_none());
        assert!(parse_note_url("keeper-note://01VAULT/").is_none());
        assert!(parse_note_url("keeper-note://01VAULT//shot.png").is_none());
        // A foreign scheme — including the media scheme, which must never be
        // answered by this handler.
        assert!(parse_note_url("keeper-media://media/a/b/c/full").is_none());
        assert!(parse_note_url("file:///etc/passwd").is_none());
    }

    #[test]
    fn only_allow_listed_extensions_are_served() {
        assert_eq!(mime_for("attachments/shot.PNG"), Some("image/png"));
        assert_eq!(mime_for("a/b/clip.mov"), Some("video/quicktime"));
        assert_eq!(mime_for("paper.pdf"), Some("application/pdf"));
        // A note, a script, an executable and a bare name are all refused —
        // never served as `application/octet-stream`.
        assert_eq!(mime_for("note.md"), None);
        assert_eq!(mime_for("run.sh"), None);
        assert_eq!(mime_for("index.html"), None);
        assert_eq!(mime_for("Makefile"), None);
        assert_eq!(mime_for(".gitignore"), None);
    }

    #[test]
    fn a_range_header_parses_into_the_forms_media_elements_emit() {
        assert_eq!(
            parse_range("bytes=0-99", 1_000),
            RangeParse::Satisfiable { start: 0, end: 99 }
        );
        assert_eq!(
            parse_range("bytes=500-", 1_000),
            RangeParse::Satisfiable {
                start: 500,
                end: 999
            }
        );
        assert_eq!(
            parse_range("bytes=-100", 1_000),
            RangeParse::Satisfiable {
                start: 900,
                end: 999
            }
        );
        // An end past the content is clamped, not refused.
        assert_eq!(
            parse_range("bytes=900-5000", 1_000),
            RangeParse::Satisfiable {
                start: 900,
                end: 999
            }
        );
        // A start past the end is the one 416 case.
        assert_eq!(parse_range("bytes=1000-", 1_000), RangeParse::Unsatisfiable);
        assert_eq!(parse_range("bytes=5-1", 1_000), RangeParse::Unsatisfiable);
        // Malformed values serve the full body rather than failing.
        assert_eq!(parse_range("items=0-1", 1_000), RangeParse::Malformed);
        assert_eq!(parse_range("bytes=0-1,5-6", 1_000), RangeParse::Malformed);
        assert_eq!(parse_range("bytes=-", 1_000), RangeParse::Malformed);
        assert_eq!(parse_range("bytes=abc-def", 1_000), RangeParse::Malformed);
        assert_eq!(parse_range("nonsense", 1_000), RangeParse::Malformed);
    }

    #[test]
    fn a_slice_is_capped_so_one_request_cannot_allocate_a_whole_video() {
        assert_eq!(capped_end(0, 10), 10, "a small range is untouched");
        assert_eq!(
            capped_end(0, 64 * 1024 * 1024),
            MAX_RANGE_CHUNK - 1,
            "an open-ended range is clamped to the cap"
        );
        assert_eq!(
            capped_end(1_000, u64::MAX),
            1_000 + MAX_RANGE_CHUNK - 1,
            "the cap is relative to the start and never overflows"
        );
    }

    #[test]
    fn every_refusal_answers_404_with_nosniff() {
        let response = not_found();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(response.body().is_empty());
        assert_eq!(
            response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .and_then(|value| value.to_str().ok()),
            Some("nosniff")
        );
    }
}
