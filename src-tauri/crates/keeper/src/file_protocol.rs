//! The `keeper-file://` custom URI-scheme protocol handler (Story 45.7,
//! FR-180, AD-59, AD-65, AD-74).
//!
//! **A fourth handler, and the reason is the same reason there was a third.**
//! Story 45.7 asks for images, audio and video to open in a panel "over
//! `keeper-recording://` with its range support". They cannot: that scheme is
//! rooted at the effective recordings destination and resolves by SESSION id,
//! and AD-74 says the Files surface must not reach for it — `sync_open_entry`
//! already refused the same shortcut, for the same reason, and worded it there.
//! `keeper-note://` is rooted at `vault.root`, which is `local_path/subfolder`
//! and therefore narrower than the tree the Files pane browses. So: the same
//! treatment with a fourth fixed root, the sync profile's own `local_path`.
//!
//! **Almost nothing is decided here, on purpose.** This crate does not build on
//! Linux (AD-55, AD-56), and a protocol handler is the last place to put a rule
//! that can only be compiled on one platform. So:
//!
//! | question | answered by | compiled and tested on |
//! |---|---|---|
//! | is this URL well formed, and what are its two halves | [`keeper_core::file_asset::parse_file_url`] | any machine |
//! | may this FORMAT's bytes reach the webview | [`keeper_core::file_asset::is_servable_path`] | any machine |
//! | is this a profile keeper knows, and is this path inside it | [`keeper_sync::file_serve::resolve_served_path`] | any machine |
//! | what `Content-Type`, what `Range`, what status | [`crate::note_protocol`] | macOS |
//!
//! What is left below is the wiring: read the request, hop to the blocking
//! pool, call those four, respond. There is no path arithmetic in this file and
//! there must never be any.
//!
//! **Every refusal collapses to one 404.** A probe must not be able to tell
//! "outside the folder" from "not there" from "no such profile" by reading the
//! status, which is `note_protocol`'s rule and is inherited rather than
//! restated. The distinction survives in the log, without the path.

use std::sync::Arc;

use keeper_sync::file_serve::{self, ServeRefusal};
use keeper_sync::profile::SyncProfile;
use tauri::http::{header, Request, Response};
use tauri::{AppHandle, Manager, Runtime, UriSchemeResponder};

use crate::ipc::AppState;
use crate::note_protocol;

/// The URI scheme this handler serves.
///
/// Re-exported from `keeper-core` rather than spelled again: the grammar and
/// the registration have to name the same scheme, and two string literals is
/// how they stop doing so.
pub const SCHEME: &str = keeper_core::file_asset::SCHEME;

/// Entry point invoked from the registered async URI-scheme protocol.
///
/// **Everything after the URL parse runs on the blocking pool.** Listing
/// profiles opens `sync.db`, and resolving canonicalizes a path that may sit on
/// a removable or network volume — doing either on the webview thread would
/// stall the UI on exactly the media the user is trying to watch.
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

    let Some((profile_id, rel)) = keeper_core::file_asset::parse_file_url(&uri) else {
        tracing::debug!("keeper-file: unparsable URL");
        responder.respond(note_protocol::not_found());
        return;
    };

    // Before the engine is even asked for. A name test, no path work, no disk —
    // so it cannot disturb the rule that an unknown profile id is refused
    // before anything is joined or canonicalized, and it means a probe for
    // `/.ssh/id_rsa` never reaches sqlite.
    if !keeper_core::file_asset::is_servable_path(&rel) {
        tracing::debug!(profile = %profile_id, "keeper-file: that format is not served");
        responder.respond(note_protocol::not_found());
        return;
    }

    // `State` borrows the app handle and cannot cross into the task, so the one
    // value only the app state can give is taken here.
    let platform = {
        let state = app.state::<AppState>();
        Arc::clone(&state.platform)
    };

    tauri::async_runtime::spawn(async move {
        let response = tokio::task::spawn_blocking(move || {
            let Ok(engine) = crate::sync::engine(platform) else {
                tracing::info!("keeper-file: the sync engine is unavailable, so nothing is served");
                return note_protocol::not_found();
            };
            let profiles: Vec<SyncProfile> = match engine.list_profiles() {
                Ok(profiles) => profiles,
                Err(error) => {
                    tracing::info!(%error, "keeper-file: profiles could not be listed");
                    return note_protocol::not_found();
                }
            };
            serve(&profiles, &profile_id, &rel, range)
        })
        .await
        .unwrap_or_else(|error| {
            tracing::debug!(%error, "keeper-file: read task failed");
            note_protocol::not_found()
        });
        responder.respond(response);
    });
}

/// Resolve one request and answer it. Blocking: a canonicalize and the file
/// read itself.
///
/// Split from [`handle`] so the whole of the answer — including the `Range`
/// shapes — is reachable from a test with a real temp directory and no Tauri
/// app, which is what the tests at the bottom of this file drive.
fn serve(
    profiles: &[SyncProfile],
    profile_id: &str,
    rel: &str,
    range: Option<String>,
) -> Response<Vec<u8>> {
    let path = match file_serve::resolve_served_path(profiles, profile_id, rel) {
        Ok(path) => path,
        Err(refusal) => {
            // The refusal, never the path: a refused request is often a probe,
            // and echoing its path into the log is how a log becomes a
            // reflection surface. `info!` rather than `debug!` because a media
            // element that shows nothing is otherwise silent, and "why did my
            // video not open" is a question the log has to already answer
            // (DW-162). The profile id is keeper's own identifier and says
            // nothing about the filesystem.
            tracing::info!(profile = %profile_id, %refusal, "keeper-file: refused");
            return match refusal {
                // Same status for every one of them. The variant is for the log
                // and the test, not for the caller.
                ServeRefusal::UnknownProfile
                | ServeRefusal::Escapes(_)
                | ServeRefusal::Missing
                | ServeRefusal::NotAFile => note_protocol::not_found(),
            };
        }
    };
    let Some(mimetype) = note_protocol::mime_for(rel) else {
        // Unreachable while `is_servable_path` and `mime_for` agree, which the
        // test below pins, and kept because the day they stop agreeing this
        // must be a 404 rather than a file served with a guessed type.
        tracing::info!(profile = %profile_id, "keeper-file: no Content-Type for that extension");
        return note_protocol::not_found();
    };
    note_protocol::read_response(&path, mimetype, range)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tauri::http::StatusCode;

    /// The slice ceiling `note_protocol` imposes. Spelled here rather than
    /// imported because the assertion is about the OBSERVED body, and a test
    /// that reads the same constant the code reads cannot notice the constant
    /// changing to something absurd.
    const EIGHT_MIB: usize = 8 * 1024 * 1024;

    fn profile(id: &str, local_path: &std::path::Path) -> SyncProfile {
        SyncProfile::new(id, "Field", local_path, "https://example.invalid/r.git")
    }

    /// A profile root holding one small video and one file larger than the
    /// slice cap, so both sides of the cap are exercised against real bytes.
    fn tree() -> (tempfile::TempDir, Vec<SyncProfile>) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("folder");
        fs::create_dir_all(&root).expect("mkdir");
        fs::write(root.join("clip.mov"), b"0123456789").expect("write");
        fs::write(root.join("big.mov"), vec![7u8; EIGHT_MIB + 4096]).expect("write");
        fs::write(root.join("notes.md"), b"# not served").expect("write");
        (dir, vec![profile("01PROFILE", &root)])
    }

    fn body_len(response: &Response<Vec<u8>>) -> usize {
        response.body().len()
    }

    #[test]
    fn serves_a_whole_small_file_as_a_200_that_advertises_range_support() {
        let (_dir, profiles) = tree();
        let response = serve(&profiles, "01PROFILE", "clip.mov", None);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"0123456789");
        assert_eq!(
            response
                .headers()
                .get(header::ACCEPT_RANGES)
                .expect("header::ACCEPT_RANGES is the subject of this assertion"),
            "bytes"
        );
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .expect("header::CONTENT_TYPE is the subject of this assertion"),
            "video/quicktime"
        );
        assert_eq!(
            response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .expect("header::X_CONTENT_TYPE_OPTIONS is the subject of this assertion"),
            "nosniff"
        );
    }

    #[test]
    fn a_range_request_is_a_206_with_only_the_bytes_it_asked_for() {
        let (_dir, profiles) = tree();
        let response = serve(
            &profiles,
            "01PROFILE",
            "clip.mov",
            Some("bytes=2-5".to_owned()),
        );
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body(), b"2345");
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_RANGE)
                .expect("header::CONTENT_RANGE is the subject of this assertion"),
            "bytes 2-5/10"
        );
    }

    #[test]
    fn a_range_that_starts_past_the_end_is_a_416_and_not_an_empty_200() {
        // The distinction a `<video>` acts on: a 416 tells it the seek was out
        // of bounds, an empty 200 tells it the file is empty and it stops.
        let (_dir, profiles) = tree();
        for header_value in ["bytes=10-", "bytes=99999-100000", "bytes=-0"] {
            let response = serve(
                &profiles,
                "01PROFILE",
                "clip.mov",
                Some(header_value.to_owned()),
            );
            assert_eq!(
                response.status(),
                StatusCode::RANGE_NOT_SATISFIABLE,
                "{header_value}"
            );
            assert_eq!(
                response
                    .headers()
                    .get(header::CONTENT_RANGE)
                    .expect("header::CONTENT_RANGE is the subject of this assertion"),
                "bytes */10",
                "{header_value}"
            );
            assert!(response.body().is_empty(), "{header_value}");
        }
    }

    #[test]
    fn a_malformed_range_serves_the_whole_body_bounded_rather_than_failing() {
        // `note_protocol`'s stated I/O matrix, inherited: a header keeper cannot
        // parse is treated as no header at all. The point of the assertion is
        // that it is BOUNDED — a garbage header must not become an unbounded
        // read.
        let (_dir, profiles) = tree();
        for header_value in [
            "bytes=abc",
            "bytes=1-2,4-5",
            "items=0-1",
            "bytes",
            "",
            "bytes=-",
        ] {
            let response = serve(
                &profiles,
                "01PROFILE",
                "clip.mov",
                Some(header_value.to_owned()),
            );
            assert_eq!(response.status(), StatusCode::OK, "{header_value}");
            assert!(body_len(&response) <= EIGHT_MIB, "{header_value}");
        }
    }

    #[test]
    fn an_absurd_range_is_clamped_to_the_slice_cap_rather_than_read_whole() {
        // A `bytes=0-` against a multi-gigabyte screen recording is exactly what
        // a media element sends first. Answering it literally would allocate the
        // whole file into the webview's process.
        let (_dir, profiles) = tree();
        let response = serve(
            &profiles,
            "01PROFILE",
            "big.mov",
            Some("bytes=0-99999999999999".to_owned()),
        );
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(body_len(&response), EIGHT_MIB);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_RANGE)
                .expect("header::CONTENT_RANGE is the subject of this assertion"),
            &format!("bytes 0-{}/{}", EIGHT_MIB - 1, EIGHT_MIB + 4096)
        );
        // And with no header at all, which is the other way the same file can be
        // asked for in full.
        let whole = serve(&profiles, "01PROFILE", "big.mov", None);
        assert_eq!(body_len(&whole), EIGHT_MIB);
    }

    #[test]
    fn every_refusal_is_the_same_404_so_a_probe_learns_nothing_from_the_status() {
        let (_dir, profiles) = tree();
        for (id, rel) in [
            ("01NOBODY", "clip.mov"),
            ("01PROFILE", "../../../etc/passwd"),
            ("01PROFILE", "/etc/passwd"),
            ("01PROFILE", "gone.mov"),
        ] {
            let response = serve(&profiles, id, rel, None);
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{id} {rel}");
            assert!(response.body().is_empty(), "{id} {rel}");
        }
    }

    #[test]
    fn a_format_that_is_not_served_never_reaches_the_resolver() {
        // Asserted through `handle`'s gate rather than `serve`'s, because the
        // gate is the thing being claimed: a `.md` in a real profile at a real
        // path is refused by the format question and not by any later one.
        assert!(!keeper_core::file_asset::is_servable_path("notes.md"));
    }

    #[test]
    fn every_format_this_scheme_serves_has_a_content_type() {
        // The two allow-lists live in different crates and would otherwise be
        // free to disagree, which shows up as a file that resolves and is then
        // 404'd for having no `Content-Type` — a refusal with no reason a reader
        // could act on.
        use keeper_core::archive::recordings_fts::{
            AUDIO_EXTENSIONS, IMAGE_EXTENSIONS, VIDEO_EXTENSIONS,
        };
        for extension in VIDEO_EXTENSIONS
            .iter()
            .chain(IMAGE_EXTENSIONS.iter())
            .chain(AUDIO_EXTENSIONS.iter())
            .chain(["pdf"].iter())
        {
            let name = format!("a.{extension}");
            assert!(
                keeper_core::file_asset::is_servable_path(&name),
                "{extension} is not served"
            );
            assert!(
                note_protocol::mime_for(&name).is_some(),
                "{extension} is served with no Content-Type"
            );
        }
    }
}
