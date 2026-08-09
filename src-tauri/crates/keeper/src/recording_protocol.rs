//! The `keeper-recording://` custom URI-scheme protocol handler (Story 42.4,
//! FR-142, FR-145, AD-59, AD-65).
//!
//! A recording note can name its recording only in relative terms, because
//! FR-145 keeps absolute paths out of a file the user syncs between machines.
//! `recording_note_targets` already turns that text into something an *action*
//! can open. This scheme is the other half of the same problem: turning it into
//! something a `<video>` can **play**, without the webview ever seeing a root.
//!
//! **Why a third handler and not a wider second one.** `keeper-note://` is
//! contained to a vault, and that containment is the feature, not an
//! inconvenience: AD-59's rule is that a protocol handler is a hole in the
//! sandbox unless its root is fixed and every request is checked against it.
//! `RecordingsConfig::validate` refuses a recordings root that overlaps a vault,
//! so a recording file is *provably* outside every vault and widening
//! `keeper-note://` to reach one would mean deleting the check that makes it
//! safe. This handler gets the identical treatment with a different fixed root:
//! the EFFECTIVE recordings destination (Story 41.2).
//!
//! **Resolution is by session id, never by joining the URL onto a root.** The
//! host is the session's immutable identity and the path is one of its targets,
//! and the answer comes from [`keeper_core::archive::recordings_fts::session_note_targets`]
//! — the same composer the IPC command uses. So the handler never builds a path
//! out of caller-supplied text at all: it lists what the session actually has
//! and picks a member by name. Containment is then true by construction, and a
//! Story 40.4 retitle cannot strand a URL the webview is already holding, since
//! every request re-resolves the folder from the index.
//!
//! Two refusals ride along anyway, because "true by construction" is a claim and
//! a check is a fact: the resolved path is canonicalized and must still be a
//! descendant of the canonical destination root — a symlink inside a session
//! folder pointing at `~/.ssh` passes every string test and fails here — and
//! only a regular file is servable.
//!
//! **Only the kinds a note renders inline are served.** Story 42.4 served
//! `kind: Video` alone, because the one consumer was the live-preview video
//! player. Story 43.5 gives that widget an `<img>` and an `<audio>` too, so the
//! allow-list widens to `Video | Image | Audio` — and stops there. A
//! `kind: File` is rendered as a chip carrying Reveal and Copy path, which
//! open through the system, so nothing ever asks this scheme for its bytes and
//! serving them would be a hole opened for no caller. `Folder` is not a file.
//!
//! What widened is the set of KINDS. The root did not move, the resolution is
//! still a membership test against the session's own composed targets, and the
//! canonicalizing containment check below still runs on every request — a
//! `.png` symlinked out of the recordings root is refused exactly as a `.mov`
//! always was, which is asserted rather than asserted-about.
//!
//! Everything below resolution — `Range` parsing, the 8 MiB slice cap, 200 / 206
//! / 416 / 404, `Content-Type` from the extension allow-list, `nosniff` — is
//! `note_protocol`'s, called rather than copied. Range support is not optional
//! here: a `<video>` seeks by issuing `Range` headers, and a handler that
//! ignored them would either refuse to scrub or push a multi-hundred-megabyte
//! screen recording down the pipe on every seek.

use std::path::{Path, PathBuf};

use keeper_core::vm::RecordingNoteTargetKind;
use tauri::http::{header, Request, Response};
use tauri::{AppHandle, Manager, Runtime, UriSchemeResponder};

use crate::ipc::AppState;
use crate::note_protocol;

/// The URI scheme this handler serves. Spelled again as
/// `RECORDING_ASSET_SCHEME` in `src/components/notes/editor/recording-embed.ts`,
/// which composes the URLs — the same split `keeper-note://` has between
/// `note_protocol::SCHEME` and the note editor's `assetUrl`.
pub const SCHEME: &str = "keeper-recording";

/// Entry point invoked from the registered async URI-scheme protocol.
///
/// **Everything after the URL parse runs on the blocking pool**, which is where
/// this departs from `note_protocol`. There, resolution is a registry lookup and
/// one `canonicalize`; here it opens `archive.db` and lists a session folder that
/// may sit on a removable or network volume (Story 41.x's whole premise), so
/// doing it on the webview thread would stall the UI on exactly the media the
/// user is trying to watch.
///
/// The two values only the app state can give — the data directory and the
/// effective destination root — are read first, because `State` borrows the app
/// handle and cannot cross into the task.
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

    let Some((session_id, rel)) = parse_recording_url(&uri) else {
        tracing::debug!("keeper-recording: unparsable URL");
        responder.respond(note_protocol::not_found());
        return;
    };

    let state = app.state::<AppState>();
    let Ok(data_dir) = state.platform.data_dir() else {
        tracing::debug!("keeper-recording: no data directory, so no archive to resolve against");
        responder.respond(note_protocol::not_found());
        return;
    };
    let destination_root = crate::ipc::effective_destination_dir(&data_dir, &state.platform);

    tauri::async_runtime::spawn(async move {
        let response = tokio::task::spawn_blocking(move || {
            serve(&data_dir, &destination_root, &session_id, &rel, range)
        })
        .await
        .unwrap_or_else(|error| {
            tracing::debug!(%error, "keeper-recording: read task failed");
            note_protocol::not_found()
        });
        responder.respond(response);
    });
}

/// Resolve one request and answer it. Blocking: sqlite, a directory read, and
/// the file read itself.
fn serve(
    data_dir: &Path,
    destination_root: &Path,
    session_id: &str,
    rel: &str,
    range: Option<String>,
) -> Response<Vec<u8>> {
    let Some(path) = resolve_servable(data_dir, destination_root, session_id, rel) else {
        // Logged without the path, for `note_protocol`'s reason: a refused
        // request is often a traversal attempt, and echoing it into the log is
        // how a log becomes a reflection surface. The session id is keeper's own
        // identifier and says nothing about the filesystem.
        tracing::debug!(
            session = %session_id,
            "keeper-recording: no servable target of this session answers to that path"
        );
        return note_protocol::not_found();
    };
    let Some(mimetype) = note_protocol::mime_for(rel) else {
        // Unreachable while the kind tables and this allow-list agree — which
        // `every_servable_kind_has_a_content_type` pins — and kept because the
        // day they stop agreeing this must be a 404 and not a file served with
        // a guessed type.
        tracing::debug!("keeper-recording: extension is not on the allow-list");
        return note_protocol::not_found();
    };
    note_protocol::read_response(&path, mimetype, range)
}

/// The absolute path of the session's servable target named by `rel`, or
/// `None`.
///
/// The list comes from the index, so the folder is wherever it is now; the
/// membership test is what makes this a *lookup* rather than a join (AD-65).
///
/// The kind test is the allow-list this scheme widened in Story 43.5, and it is
/// stated as the three kinds served rather than as "not `File`, not `Folder`":
/// a kind added to the vocabulary later must be opted IN to being served, not
/// find itself served because nobody remembered to exclude it.
fn resolve_servable(
    data_dir: &Path,
    destination_root: &Path,
    session_id: &str,
    rel: &str,
) -> Option<PathBuf> {
    let targets = crate::ipc::recording_note_targets_in(data_dir, destination_root, session_id)
        .ok()
        .flatten()?;
    let target = targets
        .iter()
        .find(|target| is_servable(target.kind) && target.relative_path == rel)?;
    contained_read(destination_root, Path::new(&target.absolute_path))
}

/// Whether a note renders this kind inline, and therefore whether this scheme
/// hands out its bytes (Story 43.5, AD-73).
fn is_servable(kind: RecordingNoteTargetKind) -> bool {
    matches!(
        kind,
        RecordingNoteTargetKind::Video
            | RecordingNoteTargetKind::Image
            | RecordingNoteTargetKind::Audio
    )
}

/// The canonicalizing containment check, AD-59's rule applied to a root that has
/// no registration step to canonicalize it at.
///
/// A vault root is canonicalized once when the vault is registered, so
/// `note_protocol` pays for one `canonicalize` per request. The recordings
/// destination is resolved per request from settings and may be a symlinked or
/// newly mounted volume, so both sides are canonicalized here. A root that
/// cannot be canonicalized is not mounted, and nothing under it is servable.
fn contained_read(destination_root: &Path, absolute: &Path) -> Option<PathBuf> {
    let root = destination_root.canonicalize().ok()?;
    let canonical = absolute.canonicalize().ok()?;
    if !canonical.starts_with(&root) {
        return None;
    }
    // A fifo or a device would block or lie about its length; only a regular
    // file is servable. `canonicalize` has already resolved every link, so this
    // describes the file that would actually be read.
    let meta = std::fs::symlink_metadata(&canonical).ok()?;
    meta.is_file().then_some(canonical)
}

/// Parse `keeper-recording://<session_id>/<url-encoded destination-relative path>`.
///
/// `parse_note_url`'s grammar with a session id where the vault id goes, and the
/// same two spellings, because Tauri's rewriting differs by platform. Returns
/// both halves percent-decoded; a query or fragment is discarded, since this
/// scheme has no parameters and a URL carrying some is being probed.
///
/// The component refusals are belt and braces — `resolve_servable` matches the path
/// against a composed list and never joins it onto anything, so a `..` could not
/// escape even if it arrived. They stay because a refusal with a legible reason
/// beats a silent no-match, and because a NUL must not reach a `Path` on a
/// platform that would truncate at it.
fn parse_recording_url(raw: &str) -> Option<(String, String)> {
    let rest = raw
        .strip_prefix(&format!("{SCHEME}://"))
        .or_else(|| raw.strip_prefix(&format!("http://{SCHEME}.localhost/")))
        .or_else(|| raw.strip_prefix(&format!("https://{SCHEME}.localhost/")))?;
    let rest = rest.split(['?', '#']).next()?;

    let mut segments = rest.split('/');
    let session_id = note_protocol::decode(segments.next()?)?;
    if session_id.is_empty() || session_id.contains('\0') {
        return None;
    }
    let mut parts = Vec::new();
    for segment in segments {
        // An empty segment is `//` or a trailing slash: neither names a file, and
        // silently collapsing them would make two URLs mean one path.
        if segment.is_empty() {
            return None;
        }
        parts.push(note_protocol::decode(segment)?);
    }
    if parts.is_empty() {
        return None;
    }
    let rel = parts.join("/");
    if rel.contains('\0') || rel.split('/').any(|part| part == ".." || part == ".") {
        return None;
    }
    Some((session_id, rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_parses_into_a_session_id_and_a_destination_relative_path() {
        assert_eq!(
            parse_recording_url("keeper-recording://01DEVICE-01SESSION/2026/a%20b/screen-0000.mov"),
            Some((
                "01DEVICE-01SESSION".to_owned(),
                "2026/a b/screen-0000.mov".to_owned()
            ))
        );
        // The Windows/Android spelling Tauri rewrites to.
        assert_eq!(
            parse_recording_url("http://keeper-recording.localhost/01S/clip.mov"),
            Some(("01S".to_owned(), "clip.mov".to_owned()))
        );
        // A cache-busting query or a fragment is discarded, not treated as path.
        assert_eq!(
            parse_recording_url("keeper-recording://01S/clip.mov?v=2"),
            Some(("01S".to_owned(), "clip.mov".to_owned()))
        );
        assert_eq!(
            parse_recording_url("keeper-recording://01S/clip.mov#t=30"),
            Some(("01S".to_owned(), "clip.mov".to_owned()))
        );
    }

    #[test]
    fn a_url_that_could_escape_is_refused_before_any_filesystem_call() {
        // Traversal, plain and percent-encoded — `%2E%2E` decodes to `..`, which
        // is why the component check runs after decoding and not before.
        assert!(parse_recording_url("keeper-recording://01S/../secrets.mov").is_none());
        assert!(parse_recording_url("keeper-recording://01S/%2E%2E/secrets.mov").is_none());
        assert!(parse_recording_url("keeper-recording://01S/a/./b.mov").is_none());
        // A NUL, in either half.
        assert!(parse_recording_url("keeper-recording://01S/clip%00.mov").is_none());
        assert!(parse_recording_url("keeper-recording://01%00S/clip.mov").is_none());
        // No session, no path, empty segments.
        assert!(parse_recording_url("keeper-recording:///clip.mov").is_none());
        assert!(parse_recording_url("keeper-recording://01S").is_none());
        assert!(parse_recording_url("keeper-recording://01S/").is_none());
        assert!(parse_recording_url("keeper-recording://01S//clip.mov").is_none());
        // A foreign scheme — including the two this handler sits beside, which it
        // must never answer for.
        assert!(parse_recording_url("keeper-note://01VAULT/shot.png").is_none());
        assert!(parse_recording_url("keeper-media://media/a/b/c/full").is_none());
        assert!(parse_recording_url("file:///etc/passwd").is_none());
    }

    #[test]
    fn a_path_outside_the_destination_root_is_refused_however_it_got_there() {
        let base = std::env::temp_dir().join(format!(
            "keeper-recording-contained-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default()
        ));
        let root = base.join("Movies");
        let outside = base.join("elsewhere");
        std::fs::create_dir_all(&root).expect("a temp destination root");
        std::fs::create_dir_all(&outside).expect("a temp sibling");
        let inside_file = root.join("clip.mov");
        std::fs::write(&inside_file, b"bytes").expect("a file inside the root");
        let outside_file = outside.join("secrets.mov");
        std::fs::write(&outside_file, b"bytes").expect("a file outside the root");
        // A link that lives inside the root and points out of it: the one shape
        // every string test passes and only canonicalisation catches.
        #[cfg(unix)]
        let escape = {
            let link = root.join("escape.mov");
            std::os::unix::fs::symlink(&outside_file, &link).expect("a symlink out of the root");
            link
        };

        assert_eq!(
            contained_read(&root, &inside_file),
            Some(inside_file.canonicalize().expect("a real path")),
            "a regular file under the root is servable"
        );
        assert_eq!(
            contained_read(&root, &outside_file),
            None,
            "a sibling directory is not the destination root"
        );
        assert_eq!(
            contained_read(&root, &root),
            None,
            "a directory is not a servable file"
        );
        assert_eq!(
            contained_read(&root, &root.join("absent.mov")),
            None,
            "a path that is not there cannot be canonicalized, so it is refused"
        );
        assert_eq!(
            contained_read(&base.join("not-mounted"), &inside_file),
            None,
            "a root that cannot be canonicalized is not mounted, and serves nothing"
        );
        #[cfg(unix)]
        assert_eq!(
            contained_read(&root, &escape),
            None,
            "a symlink inside the root pointing out of it fails the canonical check"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Story 43.5: the kinds this scheme serves grew, and the set of extensions
    /// `note_protocol` will name a `Content-Type` for did not. Those two lists
    /// live in different crates and nothing but this test makes them agree —
    /// a disagreement is a target the widget renders an element for and the
    /// scheme then 404s, which is precisely the dead player 42.6 refuses.
    #[test]
    fn every_servable_kind_has_a_content_type() {
        use keeper_core::archive::recordings_fts::{
            kind_for_file_name, AUDIO_EXTENSIONS, IMAGE_EXTENSIONS, VIDEO_EXTENSIONS,
        };

        for extension in VIDEO_EXTENSIONS
            .iter()
            .chain(IMAGE_EXTENSIONS.iter())
            .chain(AUDIO_EXTENSIONS.iter())
        {
            let name = format!("a.{extension}");
            assert!(
                is_servable(kind_for_file_name(&name)),
                "{name} is on a kind table but this scheme would not serve it"
            );
            assert!(
                note_protocol::mime_for(&name).is_some(),
                "{name} would be served with no Content-Type"
            );
        }

        // The other direction is deliberately NOT symmetric: `.pdf` has a
        // Content-Type and is still refused, because a PDF renders as a chip
        // and no element ever requests its bytes.
        assert!(note_protocol::mime_for("paper.pdf").is_some());
        assert!(!is_servable(kind_for_file_name("paper.pdf")));
        assert!(!is_servable(kind_for_file_name("manifest.json")));
        assert!(!is_servable(RecordingNoteTargetKind::Folder));
    }

    /// Index one session at `2026/Session` into a real `archive.db` under
    /// `data_dir`, which is all [`resolve_servable`] needs to place a folder.
    fn index_session(data_dir: &Path, session_id: &str) {
        let conn = keeper_core::archive::db::open_archive_db(data_dir).expect("open the archive");
        let row = keeper_core::archive::RecordingRow {
            session_id: session_id.to_owned(),
            device_id: None,
            relative_path: "2026/Session".to_owned(),
            root_kind: "folder".to_owned(),
            profile_id: None,
            started_ts: Some(1_000),
            ended_ts: Some(61_000),
            title: Some("Session".to_owned()),
            participants_json: None,
            note: None,
            tags_json: None,
            custom_json: None,
            codec: None,
            width: None,
            height: None,
            fps: None,
            durability: "local".to_owned(),
            manifest_version: 1,
        };
        keeper_core::archive::recordings::upsert_recording(&conn, &row).expect("index the session");
    }

    /// Story 43.5, the one the widening could have broken: serving images and
    /// audio widened the ALLOW-LIST and not the root.
    ///
    /// Asserted through [`resolve_servable`] rather than over
    /// [`contained_read`] alone, because the containment check being correct in
    /// isolation says nothing about whether the widened kind path still reaches
    /// it. A `.png` symlinked out of the recordings root is a target the index
    /// composes, the kind test now admits, and canonicalisation must still
    /// refuse — and before Story 43.5 that path could not even be expressed,
    /// because a `.png` was never servable in the first place.
    #[test]
    fn a_widened_kind_did_not_widen_the_root() {
        let base = std::env::temp_dir().join(format!(
            "keeper-recording-widened-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default()
        ));
        let data_dir = base.join("data");
        let root = base.join("Movies");
        let outside = base.join("elsewhere");
        let folder = root.join("2026").join("Session");
        std::fs::create_dir_all(&folder).expect("the session folder");
        std::fs::create_dir_all(&outside).expect("a temp sibling");
        std::fs::write(outside.join("secrets.png"), b"bytes").expect("a file outside the root");
        for name in ["whiteboard.png", "room-tone.wav", "manifest.json"] {
            std::fs::write(folder.join(name), b"bytes").expect("a session file");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.join("secrets.png"), folder.join("escape.png"))
            .expect("a symlink out of the root");
        index_session(&data_dir, "01DEVICE-01SESSION");

        let resolve = |rel: &str| resolve_servable(&data_dir, &root, "01DEVICE-01SESSION", rel);

        assert_eq!(
            resolve("2026/Session/whiteboard.png"),
            Some(
                folder
                    .join("whiteboard.png")
                    .canonicalize()
                    .expect("a real path")
            ),
            "an image inside the root is what the widening was for"
        );
        assert!(
            resolve("2026/Session/room-tone.wav").is_some(),
            "audio inside the root is servable too"
        );
        assert_eq!(
            resolve("2026/Session/manifest.json"),
            None,
            "a chip's file is never streamed: nothing renders it, so nothing may fetch it"
        );
        assert_eq!(
            resolve("2026/Session"),
            None,
            "the session folder is not a servable file"
        );
        #[cfg(unix)]
        assert_eq!(
            resolve("2026/Session/escape.png"),
            None,
            "an image symlinked out of the recordings root is refused after canonicalisation, \
             exactly as a video always was"
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}
