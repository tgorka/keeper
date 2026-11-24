//! Names, folds, media types and modes (decisions 5, 9, 10, 14).

use serde::{Deserialize, Serialize};

use crate::refusal::{Code, Refusal};
use crate::MAX_NAME_BYTES;

/// Admits `name` as one entry's name: exactly one `keeper-paths` plain
/// segment (so no `/`, `.`, `..`, empty name or U+FFFD), no NUL, at most
/// [`MAX_NAME_BYTES`] bytes. Otherwise `invalid_name`.
pub fn admit_name(name: &str) -> Result<(), Refusal> {
    let refused = || Refusal::new(Code::InvalidName);
    if name.len() > MAX_NAME_BYTES || name.contains('\0') {
        return Err(refused());
    }
    match keeper_paths::plain_segments(name) {
        Ok(segments) if segments.len() == 1 => Ok(()),
        _ => Err(refused()),
    }
}

/// The stored caseless fold of a name (`name_fold`): two live entries of
/// one folder never share it.
#[must_use]
pub fn fold(name: &str) -> String {
    keeper_paths::names::name_fold(name)
}

/// `syncular/files.media-types.json`, the one table a name's media type
/// comes from (decision 9, option b).
pub const MEDIA_TYPES_JSON: &str = include_str!("../../../../syncular/files.media-types.json");

/// The type a name without a known extension gets.
pub const DEFAULT_MEDIA_TYPE: &str = "application/octet-stream";

/// The media type of a file named `name`: its extension's — the text after
/// the last `.` when that `.` is not the first character, ASCII-lowercased —
/// in [`MEDIA_TYPES`], else [`DEFAULT_MEDIA_TYPE`]. The server computes it;
/// a client never declares it, and a rename recomputes it.
#[must_use]
pub fn media_type(name: &str) -> &'static str {
    let Some(dot) = name.rfind('.').filter(|&dot| dot > 0) else {
        return DEFAULT_MEDIA_TYPE;
    };
    let extension = name[dot + 1..].to_ascii_lowercase();
    MEDIA_TYPES
        .iter()
        .find(|(ext, _)| *ext == extension)
        .map_or(DEFAULT_MEDIA_TYPE, |(_, media)| media)
}

/// The extension table, equal to `files.media-types.json`'s `extensions`
/// (a test holds them equal).
pub const MEDIA_TYPES: &[(&str, &str)] = &[
    ("7z", "application/x-7z-compressed"),
    ("aac", "audio/aac"),
    ("avif", "image/avif"),
    ("bmp", "image/bmp"),
    ("css", "text/css"),
    ("csv", "text/csv"),
    ("doc", "application/msword"),
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    ("epub", "application/epub+zip"),
    ("flac", "audio/flac"),
    ("gif", "image/gif"),
    ("gz", "application/gzip"),
    ("heic", "image/heic"),
    ("heif", "image/heif"),
    ("htm", "text/html"),
    ("html", "text/html"),
    ("ics", "text/calendar"),
    ("jpeg", "image/jpeg"),
    ("jpg", "image/jpeg"),
    ("js", "text/javascript"),
    ("json", "application/json"),
    ("m4a", "audio/mp4"),
    ("md", "text/markdown"),
    ("mkv", "video/x-matroska"),
    ("mov", "video/quicktime"),
    ("mp3", "audio/mpeg"),
    ("mp4", "video/mp4"),
    ("odt", "application/vnd.oasis.opendocument.text"),
    ("ogg", "audio/ogg"),
    ("opus", "audio/opus"),
    ("pdf", "application/pdf"),
    ("png", "image/png"),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    ("py", "text/x-python"),
    ("rtf", "application/rtf"),
    ("sh", "application/x-sh"),
    ("svg", "image/svg+xml"),
    ("tar", "application/x-tar"),
    ("tif", "image/tiff"),
    ("tiff", "image/tiff"),
    ("toml", "application/toml"),
    ("ts", "text/x-typescript"),
    ("txt", "text/plain"),
    ("wav", "audio/wav"),
    ("webm", "video/webm"),
    ("webp", "image/webp"),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    ("xml", "application/xml"),
    ("yaml", "application/yaml"),
    ("yml", "application/yaml"),
    ("zip", "application/zip"),
];

/// What `mode` holds (decision 10): git's executable bit, nothing more. It
/// never makes a webview run a file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileMode {
    #[default]
    Normal,
    Executable,
}

impl FileMode {
    /// The mode of a git tree entry's file mode: `100644` normal, `100755`
    /// executable; any other (a symlink, a submodule) is `invalid_change`.
    pub fn from_git(git_mode: &str) -> Result<Self, Refusal> {
        match git_mode {
            "100644" => Ok(Self::Normal),
            "100755" => Ok(Self::Executable),
            _ => Err(Refusal::new(Code::InvalidChange)),
        }
    }
}
