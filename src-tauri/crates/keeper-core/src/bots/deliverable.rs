//! What crosses the conversation as something other than prose (Epic 61,
//! Story 61.12, FR-392, FR-393, AD-160).
//!
//! Two halves, one module, because they are the same question asked in the two
//! directions and the answer to both is a *capability*, never a guess.
//!
//! # Outbound — an image you pasted
//!
//! The bytes reach Rust the way pasted image bytes already reach Rust: a raw
//! binary IPC body, never base64 inside a JSON payload (AD-58, and
//! `notes_vault.rs:1851-1853` in the house's own words). [`stage_image`] is
//! where they land — one file per paste under the app's data dir — and
//! [`image_content_part`] is the *only* place a `data:` URI is ever minted,
//! at the moment the HTTP request to the model is built. That distinction is
//! the whole of AD-58: base64 on the wire to a model is the wire protocol
//! Ollama documents ("Image URL ✘, Base64 encoded image ✔", research §7);
//! base64 across keeper's own IPC channel is a payload multiplier with no
//! reader.
//!
//! The gate in front of all of it is [`accept_image`], and it is tri-state on
//! purpose: a model whose vision capability keeper could not read is
//! `unknown`, and `unknown` offers the paste **with a warning** rather than
//! hiding it — a capability keeper could not read is never `false` (AD-27, and
//! the epic's 61.3 rule stated for the third time).
//!
//! # Inbound — a path the model named
//!
//! Hermes' own gateway implements "deliverable mode" by *rewriting the reply*:
//! it strips `MEDIA:` directives and absolute paths out of the visible text and
//! uploads the file (research §2.11). keeper is on the receiving end of the
//! api-server, where no stripping happens, and **keeper does not strip either**
//! — here the reply is the record. [`scan_paths`] therefore returns byte
//! offsets into the reply and never a rewritten string, so the renderer can
//! overlay a control on a span it did not alter.
//!
//! A control is offered only where the path falls inside a live grant
//! ([`resolve_deliverables`] calls [`crate::bots::grant::decide`] for every
//! mention). Outside one, the path renders as the text it is, with one sentence
//! saying why there is no button — which is AD-27's shape: an affordance that
//! could not work is absent, and the reason is on screen.
//!
//! keeper never *fetches* a path a model named. There is no read here, no copy
//! and no stat outside the drive: the only filesystem question this module
//! asks is "is there still a file there", and it asks it only after the grant
//! said yes.

use std::path::{Component, Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;

use crate::bots::chat::ContentPart;
use crate::bots::grant::{decide, Effect, Grant, GrantVerdict, ToolTarget};
use crate::error::CoreError;

// ---------------------------------------------------------------------------
// Outbound: the caps
// ---------------------------------------------------------------------------

/// The largest single image keeper will attach, in bytes.
///
/// 8 MiB, and the number is borrowed rather than invented: it is
/// `note_protocol::MAX_RANGE_CHUNK`, the ceiling the tree already puts on one
/// read of one asset into the webview. Two properties make it the right
/// ceiling here too. A pasted screenshot from any current display is an order
/// of magnitude under it, so the cap refuses nothing a person actually does;
/// and its base64 expansion is ~10.7 MB, which is then the largest JSON body
/// keeper will ever hand `reqwest` — a bound worth having on a request whose
/// far side may be a local process with a 4 GB model resident.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

/// The most images one message may carry.
///
/// Four, and the reason is the context window rather than the byte count.
/// Ollama's `/v1` layer **cannot set `num_ctx`** — the quirk table records it
/// as [`Support::No`](crate::bots::quirks::Support::No) and the native dialect
/// is deferred (DW-210) — so keeper cannot widen the window it is about to
/// fill. Each image costs on the order of a thousand tokens once tiled, and a
/// local model served at its 4k default still has room for the question at
/// four. A fifth image is not refused because it is expensive; it is refused
/// because keeper cannot tell the model to make room for it.
pub const MAX_IMAGES_PER_MESSAGE: usize = 4;

/// The image formats keeper will attach.
///
/// The intersection of what the Chat Completions vision guide documents (PNG,
/// JPEG, WEBP, non-animated GIF — research §3.2) and what the back ends take.
/// SVG is deliberately absent: Ollama has errored on SVG input since v0.4.6
/// (research §7), and an SVG is a document that can carry script, which is the
/// same reason `note_protocol.rs:216-218` serves it only behind `nosniff`.
pub const IMAGE_MIMES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];

// ---------------------------------------------------------------------------
// Outbound: the copy
// ---------------------------------------------------------------------------

/// Why an image was not attached to a model that cannot see (FR-392).
///
/// Names the model, because the person chose a bot and the refusal has to be
/// about *that* choice rather than about images in general — the next act is
/// switching model, and the sentence has to make that obvious.
pub fn refuse_vision(model: &str) -> String {
    format!(
        "{model} does not accept images, so the paste was not attached. \
         Choose a model that can see, and paste again."
    )
}

/// What an `unknown` vision capability says while still offering the paste.
///
/// `unknown` is not `false`: the endpoint did not tell keeper, so keeper offers
/// the affordance and says what it does not know. The alternative — hiding it —
/// would make keeper's silence look like the model's refusal.
pub fn warn_vision_unknown(model: &str) -> String {
    format!(
        "keeper could not read whether {model} accepts images. \
         The image is attached, and the model may answer that it cannot see it."
    )
}

/// Why an oversized paste was refused, with both numbers in it.
pub fn refuse_oversize(byte_len: usize) -> String {
    format!(
        "That image is {} and keeper attaches at most {} per image, so it was not attached. \
         Save it, shrink it, and paste it again.",
        human_bytes(byte_len),
        human_bytes(MAX_IMAGE_BYTES)
    )
}

/// Why one image too many was refused, with the count in it.
pub fn refuse_too_many() -> String {
    format!(
        "This message already carries {MAX_IMAGES_PER_MESSAGE} images, which is as many as \
         keeper attaches at once. Send these, then paste the next one."
    )
}

/// Why a clipboard image in a format keeper does not attach was refused.
pub fn refuse_mime(mime: &str) -> String {
    format!(
        "keeper does not attach {mime} images, so the paste was not attached. \
         PNG, JPEG, WEBP and GIF are attached."
    )
}

/// Render a byte count the way a refusal should read it.
///
/// Whole MB above a megabyte, whole kB below — a refusal that says
/// "8.00 MB" about a limit and "8.39 MB" about the file has told the reader
/// nothing they can act on.
fn human_bytes(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = 1024 * 1024;
    if bytes >= MB {
        let tenths = (bytes * 10).div_ceil(MB);
        format!("{}.{} MB", tenths / 10, tenths % 10)
    } else {
        format!("{} kB", bytes.div_ceil(KB))
    }
}

// ---------------------------------------------------------------------------
// Outbound: the gate
// ---------------------------------------------------------------------------

/// Whether the paste affordance exists for this model, and what it says.
///
/// The tri-state, as a type, so no caller can collapse it into a boolean on the
/// way to a component prop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageOffer {
    /// The endpoint said the model can see. Nothing to disclose.
    Offer,
    /// The endpoint did not say. Offered, with this sentence beside it.
    OfferWithWarning(String),
    /// The endpoint said the model cannot see. This sentence, no affordance.
    Refuse(String),
}

/// Decide whether a bot's composer offers an image paste at all (FR-392).
///
/// `vision` is the model's own answer, read from the endpoint by Story 61.3:
/// `Some(true)` where it said yes, `Some(false)` where it said no, `None` where
/// keeper could not read it.
pub fn image_offer(vision: Option<bool>, model: &str) -> ImageOffer {
    match vision {
        Some(true) => ImageOffer::Offer,
        None => ImageOffer::OfferWithWarning(warn_vision_unknown(model)),
        Some(false) => ImageOffer::Refuse(refuse_vision(model)),
    }
}

/// The whole gate for one pasted image: capability, format, size, count.
///
/// Returns the sentence to show on refusal, so there is exactly one place each
/// refusal is worded and the frontend's copy is this copy.
///
/// Ordered capability-first deliberately. A person pasting into a bot that
/// cannot see needs to hear *that*, not that their PNG is 9 MB — the size would
/// be a true sentence about the wrong problem.
pub fn accept_image(
    vision: Option<bool>,
    model: &str,
    mime: &str,
    byte_len: usize,
    already_attached: usize,
) -> Result<ImageOffer, String> {
    let offer = image_offer(vision, model);
    if let ImageOffer::Refuse(reason) = offer {
        return Err(reason);
    }
    if !IMAGE_MIMES.contains(&mime) {
        return Err(refuse_mime(mime));
    }
    if byte_len > MAX_IMAGE_BYTES {
        return Err(refuse_oversize(byte_len));
    }
    if already_attached >= MAX_IMAGES_PER_MESSAGE {
        return Err(refuse_too_many());
    }
    Ok(offer)
}

// ---------------------------------------------------------------------------
// Outbound: staging and the content part
// ---------------------------------------------------------------------------

/// Where a staged paste lives, relative to the app data dir.
const STAGE_DIR: &str = "bots/attachments";

/// One pasted image, on disk and not in a JSON payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedImage {
    /// The opaque id the webview holds and the send path resolves.
    pub id: String,
    /// The MIME the clipboard reported, already checked against
    /// [`IMAGE_MIMES`].
    pub mime: String,
    /// How many bytes landed.
    pub byte_len: usize,
}

/// The file extension keeper gives a staged image of this type.
fn extension_for(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        // Everything reaching here passed `IMAGE_MIMES`, so the remaining arm
        // is PNG. Kept as the fallback rather than an error because a staged
        // file's extension is a convenience for a human looking in the folder,
        // never the type keeper sends — that comes from `mime`.
        _ => "png",
    }
}

/// Write a pasted image into the staging folder (FR-392, AD-58).
///
/// Called from the raw-body IPC command, with the bytes the webview handed over
/// as an `ArrayBuffer`. Nothing about this function is reachable from a JSON
/// payload, which is the property the whole design turns on.
pub fn stage_image(data_dir: &Path, mime: &str, bytes: &[u8]) -> Result<StagedImage, CoreError> {
    if !IMAGE_MIMES.contains(&mime) {
        return Err(CoreError::Unsupported(refuse_mime(mime)));
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(CoreError::Unsupported(refuse_oversize(bytes.len())));
    }
    let dir = data_dir.join(STAGE_DIR);
    std::fs::create_dir_all(&dir)
        .map_err(|error| CoreError::Internal(format!("{STAGE_DIR}: {error}")))?;
    let id = ulid::Ulid::new().to_string();
    let path = dir.join(format!("{id}.{}", extension_for(mime)));
    std::fs::write(&path, bytes)
        .map_err(|error| CoreError::Internal(format!("{STAGE_DIR}/{id}: {error}")))?;
    Ok(StagedImage {
        id,
        mime: mime.to_owned(),
        byte_len: bytes.len(),
    })
}

/// Read a staged image back, by id and MIME (FR-392).
///
/// The id is matched against the folder's own listing rather than joined into a
/// path: an id that arrived over IPC is input, and `browse::resolve` is not
/// reachable from `keeper-core`, so the containment here is "the name must be
/// one this folder already holds" — which no traversal string can satisfy.
pub fn read_staged(data_dir: &Path, id: &str) -> Result<Vec<u8>, CoreError> {
    let dir = data_dir.join(STAGE_DIR);
    let entries = std::fs::read_dir(&dir)
        .map_err(|error| CoreError::Internal(format!("{STAGE_DIR}: {error}")))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let stem = name.split('.').next().unwrap_or_default();
        if stem == id {
            return std::fs::read(entry.path())
                .map_err(|error| CoreError::Internal(format!("{STAGE_DIR}/{id}: {error}")));
        }
    }
    Err(CoreError::Internal(format!(
        "no staged image {id}; it may already have been sent"
    )))
}

/// Delete a staged image, best-effort (FR-392).
///
/// Called after a send. A staging folder that only grows is a folder that
/// eventually holds every screenshot the person ever pasted, and nobody asked
/// keeper to keep those.
pub fn discard_staged(data_dir: &Path, id: &str) {
    let dir = data_dir.join(STAGE_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.split('.').next().unwrap_or_default() == id {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Mint the one `data:` URI this feature ever creates (FR-392).
///
/// **This is base64, and it is base64 in the right place.** It exists for the
/// duration of one outbound HTTP body, because that is the only shape Ollama's
/// OpenAI layer accepts — "Image URL ✘ / Base64 encoded image ✔" (research §7),
/// and the quirk table already records that the part is a bare string there and
/// an object on Hermes ([`crate::bots::quirks::ImagePartShape`]). It never
/// touches an IPC payload, a row or a log line.
pub fn image_data_uri(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", BASE64.encode(bytes))
}

/// The MIME of a staged image, read back from the extension keeper gave it.
///
/// The extension is keeper's own — [`extension_for`] wrote it from a MIME that
/// had already passed [`IMAGE_MIMES`] — so this is a round trip through a
/// vocabulary this module owns, not a sniff of a name a caller chose.
pub fn staged_mime(data_dir: &Path, id: &str) -> Option<String> {
    let dir = data_dir.join(STAGE_DIR);
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let mut parts = name.splitn(2, '.');
        if parts.next() != Some(id) {
            continue;
        }
        return match parts.next() {
            Some("jpg") => Some("image/jpeg".to_owned()),
            Some("webp") => Some("image/webp".to_owned()),
            Some("gif") => Some("image/gif".to_owned()),
            Some("png") => Some("image/png".to_owned()),
            _ => None,
        };
    }
    None
}

/// The content part for one attached image.
///
/// `detail` is left unset: the guide's own warning is that `low` "does not
/// always use fewer tokens than `high`" (research §3.2), so a default keeper
/// picked would be a knob that costs tokens for a reason nobody could state.
/// The serializer in [`crate::bots::chat`] shapes it per kind from the quirk
/// table, so this function makes no per-provider decision at all.
pub fn image_content_part(mime: &str, bytes: &[u8]) -> ContentPart {
    ContentPart::Image {
        url: image_data_uri(mime, bytes),
        detail: None,
    }
}

// ---------------------------------------------------------------------------
// Inbound: the copy
// ---------------------------------------------------------------------------

/// Why a path outside every synced folder gets no control.
pub const NO_CONTROL_OUTSIDE_DRIVE: &str =
    "That path is outside every folder keeper syncs, so keeper is not offering to open it. \
     keeper never opens a location a model named on its own.";

/// Why a path inside the drive but outside every grant gets no control.
pub const NO_CONTROL_NO_GRANT: &str =
    "No grant lets this bot reach that path, so keeper is not offering to open it. \
     Grants live in Settings → Bots.";

/// Why a granted path that is not there gets no control.
pub const NO_CONTROL_MISSING: &str =
    "There is no file at that path now, so there is nothing to open. \
     The model may have named a file it planned to write.";

// ---------------------------------------------------------------------------
// Inbound: the scanner
// ---------------------------------------------------------------------------

/// One path-shaped run of text in a reply, and where it sits in it.
///
/// `start`/`end` are byte offsets into the reply **as it was received**. keeper
/// does not rewrite a reply, so the renderer's job is to decorate this span and
/// never to replace it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMention {
    /// Exactly the characters that were matched, punctuation already trimmed.
    pub raw: String,
    /// Byte offset of `raw` in the reply.
    pub start: usize,
    /// Byte offset one past the end of `raw` in the reply.
    pub end: usize,
}

/// Characters that end a path run.
///
/// Whitespace ends it obviously. The rest are the delimiters a path is quoted,
/// bracketed or piped inside in prose and in Markdown, and none of them can
/// legally appear in a POSIX path keeper would open.
const PATH_STOPS: [char; 8] = ['`', '"', '\'', '<', '>', '|', '\\', '\0'];

/// Trailing characters a sentence puts after a path and a path never ends with.
///
/// A trailing `/` is trimmed too, so `/notes/` and `/notes` are one mention and
/// the grant grammar sees the spelling [`crate::bots::grant::parse_subpath`]
/// normalizes to.
const PATH_TRAILERS: [char; 10] = ['.', ',', ';', ':', '!', '?', ')', ']', '}', '/'];

/// Find every absolute or `~`-relative path in a reply (FR-393).
///
/// # The rules, in the order they apply
///
/// 1. **Code is not prose.** Anything inside a fenced block (``` or `~~~`, the
///    fence run repeated at the start of a line) or inside an inline code span
///    (a backtick run and its match on the same line) is masked out before any
///    matching happens. A model showing you a command is not a model handing
///    you a deliverable, and offering "open" on the `/etc/hosts` inside an
///    example would be keeper acting on something nobody claimed.
/// 2. A mention starts at `/` or at `~/`, and only where the character before
///    it is not part of a word, a path, or a URL scheme's `:`. That last one is
///    why `https://example.com/x` yields nothing.
/// 3. It runs to whitespace or to one of [`PATH_STOPS`].
/// 4. Trailing sentence punctuation and a trailing `/` are trimmed
///    ([`PATH_TRAILERS`]), repeatedly, so `see /a/b.md.` and `(/a/b.md)` both
///    yield `/a/b.md`.
/// 5. What survives must still have a segment after its root, so a bare `/` or
///    a bare `~/` is not a mention.
///
/// Windows spellings are not matched, and that is a statement rather than a
/// gap: keeper's drive paths are POSIX on both platforms it ships to, and a
/// pattern loose enough to catch `C:\Users\…` also catches every `12:30` in a
/// reply.
pub fn scan_paths(reply: &str) -> Vec<PathMention> {
    let code = code_mask(reply);
    let bytes = reply.as_bytes();
    let mut out = Vec::new();
    let mut idx = 0usize;
    while idx < bytes.len() {
        // `idx` walks bytes, so a multi-byte character would otherwise be
        // sliced through the middle. A path start is always ASCII, so a
        // non-boundary can only be interior to something that is not one.
        if !reply.is_char_boundary(idx) || code[idx] || !starts_here(reply, idx) {
            idx += 1;
            continue;
        }
        let mut end = idx;
        while end < bytes.len() && !code[end] {
            let ch = reply[end..].chars().next().unwrap_or(' ');
            if ch.is_whitespace() || PATH_STOPS.contains(&ch) {
                break;
            }
            end += ch.len_utf8();
        }
        let mut trimmed = &reply[idx..end];
        while let Some(last) = trimmed.chars().last() {
            if PATH_TRAILERS.contains(&last) {
                trimmed = &trimmed[..trimmed.len() - last.len_utf8()];
            } else {
                break;
            }
        }
        if has_a_segment(trimmed) {
            out.push(PathMention {
                raw: trimmed.to_owned(),
                start: idx,
                end: idx + trimmed.len(),
            });
            idx = end.max(idx + 1);
        } else {
            idx += 1;
        }
    }
    out
}

/// Whether a path run may begin at `idx`.
fn starts_here(reply: &str, idx: usize) -> bool {
    let rest = &reply[idx..];
    let opens_root = rest.starts_with('/') && !rest.starts_with("//");
    let opens_home = rest.starts_with("~/");
    if !opens_root && !opens_home {
        return false;
    }
    // Look at the character immediately before. A path in prose is preceded by
    // a space, a bracket or the start of the reply; a `/` inside a longer token
    // (`https://x`, `a/b/c`, `1/2`) is not a path start, and `:` is what makes
    // a URL scheme detectable without a scheme list.
    let before = reply[..idx].chars().next_back();
    match before {
        None => true,
        Some(ch) => {
            !(ch.is_alphanumeric() || matches!(ch, '/' | '~' | '.' | '-' | '_' | ':' | '%'))
        }
    }
}

/// Whether a trimmed run still names something below its root.
fn has_a_segment(candidate: &str) -> bool {
    let tail = candidate
        .strip_prefix("~/")
        .or_else(|| candidate.strip_prefix('/'));
    tail.is_some_and(|tail| !tail.is_empty())
}

/// A byte mask over the reply: `true` where the byte is inside code.
///
/// Fenced blocks first, then inline spans within the lines the fences left.
/// Both are the CommonMark rules reduced to what a scanner needs: a fence is a
/// run of three or more backticks or tildes at the start of a line, closed by a
/// run of the same character at least as long; an inline span is a backtick run
/// closed by an equal run on the same line. An unterminated fence masks to the
/// end of the reply, which is the right answer mid-stream — a reply still
/// arriving inside a code block is all code until it says otherwise.
fn code_mask(reply: &str) -> Vec<bool> {
    let mut mask = vec![false; reply.len()];
    let mut fence: Option<(char, usize)> = None;
    let mut offset = 0usize;
    for line in reply.split_inclusive('\n') {
        let line_len = line.len();
        let trimmed = line.trim_start();
        let indent = line_len - trimmed.len();
        let opener = fence_run(trimmed);
        match (&fence, opener) {
            (None, Some((ch, len))) => {
                fence = Some((ch, len));
                mask[offset..offset + line_len].fill(true);
            }
            (Some((open_ch, open_len)), Some((ch, len)))
                if ch == *open_ch && len >= *open_len && trimmed[len..].trim().is_empty() =>
            {
                fence = None;
                mask[offset..offset + line_len].fill(true);
            }
            (Some(_), _) => {
                mask[offset..offset + line_len].fill(true);
            }
            (None, None) => {
                mask_inline_code(trimmed, &mut mask[offset + indent..offset + line_len]);
            }
        }
        offset += line_len;
    }
    mask
}

/// The fence this line opens or closes, if it is a fence line at all.
fn fence_run(trimmed: &str) -> Option<(char, usize)> {
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let len = trimmed.chars().take_while(|ch| *ch == first).count();
    (len >= 3).then_some((first, len))
}

/// Mask the inline code spans of one line into `mask`, which covers exactly the
/// line's own bytes from its first non-space character.
fn mask_inline_code(line: &str, mask: &mut [bool]) {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let open = bytes[i..].iter().take_while(|b| **b == b'`').count();
        let mut j = i + open;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let close = bytes[j..].iter().take_while(|b| **b == b'`').count();
                if close == open {
                    mask[i..j + close].fill(true);
                    break;
                }
                j += close;
            } else {
                j += 1;
            }
        }
        if j >= bytes.len() {
            // An unterminated run is literal backticks, not a span.
            i += open;
        } else {
            i = j + open;
        }
    }
}

// ---------------------------------------------------------------------------
// Inbound: the grant check
// ---------------------------------------------------------------------------

/// One synced folder, as this module needs it: an id and where it is on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliverableRoot {
    /// The sync profile's id — the same id a [`Grant`]'s scope names.
    pub profile_id: String,
    /// Its `local_path`.
    pub local_path: PathBuf,
}

/// Whether keeper offers a control on a mentioned path, and if not, why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliverableControl {
    /// Reveal and open are offered, for this profile-relative location.
    Reveal {
        /// The profile the file is in.
        profile_id: String,
        /// Where in it, profile-relative.
        subpath: String,
    },
    /// No control, and the sentence that says why.
    None {
        /// One sentence, already worded for the surface.
        reason: String,
    },
}

/// One mentioned path and what keeper will do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deliverable {
    /// Where the path sits in the reply, and exactly what it said.
    pub mention: PathMention,
    /// The path with `~` expanded, for display. Not resolved, not
    /// canonicalized, and never joined to anything by this module.
    pub absolute: String,
    /// The verdict.
    pub control: DeliverableControl,
}

/// Resolve every path a reply named against the drive and the live grants
/// (FR-393, AD-160).
///
/// `exists` is injected rather than called directly so the decision is testable
/// without a filesystem — production passes `|path| path.exists()`. It is
/// consulted **only** for a path a grant already allowed: keeper does not stat
/// a location it has no permission to look at, because a probe that
/// distinguishes "not there" from "not allowed" is an oracle a reply author
/// should not have.
pub fn resolve_deliverables(
    reply: &str,
    home: Option<&Path>,
    roots: &[DeliverableRoot],
    grants: &[Grant],
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<Deliverable> {
    scan_paths(reply)
        .into_iter()
        .map(|mention| {
            let expanded = expand_home(&mention.raw, home);
            let control = classify(&expanded, roots, grants, exists);
            Deliverable {
                mention,
                absolute: expanded.to_string_lossy().into_owned(),
                control,
            }
        })
        .collect()
}

/// Expand a leading `~/`. Everything else is already absolute.
///
/// A `~` with no home to expand it against stays a `~` path, which then matches
/// no root and gets the outside-the-drive sentence — the honest outcome, and
/// better than silently treating it as relative to whatever the process cwd is.
fn expand_home(raw: &str, home: Option<&Path>) -> PathBuf {
    match (raw.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(raw),
    }
}

/// The verdict for one expanded path.
fn classify(
    path: &Path,
    roots: &[DeliverableRoot],
    grants: &[Grant],
    exists: &dyn Fn(&Path) -> bool,
) -> DeliverableControl {
    let Some((root, subpath)) = inside_a_root(path, roots) else {
        return DeliverableControl::None {
            reason: NO_CONTROL_OUTSIDE_DRIVE.to_owned(),
        };
    };
    let target = if subpath.is_empty() {
        ToolTarget::profile_root(&root.profile_id)
    } else {
        match ToolTarget::parse(&root.profile_id, &subpath) {
            Ok(target) => target,
            // A path that reached here cannot hold `..` — it came out of a root
            // strip — but the grammar is the door and a target that cannot pass
            // it is a target keeper will not reason about.
            Err(_) => {
                return DeliverableControl::None {
                    reason: NO_CONTROL_OUTSIDE_DRIVE.to_owned(),
                }
            }
        }
    };
    // The grant check. `Read`, because reveal and open are reads: keeper is
    // being asked to show a file, and a bot that may not read a folder may not
    // make keeper open it either.
    if !matches!(
        decide(grants, &target, Effect::Read),
        GrantVerdict::Allow { .. }
    ) {
        return DeliverableControl::None {
            reason: NO_CONTROL_NO_GRANT.to_owned(),
        };
    }
    if !exists(path) {
        return DeliverableControl::None {
            reason: NO_CONTROL_MISSING.to_owned(),
        };
    }
    DeliverableControl::Reveal {
        profile_id: root.profile_id.clone(),
        subpath: target.subpath,
    }
}

/// Which root contains `path`, and where inside it — **by component**.
///
/// The same rule [`crate::bots::grant`] applies to subtrees, for the same
/// reason: a `starts_with` over the joined string would put `/drive-old/x`
/// inside `/drive`. The deepest matching root wins, so a profile nested inside
/// another profile's folder resolves to the one that actually holds the file.
fn inside_a_root<'a>(
    path: &Path,
    roots: &'a [DeliverableRoot],
) -> Option<(&'a DeliverableRoot, String)> {
    let mut best: Option<(&DeliverableRoot, String, usize)> = None;
    for root in roots {
        let Ok(rest) = path.strip_prefix(&root.local_path) else {
            continue;
        };
        let depth = root.local_path.components().count();
        if best.as_ref().is_some_and(|(_, _, seen)| *seen >= depth) {
            continue;
        }
        let mut segments = Vec::new();
        for component in rest.components() {
            match component {
                Component::Normal(part) => segments.push(part.to_string_lossy().into_owned()),
                // `strip_prefix` cannot produce a root or a prefix here, and a
                // `.`/`..` would mean the reply named a path keeper will not
                // reason about — refused rather than normalized, which is
                // `parse_subpath`'s own position.
                _ => return None,
            }
        }
        best = Some((root, segments.join("/"), depth));
    }
    best.map(|(root, subpath, _)| (root, subpath))
}
