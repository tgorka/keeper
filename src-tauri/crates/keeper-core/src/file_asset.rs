//! The `keeper-file://` scheme's grammar (Story 45.7, FR-180, AD-59, AD-65,
//! AD-74).
//!
//! # Why a fourth scheme, stated before the code
//!
//! Story 45.7 asks for images, audio and video to open in a panel over
//! `keeper-recording://` "with its range support". The coordinates do not
//! match, and the mismatch is the whole of this module's reason to exist.
//!
//! - `keeper-recording://<session_id>/<rel>` is rooted at the **effective
//!   recordings destination** and resolves by listing what a SESSION has and
//!   picking a member by name. A Files-pane file has no session. AD-74 says in
//!   as many words that the Files tab must not reach for this scheme, and
//!   `sync_open_entry` already refused the same shortcut for the same reason:
//!   pointed at a note in a vault it would decline, correctly, and a browser
//!   whose Open works for one folder in five is worse than one with no Open.
//! - `keeper-note://<vault_id>/<rel>` is rooted at `vault.root`, which is
//!   `local_path/subfolder` — the notes subfolder, not the profile root the
//!   Files pane browses. Widening it to the profile root would delete the
//!   containment that makes it safe (AD-59), and half the pane's files sit
//!   outside it anyway.
//!
//! So: a fourth scheme with a fourth fixed root — the sync profile's own
//! `local_path` — resolved by [`keeper_sync::browse::resolve`], the same
//! function `sync_browse`, `sync_open_entry` and `sync_read_text` already use.
//! Not a second containment rule; the first one, called again. Everything below
//! resolution — `Range`, the slice cap, 200/206/416/404, `Content-Type` from an
//! allow-list, `nosniff` — is `note_protocol`'s and is called rather than
//! copied.
//!
//! # Why the grammar lives here and not beside the handler
//!
//! `parse_note_url` and `parse_recording_url` live in the `keeper` shell crate,
//! which does not build on Linux (AD-55, AD-56) — so their tests only run on
//! macOS. A URL parser is the most testable thing in a protocol handler and the
//! part where a mistake is a file-disclosure primitive. It belongs in the crate
//! that compiles everywhere. The shell keeps only what needs `tauri`: reading
//! the request, resolving the profile, and answering.
//!
//! # What this module does NOT do
//!
//! It composes no path and touches no disk. It turns a URL into two strings and
//! says whether a kind is served. Containment is the caller's `browse::resolve`
//! and it is not restated here, because a second copy of a containment rule is
//! a rule that will disagree with the first.

use crate::vm::RecordingNoteTargetKind;

/// The URI scheme this grammar describes.
///
/// Spelled again as `FILE_ASSET_SCHEME` in `src/lib/viewers/file-asset-url.ts`,
/// which composes the URLs — the same split `keeper-note://` and
/// `keeper-recording://` both have between the handler and the frontend that
/// builds the `src`. The two spellings are pinned to each other by
/// `file-asset-url-vectors.json`, which both test suites load, so a change to
/// either side fails on the commit that introduces it.
pub const SCHEME: &str = "keeper-file";

/// Whether the webview may be handed this kind's bytes over this scheme
/// (Story 45.7, AD-73).
///
/// Stated as the three kinds served rather than as "not `File`, not `Folder`",
/// which is 43.5's wording and 43.5's reason: a kind added to the vocabulary
/// later must be opted IN to being served, not find itself served because
/// nobody remembered to exclude it.
///
/// A `File` is not served BY ITS KIND. See [`is_servable_path`] for the one
/// extension that is opted in on top of this, and why it is a different
/// question rather than a wider answer to this one.
pub fn is_servable_kind(kind: RecordingNoteTargetKind) -> bool {
    matches!(
        kind,
        RecordingNoteTargetKind::Video
            | RecordingNoteTargetKind::Image
            | RecordingNoteTargetKind::Audio
    )
}

/// The one extension served on top of [`is_servable_kind`] (Story 45.8).
///
/// PDF is not a `RecordingNoteTargetKind` and should not become one: the kind
/// vocabulary answers "which element does a note embed this as", and a note
/// embeds a PDF as nothing. What a PDF is, is the one document format whose
/// PIXELS the webview draws for itself, in an `<embed>`, from a URL. DOCX,
/// PPTX and XLSX are not here and must not be: Story 45.8 parses those in Rust
/// and ships a bounded view model, so their bytes never cross into the webview
/// at all.
///
/// Served over this scheme rather than base64'd across IPC because a 200 MB
/// PDF costs a `Range`-served reader nothing and costs a marshalled one 200 MB
/// of webview heap — the cap that would then be needed is a refusal to open a
/// file keeper can perfectly well open.
const SERVABLE_EXTENSIONS: [&str; 1] = ["pdf"];

/// Whether this scheme will serve the bytes of the file named by `rel`
/// (Story 45.7, Story 45.8).
///
/// **Two questions, deliberately, and both are answered on the NAME.** The kind
/// question is 43.5's classifier and covers everything a viewer mounts an
/// element for. The extension question is this module's own allow-list and
/// covers the one format that is not a kind. Collapsing them into a single
/// extension table would put a fourth classifier in the repo, and AD-73 exists
/// because there were already three.
///
/// Touches no path and no disk: it reads the last dot of a name. That is why
/// the handler may ask it before it has established that the profile id is real
/// without disturbing the rule that an unknown id is refused before any path
/// work happens.
pub fn is_servable_path(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    if is_servable_kind(crate::archive::recordings_fts::kind_for_file_name(name)) {
        return true;
    }
    let Some(extension) = std::path::Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
    else {
        return false;
    };
    SERVABLE_EXTENSIONS
        .iter()
        .any(|known| extension.eq_ignore_ascii_case(known))
}

/// Parse `keeper-file://<profile_id>/<url-encoded profile-relative path>`.
///
/// `parse_note_url`'s grammar with a profile id where the vault id goes, and
/// the same two spellings, because Tauri's rewriting differs by platform: the
/// `<scheme>://<host>/<path>` form macOS, Linux and iOS see, and the
/// `http://<scheme>.localhost/<host>/<path>` form Windows and Android use.
///
/// Returns both halves percent-decoded. A query or fragment is discarded: this
/// scheme has no parameters, so a URL carrying some is being probed rather than
/// used — and a `?v=2` cache-buster appended by a reload must name the same
/// file rather than a missing one.
///
/// The component refusals are belt and braces. `browse::resolve` refuses `..`
/// lexically and refuses a symlink out of the tree after canonicalisation, so a
/// dot segment could not escape anything. Refusing here keeps the reason
/// legible in the log and keeps a NUL out of a `Path` on a platform that would
/// truncate at it.
pub fn parse_file_url(raw: &str) -> Option<(String, String)> {
    let rest = raw
        .strip_prefix(&format!("{SCHEME}://"))
        .or_else(|| raw.strip_prefix(&format!("http://{SCHEME}.localhost/")))
        .or_else(|| raw.strip_prefix(&format!("https://{SCHEME}.localhost/")))?;
    // A query or fragment ends the path. `split` always yields at least one
    // element, so the `?` is unreachable in practice and keeps this total.
    let rest = rest.split(['?', '#']).next()?;

    let mut segments = rest.split('/');
    let profile_id = decode(segments.next()?)?;
    if profile_id.is_empty() || profile_id.contains('\0') {
        return None;
    }
    let mut parts = Vec::new();
    for segment in segments {
        // An empty segment is `//` or a trailing slash: neither names a file,
        // and silently collapsing them would make two URLs mean one path.
        if segment.is_empty() {
            return None;
        }
        parts.push(decode(segment)?);
    }
    if parts.is_empty() {
        return None;
    }
    let rel = parts.join("/");
    if rel.contains('\0') || rel.split('/').any(|part| part == ".." || part == ".") {
        return None;
    }
    Some((profile_id, rel))
}

/// Percent-decode one segment to UTF-8, or `None` when the bytes are not UTF-8.
///
/// A separate copy of `note_protocol::decode` only in the sense that the shell
/// cannot be depended on from here — `keeper-core` carries no `tauri` (AD-6).
/// It is three lines of `percent_encoding`, and the alternative was leaving the
/// whole parser in a crate that does not build on this machine.
fn decode(segment: &str) -> Option<String> {
    percent_encoding::percent_decode_str(segment)
        .decode_utf8()
        .ok()
        .map(std::borrow::Cow::into_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pinning table. `src/lib/viewers/file-asset-url.test.ts` loads THIS
    /// file and asserts its composer produces each `url`; the test below
    /// asserts this parser takes each `url` back apart into the same two
    /// halves. A composer and a parser in two languages that never meet is how
    /// a space in a folder name becomes a 404 nobody can reproduce.
    const VECTORS_JSON: &str = include_str!("file-asset-url-vectors.json");

    #[derive(serde::Deserialize)]
    struct Vector {
        profile_id: String,
        relative_path: String,
        url: String,
    }

    #[derive(serde::Deserialize)]
    struct Vectors {
        ok: Vec<Vector>,
        refused: Vec<Vector>,
    }

    fn vectors() -> Vectors {
        serde_json::from_str(VECTORS_JSON).expect("the shared vector table parses")
    }

    #[test]
    fn every_shared_vector_takes_apart_into_the_two_halves_it_was_composed_from() {
        let table = vectors();
        assert!(
            table.ok.len() >= 8,
            "the shared table is the only thing pinning the two languages together; \
             emptying it would leave both suites passing while agreeing on nothing"
        );
        for vector in &table.ok {
            assert_eq!(
                parse_file_url(&vector.url),
                Some((vector.profile_id.clone(), vector.relative_path.clone())),
                "{} did not take apart into its two halves",
                vector.url
            );
        }
    }

    #[test]
    fn every_composed_dot_segment_is_refused_by_the_parser() {
        // The composer deliberately still produces a URL for `..`, so the
        // attempt reaches the log as visible text rather than as a path that
        // already collapsed. This is the other end of that decision: nothing it
        // composes for a dot segment is ever accepted.
        let table = vectors();
        assert!(!table.refused.is_empty());
        for vector in &table.refused {
            assert_eq!(
                parse_file_url(&vector.url),
                None,
                "{} was accepted",
                vector.url
            );
        }
    }

    #[test]
    fn accepts_the_windows_and_android_spelling_tauri_rewrites_to() {
        assert_eq!(
            parse_file_url("http://keeper-file.localhost/01PROFILE/a%20b/clip.mov"),
            Some(("01PROFILE".to_owned(), "a b/clip.mov".to_owned()))
        );
        assert_eq!(
            parse_file_url("https://keeper-file.localhost/01PROFILE/clip.mov"),
            Some(("01PROFILE".to_owned(), "clip.mov".to_owned()))
        );
    }

    #[test]
    fn a_cache_buster_or_a_fragment_is_discarded_rather_than_read_as_path() {
        assert_eq!(
            parse_file_url("keeper-file://01P/clip.mov?retry=1"),
            Some(("01P".to_owned(), "clip.mov".to_owned()))
        );
        assert_eq!(
            parse_file_url("keeper-file://01P/clip.mov#t=30"),
            Some(("01P".to_owned(), "clip.mov".to_owned()))
        );
    }

    #[test]
    fn refuses_a_dot_segment_in_either_spelling() {
        // Decoded before the component check, which is why `%2E%2E` is refused
        // and why the check runs after `decode` rather than before it.
        assert!(parse_file_url("keeper-file://01P/../secrets.mov").is_none());
        assert!(parse_file_url("keeper-file://01P/%2E%2E/secrets.mov").is_none());
        assert!(parse_file_url("keeper-file://01P/a/./b.mov").is_none());
    }

    #[test]
    fn refuses_a_nul_in_either_half() {
        assert!(parse_file_url("keeper-file://01P/clip%00.mov").is_none());
        assert!(parse_file_url("keeper-file://01%00P/clip.mov").is_none());
    }

    #[test]
    fn refuses_an_empty_id_an_empty_path_and_an_empty_segment() {
        assert!(parse_file_url("keeper-file:///clip.mov").is_none());
        assert!(parse_file_url("keeper-file://01P").is_none());
        assert!(parse_file_url("keeper-file://01P/").is_none());
        assert!(parse_file_url("keeper-file://01P//clip.mov").is_none());
    }

    #[test]
    fn refuses_another_scheme_entirely() {
        assert!(parse_file_url("keeper-recording://01P/clip.mov").is_none());
        assert!(parse_file_url("keeper-note://01P/clip.mov").is_none());
        assert!(parse_file_url("file:///etc/passwd").is_none());
    }

    #[test]
    fn refuses_bytes_that_are_not_utf8() {
        assert!(parse_file_url("keeper-file://01P/%FF%FE.mov").is_none());
    }

    #[test]
    fn the_kind_gate_is_exactly_the_three_elements_a_viewer_mounts() {
        assert!(is_servable_kind(RecordingNoteTargetKind::Video));
        assert!(is_servable_kind(RecordingNoteTargetKind::Image));
        assert!(is_servable_kind(RecordingNoteTargetKind::Audio));
        // A PDF classifies as `File`, so the kind gate says no to it and the
        // extension gate is what lets it through. Two questions, kept apart.
        assert!(!is_servable_kind(RecordingNoteTargetKind::File));
        assert!(!is_servable_kind(RecordingNoteTargetKind::Folder));
    }

    #[test]
    fn serves_every_media_extension_the_one_classifier_recognises() {
        // Driven through the classifier's own tables rather than a list retyped
        // here: a table retyped in a second place is a table that drifts, which
        // is the whole of AD-73.
        use crate::archive::recordings_fts::{
            AUDIO_EXTENSIONS, IMAGE_EXTENSIONS, VIDEO_EXTENSIONS,
        };
        for extension in VIDEO_EXTENSIONS
            .iter()
            .chain(IMAGE_EXTENSIONS.iter())
            .chain(AUDIO_EXTENSIONS.iter())
        {
            assert!(
                is_servable_path(&format!("a/b/clip.{extension}")),
                "{extension}"
            );
            // Case-insensitively, because a file copied from another machine may
            // be `.MOV` and a player absent for the same video under a different
            // spelling reads as a bug.
            assert!(
                is_servable_path(&format!("clip.{}", extension.to_uppercase())),
                "{extension}"
            );
        }
    }

    #[test]
    fn serves_a_pdf_and_no_other_document() {
        assert!(is_servable_path("reports/2026 Q3.pdf"));
        assert!(is_servable_path("A.PDF"));
        // Story 45.8 parses these in Rust and ships a bounded view model; their
        // bytes never reach the webview, so serving them would open a hole for
        // no caller.
        for name in ["a.docx", "b.pptx", "c.xlsx", "d.odt", "e.rtf"] {
            assert!(!is_servable_path(name), "{name}");
        }
    }

    #[test]
    fn refuses_everything_that_is_not_opted_in() {
        for rel in [
            "notes.md",
            "a/b/config.json",
            "Makefile",
            ".gitignore",
            "clip.mov.bak",
            "archive.zip",
            "script.sh",
            "",
            "a/",
            "folder",
        ] {
            assert!(!is_servable_path(rel), "{rel}");
        }
    }

    #[test]
    fn the_last_extension_decides_and_a_directory_in_the_path_cannot_lend_one() {
        // `clip.mov.bak` is a backup, not a video; `photos.png/notes.md` is a
        // markdown file in an oddly named folder, and serving it because an
        // ANCESTOR looked like an image would be a content-type confusion with
        // a `nosniff` header on top of it.
        assert!(!is_servable_path("clip.mov.bak"));
        assert!(!is_servable_path("photos.png/notes.md"));
        assert!(is_servable_path("notes.md/clip.mov"));
    }
}
