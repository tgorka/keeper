//! What a person sends into an agent's room besides words (story 101.2,
//! R417): a file or a photo, as an `m.file` or `m.image`, with a caption or
//! without one.
//!
//! The message is a turn whose text is its caption ([`caption_of`]). The
//! host downloads the file — at most [`MAX_BYTES`], counted as the body
//! arrives, within [`DOWNLOAD_WITHIN`], dropped when the turn stops — and
//! stores it, as it was sent, in the session's `attachments/` folder beside
//! the log, where the drive's own LFS rules route a large one when it
//! syncs; the turn's `user` line names it, so the model reads "Attached
//! files" and opens it with `drive_read`. A PDF's text is written beside it
//! as `<name>.txt`, read by a worker process with bounds ([`PdfReader`]); a
//! photo the model can see goes to it as an image part.
//! Whatever could not be done is a sentence ([`Intake::notes`]) the room
//! and the model both read, and a WARN line in the log (R381).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use keeper_core::bots::chat::{CancelSignal, ContentPart};
use keeper_core::bots::deliverable;
use keeper_core::bots::tools::FILE_CONTENT_IS_DATA;
use serde_json::Value;

/// The most bytes one message's file may have for an agent to get it: the
/// file is refused before it is downloaded when the message says it is
/// larger, and at the first byte past it as it arrives (R444/01).
pub const MAX_BYTES: u64 = 25 * 1024 * 1024;

/// The most bytes of a PDF's text written beside it: the rest is cut off,
/// and the file says where.
pub const MAX_PDF_TEXT_BYTES: usize = 4 * 1024 * 1024;

/// How long a file's whole download may take — [`MAX_BYTES`] at about
/// 90 kB/s — before the turn goes on without it (R444/02).
pub const DOWNLOAD_WITHIN: Duration = Duration::from_secs(300);

/// How long a PDF's reader may run, by the clock, before it is killed and
/// the turn goes on without the text (R444/03).
pub const PDF_READ: Duration = Duration::from_secs(60);

/// How much memory a PDF's reader may map beyond what it maps when it
/// starts — enforced on Linux only (R444/03).
pub const PDF_MEMORY: u64 = 1024 * 1024 * 1024;

/// How many seconds of CPU a PDF's reader may use (R444/03).
pub const PDF_CPU_SECONDS: u64 = 60;

/// The bounds a host takes a person's file in under (R444): how long its
/// download may take, and the PDF reader — `None` where this host has none,
/// and then a PDF's text is not read.
#[derive(Clone)]
pub struct Bounds {
    pub download_within: Duration,
    pub pdf: Option<Arc<PdfReader>>,
}

impl Bounds {
    /// This host's: [`DOWNLOAD_WITHIN`], and its own binary as the reader
    /// ([`PdfReader::this_binary`]).
    pub fn this_host() -> Bounds {
        Bounds {
            download_within: DOWNLOAD_WITHIN,
            pdf: PdfReader::this_binary().map(Arc::new),
        }
    }
}

/// The message types that carry a person's file.
const FILE_MSGTYPES: [&str; 2] = ["m.file", "m.image"];

/// Whether `content` is a message carrying a file: an `m.file` or an
/// `m.image`.
pub fn is_attachment(content: &Value) -> bool {
    content["msgtype"]
        .as_str()
        .is_some_and(|msgtype| FILE_MSGTYPES.contains(&msgtype))
}

/// The caption of a file message, as Matrix 1.10 puts it: `body` when the
/// message also names a `filename` that `body` is not; otherwise `body` is
/// only the file's name and there is no caption.
pub fn caption_of(content: &Value) -> String {
    let body = content["body"].as_str().unwrap_or_default();
    match content["filename"].as_str() {
        Some(filename) if filename != body => body.to_owned(),
        _ => String::new(),
    }
}

/// One file a message carries, as the message names it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sent {
    /// The plain name it is stored under ([`file_name`]).
    pub name: String,
    /// Its media type, lowercase: the message's, else (none, or only
    /// `application/octet-stream`) read off its name.
    pub mime: String,
    /// The size the message says it has.
    pub declared: Option<u64>,
    /// Where its bytes are: the encrypted file's object, or an `mxc://`
    /// URI string in a room without encryption; `None` when the message
    /// names neither — a file message no file can be fetched for, which is
    /// said, never taken for a message without one (R444/06).
    pub source: Option<Value>,
}

/// The file an `m.file` or `m.image` `content` carries, as it names it.
pub fn sent_of(content: &Value) -> Sent {
    let source = if content["file"].is_object() {
        Some(content["file"].clone())
    } else {
        content["url"]
            .as_str()
            .map(|url| Value::String(url.to_owned()))
    };
    let named = content["filename"]
        .as_str()
        .or(content["body"].as_str())
        .unwrap_or_default();
    let mime = content["info"]["mimetype"]
        .as_str()
        .map(str::to_ascii_lowercase)
        .filter(|mime| !mime.is_empty() && mime != "application/octet-stream")
        .unwrap_or_else(|| mime_of_name(named).to_owned());
    Sent {
        name: file_name(named, &mime),
        declared: content["info"]["size"].as_u64(),
        mime,
        source,
    }
}

/// How long a stored name may be, in bytes, its extension included.
const MAX_NAME_BYTES: usize = 120;

/// `named` — what a person's client called the file — as a plain file name
/// of the session: its last path segment, without control characters or
/// the characters a Mac or Windows volume refuses, without leading dots or
/// trailing dots and spaces, at most [`MAX_NAME_BYTES`] long with its
/// extension kept; `attachment.<ext>` when nothing is left.
pub fn file_name(named: &str, mime: &str) -> String {
    let last = named.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| {
            if matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned
        .trim()
        .trim_start_matches('.')
        .trim_end_matches(['.', ' '])
        .trim();
    if trimmed.is_empty() {
        return format!("attachment.{}", extension_of_mime(mime));
    }
    if trimmed.len() <= MAX_NAME_BYTES {
        return trimmed.to_owned();
    }
    let (stem, extension) = match trimmed.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && extension.len() <= 16 => {
            (stem, Some(extension))
        }
        _ => (trimmed, None),
    };
    let room = MAX_NAME_BYTES - extension.map_or(0, |extension| extension.len() + 1);
    let mut end = room.min(stem.len());
    while !stem.is_char_boundary(end) {
        end -= 1;
    }
    match extension {
        Some(extension) => format!("{}.{extension}", stem[..end].trim_end()),
        None => stem[..end].trim_end().to_owned(),
    }
}

/// What keeper does with a file of a kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A picture a model may be shown: PNG, JPEG, WEBP or GIF.
    Image,
    /// A photo in a format no model is shown — HEIC, HEIF, TIFF… — named
    /// by it (`HEIC`): stored, and said not to be shown (R444/05).
    Unshown(String),
    /// A PDF: its text is written beside it.
    Pdf,
    /// Text `drive_read` reads as it is: plain text, Markdown, CSV, JSON….
    Text,
    /// Anything else: stored, named, not read.
    Other,
}

/// The kind of a file of media type `mime` named `name`.
pub fn kind_of(mime: &str, name: &str) -> Kind {
    let mime = mime.split(';').next().unwrap_or_default().trim();
    let named = mime_of_name(name);
    if deliverable::IMAGE_MIMES.contains(&mime) {
        Kind::Image
    } else if let Some(format) = mime.strip_prefix("image/") {
        Kind::Unshown(format.trim_start_matches("x-").to_ascii_uppercase())
    } else if mime == "application/pdf" || named == "application/pdf" {
        Kind::Pdf
    } else if mime.starts_with("text/") || TEXT_MIMES.contains(&mime) || named.starts_with("text/")
    {
        Kind::Text
    } else {
        Kind::Other
    }
}

/// Media types other than `text/*` whose bytes are text.
const TEXT_MIMES: [&str; 5] = [
    "application/json",
    "application/csv",
    "application/x-yaml",
    "application/yaml",
    "application/xml",
];

/// The media type a file's name says, for a name the message gave no type
/// for: the kinds [`kind_of`] tells apart, else `application/octet-stream`.
fn mime_of_name(name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "tif" | "tiff" => "image/tiff",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "md" | "markdown" => "text/markdown",
        "txt" | "log" => "text/plain",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "xml" => "application/xml",
        _ => "application/octet-stream",
    }
}

/// The extension a file of `mime` with no usable name is stored with.
fn extension_of_mime(mime: &str) -> &'static str {
    match mime {
        "application/pdf" => "pdf",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/heic" => "heic",
        "image/heif" => "heif",
        "text/csv" => "csv",
        "text/markdown" => "md",
        "text/plain" => "txt",
        "application/json" => "json",
        _ => "bin",
    }
}

/// What reading a PDF's text found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfText {
    /// Its text, at most [`MAX_PDF_TEXT_BYTES`] of it.
    Text(String),
    /// It reads, and holds no text — a scan, most likely.
    NoText,
    /// It does not read as a PDF, or its reader stopped on it: why.
    Unreadable(String),
}

/// The text of the PDF `bytes`, by `pdf-extract`, in the process that
/// reads it — only ever [`PdfReader`]'s worker, under its bounds. A panic
/// in the parser is caught and is an unreadable PDF.
fn pdf_text(bytes: &[u8]) -> PdfText {
    let read = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes));
    match read {
        Ok(Ok(text)) if text.chars().any(char::is_alphanumeric) => {
            PdfText::Text(bounded(text.trim().to_owned()))
        }
        Ok(Ok(_)) => PdfText::NoText,
        Ok(Err(error)) => PdfText::Unreadable(error.to_string()),
        Err(_) => PdfText::Unreadable("the PDF reader stopped on it".to_owned()),
    }
}

/// The argument a host's own binary is started with to read one PDF's
/// text and exit: `<binary> __keeper-pdf-text <out> <memory> <cpu>`
/// ([`served_as_pdf_reader`]). agentd and the desktop app both answer it
/// before anything else runs.
pub const PDF_READER_ARG: &str = "__keeper-pdf-text";

/// The worker's exits: the text, written to `<out>`; no text; unreadable,
/// why written to `<out>`; refused — its bounds or its input failed. None
/// is 0: a binary that does not answer [`PDF_READER_ARG`] and exits 0 has
/// read nothing, and is an unreadable PDF, never an empty text.
const READ_TEXT: i32 = 40;
const READ_NO_TEXT: i32 = 41;
const READ_UNREADABLE: i32 = 42;
const READ_REFUSED: i32 = 43;

/// The most bytes the worker may write: the bounded text, its cut note,
/// and room to spare. `RLIMIT_FSIZE` holds it to this; the host reads no
/// more of it than this.
const PDF_OUT_MAX: u64 = MAX_PDF_TEXT_BYTES as u64 + 4096;

/// The PDF reader (R444/03): a child process — this host's own binary
/// started with [`PDF_READER_ARG`] — that gets the PDF on its stdin and
/// writes its text to a file of its own, under bounds it sets on itself
/// before it reads a byte: on Linux `RLIMIT_AS` (what it maps at start
/// plus `memory`) and `RLIMIT_CPU`; on a Mac `RLIMIT_CPU` only — macOS does
/// not enforce an address-space limit, so there the clock and the CPU bound
/// it; everywhere `RLIMIT_FSIZE` on its output and no core file. Past
/// `within` by the clock, or when the turn stops, it is killed and reaped.
/// A parser that runs away dies alone: the host only reads a file.
#[derive(Debug, Clone)]
pub struct PdfReader {
    /// The binary, and the arguments before [`PDF_READER_ARG`].
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// How long it may run by the clock.
    pub within: Duration,
    /// How much it may map beyond what it maps at start (Linux).
    pub memory: u64,
    /// How many CPU seconds it may use.
    pub cpu_seconds: u64,
}

impl PdfReader {
    /// This host's own binary, with [`PDF_READ`], [`PDF_MEMORY`] and
    /// [`PDF_CPU_SECONDS`]; `None` when the binary cannot be found.
    pub fn this_binary() -> Option<PdfReader> {
        let program = std::env::current_exe().ok()?;
        Some(PdfReader {
            program,
            args: Vec::new(),
            within: PDF_READ,
            memory: PDF_MEMORY,
            cpu_seconds: PDF_CPU_SECONDS,
        })
    }

    /// The text of the PDF `bytes`, read by a worker; `None` when `stop`
    /// fired first — the worker killed and reaped either way it did not
    /// end by itself.
    pub async fn read(&self, bytes: Vec<u8>, stop: &CancelSignal) -> Option<PdfText> {
        use tokio::io::AsyncWriteExt as _;
        let out = match scratch() {
            Ok(out) => out,
            Err(error) => {
                return Some(PdfText::Unreadable(format!(
                    "no file for the PDF reader's text: {error}"
                )))
            }
        };
        let spawned = tokio::process::Command::new(&self.program)
            .args(&self.args)
            .arg(PDF_READER_ARG)
            .arg(&out)
            .arg(self.memory.to_string())
            .arg(self.cpu_seconds.to_string())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => {
                let _ = std::fs::remove_file(&out);
                return Some(PdfText::Unreadable(format!(
                    "the PDF reader did not start: {error}"
                )));
            }
        };
        let stdin = child.stdin.take();
        let feed = async move {
            if let Some(mut stdin) = stdin {
                // A worker that died early closed its end: what it said
                // by its exit is the answer.
                let _ = stdin.write_all(&bytes).await;
            }
        };
        let mut stop = stop.clone();
        let ended = {
            let run = async { tokio::join!(feed, child.wait()).1 };
            tokio::select! {
                biased;
                () = stop.cancelled() => Ended::Stopped,
                ran = tokio::time::timeout(self.within, run) => match ran {
                    Ok(Ok(status)) => Ended::Exited(status),
                    Ok(Err(error)) => Ended::Lost(error.to_string()),
                    Err(_) => Ended::TimedOut,
                },
            }
        };
        if !matches!(ended, Ended::Exited(_)) {
            // Killed and waited for: no worker outlives its answer.
            let _ = child.kill().await;
        }
        let read = match ended {
            Ended::Stopped => None,
            Ended::TimedOut => Some(PdfText::Unreadable(format!(
                "its reader ran past {} s and was stopped",
                self.within.as_secs_f32()
            ))),
            Ended::Lost(why) => Some(PdfText::Unreadable(why)),
            Ended::Exited(status) => Some(match status.code() {
                Some(READ_TEXT) => match read_out(&out) {
                    Ok(text) => PdfText::Text(text),
                    Err(error) => PdfText::Unreadable(format!("its text did not read: {error}")),
                },
                Some(READ_NO_TEXT) => PdfText::NoText,
                Some(READ_UNREADABLE) => PdfText::Unreadable(
                    read_out(&out).unwrap_or_else(|_| "it did not read".to_owned()),
                ),
                _ => PdfText::Unreadable(format!("its reader stopped on it ({status})")),
            }),
        };
        let _ = std::fs::remove_file(&out);
        read
    }
}

/// How a worker's run ended.
enum Ended {
    Exited(std::process::ExitStatus),
    TimedOut,
    Stopped,
    Lost(String),
}

/// A new, empty file only this user may read, for one worker's text.
fn scratch() -> std::io::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "keeper-pdf-{}-{}.txt",
        std::process::id(),
        ulid::Ulid::new()
    ));
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
    open.open(&path)?;
    Ok(path)
}

/// At most [`PDF_OUT_MAX`] bytes of the worker's file, as text.
fn read_out(out: &Path) -> std::io::Result<String> {
    use std::io::Read as _;
    let mut text = String::new();
    std::fs::File::open(out)?
        .take(PDF_OUT_MAX)
        .read_to_string(&mut text)?;
    Ok(text)
}

/// Be the PDF reader when `args` — this process's arguments after its
/// program — start with [`PDF_READER_ARG`]: set the bounds, read the PDF
/// from stdin, write what it holds to `<out>`, and answer the exit code
/// the process ends with. `None` for every other run. A host calls this
/// first, before it starts anything.
pub fn served_as_pdf_reader(args: impl IntoIterator<Item = OsString>) -> Option<i32> {
    let mut args = args.into_iter();
    if args.next()? != PDF_READER_ARG {
        return None;
    }
    let out = PathBuf::from(args.next()?);
    let number = |arg: Option<OsString>| arg?.to_str()?.parse::<u64>().ok();
    let (Some(memory), Some(cpu)) = (number(args.next()), number(args.next())) else {
        return Some(READ_REFUSED);
    };
    Some(read_as_worker(&out, memory, cpu))
}

fn read_as_worker(out: &Path, memory: u64, cpu_seconds: u64) -> i32 {
    use std::io::Read as _;
    if bound_self(memory, cpu_seconds).is_err() {
        return READ_REFUSED;
    }
    let mut bytes = Vec::new();
    if std::io::stdin()
        .lock()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return READ_REFUSED;
    }
    let (code, said) = match pdf_text(&bytes) {
        PdfText::Text(text) => (READ_TEXT, text),
        PdfText::NoText => (READ_NO_TEXT, String::new()),
        PdfText::Unreadable(why) => (READ_UNREADABLE, why),
    };
    match std::fs::write(out, said) {
        Ok(()) => code,
        Err(_) => READ_REFUSED,
    }
}

/// The worker's bounds on itself, set before it reads its input.
fn bound_self(memory: u64, cpu_seconds: u64) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use rustix::process::{setrlimit, Resource, Rlimit};
        let cap = |value: u64| Rlimit {
            current: Some(value),
            maximum: Some(value),
        };
        setrlimit(Resource::Core, cap(0))?;
        setrlimit(Resource::Cpu, cap(cpu_seconds.max(1)))?;
        setrlimit(Resource::Fsize, cap(PDF_OUT_MAX))?;
        #[cfg(target_os = "linux")]
        setrlimit(Resource::As, cap(mapped_now()?.saturating_add(memory)))?;
    }
    #[cfg(not(target_os = "linux"))]
    let _ = memory;
    #[cfg(not(unix))]
    let _ = cpu_seconds;
    Ok(())
}

/// How many bytes this process maps now: `/proc/self/status`'s `VmSize`.
#[cfg(target_os = "linux")]
fn mapped_now() -> std::io::Result<u64> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmSize:"))
        .and_then(|size| {
            size.trim()
                .trim_end_matches("kB")
                .trim()
                .parse::<u64>()
                .ok()
        })
        .map(|kib| kib * 1024)
        .ok_or_else(|| std::io::Error::other("no VmSize in /proc/self/status"))
}

/// `text` cut to [`MAX_PDF_TEXT_BYTES`] at a character boundary, saying so
/// at its end when it was.
fn bounded(mut text: String) -> String {
    if text.len() <= MAX_PDF_TEXT_BYTES {
        return text;
    }
    let mut end = MAX_PDF_TEXT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str("\n\n[keeper cut the text here: the rest of the PDF is not in this file.]");
    text
}

/// The name a PDF's text is written under, beside it: `<name>.txt`.
pub fn text_name(stored: &str) -> String {
    format!("{stored}.txt")
}

/// A picture a person sent, as stored: its name, media type and bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// What became of a message's file before its turn: the files the `user`
/// line names, the pictures the model may be shown with it, and a sentence
/// for each thing that could not be done.
#[derive(Debug, Default)]
pub struct Intake {
    /// Drive-relative paths in the home drive, in the order written.
    pub stored: Vec<String>,
    /// Each picture to show the model, if it can see ([`shown`]).
    pub pictures: Vec<Picture>,
    /// What the person and the model are told.
    pub notes: Vec<String>,
}

/// What the turn's user message gains for a model that answers `vision`
/// (`None`: it did not say), and the notes of the whole intake: each
/// picture as an image part unless the model said it cannot see (R417) or
/// the picture is larger than a model is shown — then a note says so. The
/// image parts are file content, so they come after one text part of
/// their own saying so ([`FILE_CONTENT_IS_DATA`], the sentence every drive
/// read carries — AD-159, NFR-48), apart from the person's caption. When
/// there are notes, one text part holds them under [`NOTES_FOR_THE_MODEL`].
pub fn shown(
    pictures: Vec<Picture>,
    mut notes: Vec<String>,
    vision: Option<bool>,
    model: &str,
) -> (Vec<ContentPart>, Vec<String>) {
    let mut images = Vec::with_capacity(pictures.len());
    for picture in pictures {
        if let deliverable::ImageOffer::Refuse(_) = deliverable::image_offer(vision, model) {
            notes.push(cannot_see(model, &picture.name));
        } else if picture.bytes.len() > deliverable::MAX_IMAGE_BYTES {
            notes.push(too_large_to_show(&picture.name, picture.bytes.len() as u64));
        } else {
            images.push(deliverable::image_content_part(
                &picture.mime,
                &picture.bytes,
            ));
        }
    }
    let mut parts = Vec::with_capacity(images.len() + 2);
    if !images.is_empty() {
        parts.push(ContentPart::Text(FILE_CONTENT_IS_DATA.to_owned()));
        parts.extend(images);
    }
    if !notes.is_empty() {
        let listed: Vec<String> = notes.iter().map(|note| format!("- {note}")).collect();
        parts.push(ContentPart::Text(format!(
            "{NOTES_FOR_THE_MODEL}\n{}",
            listed.join("\n")
        )));
    }
    (parts, notes)
}

/// How a byte count reads in a sentence: whole kB below a megabyte, one
/// decimal of MB above — rounded up, in `u128`, so the largest size a
/// message may claim reads too (R444/08).
fn human(bytes: u64) -> String {
    const MB: u128 = 1024 * 1024;
    let bytes = u128::from(bytes);
    if bytes >= MB {
        let tenths = (bytes * 10).div_ceil(MB);
        format!("{}.{} MB", tenths / 10, tenths % 10)
    } else {
        format!("{} kB", bytes.div_ceil(1024))
    }
}

/// A file over [`MAX_BYTES`].
pub fn too_large(name: &str, bytes: u64) -> String {
    format!(
        "{name} is {} and an agent takes at most {} from one message, so it did not get the file.",
        human(bytes),
        human(MAX_BYTES)
    )
}

/// A host with no way to download from the room.
pub fn no_download(name: &str) -> String {
    format!("This agent's host cannot download files from the room, so it did not get {name}.")
}

/// A download that failed.
pub fn not_downloaded(name: &str) -> String {
    format!("keeper could not download {name} from the room, so the agent did not get it. Send it again.")
}

/// A download that did not end within the host's bound.
pub fn download_timed_out(name: &str, within: Duration) -> String {
    format!(
        "keeper stopped downloading {name} after {} s, so the agent did not get it. Send it again, or send a smaller file.",
        within.as_secs()
    )
}

/// A file message that names no file keeper can fetch (R444/06).
pub fn no_source(name: &str) -> String {
    format!("{name} arrived without a file keeper can fetch, so the agent did not get it. Send it again.")
}

/// A photo in a format no model is shown (R444/05).
pub fn format_not_shown(name: &str, format: &str) -> String {
    format!("{name} was saved for the agent, but this photo's format ({format}) can't be shown to the model.")
}

/// A host that cannot read a PDF's text.
pub fn no_pdf_reader(name: &str) -> String {
    format!("This agent's host cannot read the text of PDFs, so the agent knows only that {name} was sent.")
}

/// A file that could not be stored in the session.
pub fn not_stored(name: &str, why: &str) -> String {
    format!("keeper could not save {name} in this conversation's folder, so the agent did not get it: {why}")
}

/// A PDF with no text.
pub fn no_text(name: &str) -> String {
    format!("{name} has no text keeper can read — it may be a scan — so the agent knows only that it was sent.")
}

/// A PDF whose text could not be read.
pub fn pdf_unreadable(name: &str) -> String {
    format!("keeper could not read the text of {name}, so the agent knows only that it was sent.")
}

/// A PDF's text that could not be stored beside it.
pub fn text_not_stored(name: &str, why: &str) -> String {
    format!(
        "keeper could not save the text of {name} beside it, so the agent cannot read it: {why}"
    )
}

/// A photo for a model that said it cannot see.
pub fn cannot_see(model: &str, name: &str) -> String {
    format!("{model} cannot see images, so {name} was saved for the agent but not shown to it.")
}

/// A photo larger than a model is shown.
pub fn too_large_to_show(name: &str, bytes: u64) -> String {
    format!(
        "{name} is {}, more than the {} keeper shows a model, so it was saved for the agent but not shown to it.",
        human(bytes),
        human(deliverable::MAX_IMAGE_BYTES as u64)
    )
}

/// The heading the model reads the notes under, in the turn they belong to.
pub const NOTES_FOR_THE_MODEL: &str = "keeper, about the files in this message:";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Matrix 1.10's caption rule: `body` is a caption only beside a
    /// `filename` it differs from; a bare file, or one whose body is its
    /// name, has none.
    #[test]
    fn a_caption_is_the_body_beside_a_different_file_name() {
        let captioned =
            json!({"msgtype": "m.file", "body": "what is the total?", "filename": "q.pdf"});
        assert_eq!(caption_of(&captioned), "what is the total?");
        let named = json!({"msgtype": "m.file", "body": "q.pdf", "filename": "q.pdf"});
        assert_eq!(caption_of(&named), "");
        let bare = json!({"msgtype": "m.image", "body": "IMG_1.jpg"});
        assert_eq!(caption_of(&bare), "");
    }

    /// A message's file: its encrypted object or its plain URI, its name
    /// from `filename` before `body`, its type from `info` before its name.
    #[test]
    fn a_messages_file_is_read_from_file_or_url() {
        let encrypted = json!({
            "msgtype": "m.file", "body": "see this", "filename": "Q3 report.PDF",
            "file": {"url": "mxc://x/1", "v": "v2"}, "info": {"size": 42},
        });
        let sent = sent_of(&encrypted);
        assert_eq!(sent.name, "Q3 report.PDF");
        assert_eq!(sent.mime, "application/pdf");
        assert_eq!(sent.declared, Some(42));
        assert_eq!(sent.source.expect("a file")["url"], "mxc://x/1");

        let plain = json!({
            "msgtype": "m.image", "body": "cat.png", "url": "mxc://x/2",
            "info": {"mimetype": "image/PNG"},
        });
        let sent = sent_of(&plain);
        assert_eq!(sent.source, Some(json!("mxc://x/2")));
        assert_eq!(sent.mime, "image/png");
        // R444/06: a file message without its file is still one, named.
        let bare = sent_of(&json!({"msgtype": "m.file", "body": "x.pdf"}));
        assert_eq!((bare.name.as_str(), bare.source), ("x.pdf", None));
    }

    /// A name is a plain file name of the session whatever the client sent.
    #[test]
    fn a_sent_name_becomes_a_plain_file_name() {
        assert_eq!(file_name("../../etc/passwd", "text/plain"), "passwd");
        assert_eq!(file_name("C:\\Users\\a\\b.csv", "text/csv"), "b.csv");
        assert_eq!(file_name(".hidden.md", "text/markdown"), "hidden.md");
        assert_eq!(file_name("a:b?c\u{7}.txt", "text/plain"), "a-b-c.txt");
        assert_eq!(file_name("...", "image/jpeg"), "attachment.jpg");
        assert_eq!(file_name("", "application/pdf"), "attachment.pdf");
        let long = format!("{}.pdf", "é".repeat(100));
        let cut = file_name(&long, "application/pdf");
        assert!(cut.len() <= MAX_NAME_BYTES, "{}", cut.len());
        assert!(cut.ends_with(".pdf"), "{cut}");
    }

    /// The kinds, by type first and by name where the type says nothing;
    /// HEIC is a photo no model is shown, named by its format (R444/05).
    #[test]
    fn a_file_is_an_image_a_pdf_text_or_other() {
        assert_eq!(kind_of("image/jpeg", "p.jpg"), Kind::Image);
        assert_eq!(
            kind_of("image/heic", "p.heic"),
            Kind::Unshown("HEIC".to_owned())
        );
        assert_eq!(
            kind_of("image/x-tiff", "p.tif"),
            Kind::Unshown("TIFF".to_owned())
        );
        assert_eq!(kind_of("application/zip", "p.png"), Kind::Other);
        assert_eq!(kind_of("application/pdf", "x"), Kind::Pdf);
        assert_eq!(kind_of("application/octet-stream", "x.pdf"), Kind::Pdf);
        assert_eq!(kind_of("text/csv; charset=utf-8", "d.csv"), Kind::Text);
        assert_eq!(kind_of("application/octet-stream", "d.csv"), Kind::Text);
        assert_eq!(kind_of("application/json", "d"), Kind::Text);
        assert_eq!(kind_of("application/zip", "d.zip"), Kind::Other);
    }

    /// A PDF's text is read; a page that only draws holds none; bytes that
    /// are no PDF are unreadable, never a panic.
    #[test]
    fn a_pdfs_text_is_read_or_said_to_be_missing() {
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/attachments");
        let quarterly = std::fs::read(fixtures.join("quarterly.pdf")).expect("fixture");
        match pdf_text(&quarterly) {
            PdfText::Text(text) => assert!(text.contains("Quarterly total 4217"), "{text}"),
            other => panic!("no text: {other:?}"),
        }
        let scan = std::fs::read(fixtures.join("scan.pdf")).expect("fixture");
        assert_eq!(pdf_text(&scan), PdfText::NoText);
        assert!(matches!(pdf_text(b"not a pdf"), PdfText::Unreadable(_)));
    }

    /// Text past the cap is cut at a character and says it was.
    #[test]
    fn a_long_pdf_text_is_cut_and_says_so() {
        let long = "ż".repeat(MAX_PDF_TEXT_BYTES);
        let cut = bounded(long);
        assert!(cut.len() < MAX_PDF_TEXT_BYTES + 200);
        assert!(cut.ends_with("not in this file.]"));
        assert_eq!(bounded("short".to_owned()), "short");
    }

    /// R417: a picture goes to a model that can see or did not say; one
    /// that said it cannot gets a note instead, as does a picture over the
    /// size a model is shown; the notes ride the message as one text part.
    #[test]
    fn a_picture_is_shown_unless_the_model_said_it_cannot_see() {
        let photo = || Picture {
            name: "photo.jpg".to_owned(),
            mime: "image/jpeg".to_owned(),
            bytes: vec![0xff, 0xd8, 0xff],
        };
        let images = |parts: &[ContentPart]| {
            parts
                .iter()
                .filter(|part| matches!(part, ContentPart::Image { .. }))
                .count()
        };
        for vision in [Some(true), None] {
            let (parts, notes) = shown(vec![photo()], Vec::new(), vision, "llava");
            assert_eq!(images(&parts), 1, "{vision:?}");
            assert!(notes.is_empty(), "{vision:?}");
            // R444/07: the photo is file content, after the sentence that
            // says file content is data.
            assert!(
                matches!(&parts[..], [ContentPart::Text(said), ContentPart::Image { .. }] if said == FILE_CONTENT_IS_DATA),
                "{parts:?}"
            );
        }
        let (parts, notes) = shown(vec![photo()], Vec::new(), Some(false), "qwen3");
        assert_eq!(images(&parts), 0);
        assert_eq!(notes, [cannot_see("qwen3", "photo.jpg")]);
        assert!(matches!(&parts[..], [ContentPart::Text(text)] if text.contains(&notes[0])));

        let huge = Picture {
            bytes: vec![0; deliverable::MAX_IMAGE_BYTES + 1],
            ..photo()
        };
        let earlier = no_text("scan.pdf");
        let (parts, notes) = shown(vec![huge], vec![earlier.clone()], Some(true), "llava");
        assert_eq!(images(&parts), 0);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0], earlier);
    }

    /// R444/08: a size reads rounded up, never overflowing — the largest a
    /// message may claim too — and a size past the bound never reads as it.
    #[test]
    fn a_size_reads_whatever_a_message_claims() {
        assert!(too_large("x", u64::MAX).contains("17592186044416.0 MB"));
        assert!(too_large("x", MAX_BYTES).starts_with("x is 25.0 MB"));
        assert!(too_large("x", MAX_BYTES + 1).starts_with("x is 25.1 MB"));
        assert_eq!(human(1024 * 1024 - 1), "1024 kB");
    }

    /// This test binary as the PDF reader ([`pdf_reader_child`]), with the
    /// bounds a test can wait for.
    fn reader(within: Duration, memory: u64, stall: Option<&Path>) -> PdfReader {
        let mut args: Vec<OsString> = [
            "attachments::tests::pdf_reader_child",
            "--exact",
            "--nocapture",
        ]
        .map(OsString::from)
        .to_vec();
        // A filter no test matches: libtest still runs the one `--exact`
        // names, and the child reads it.
        args.extend(stall.map(|pid| OsString::from(format!("{STALL}{}", pid.display()))));
        PdfReader {
            program: std::env::current_exe().expect("this test binary"),
            args,
            within,
            memory,
            cpu_seconds: 20,
        }
    }

    /// The reader, when this binary was started as one; one that stalls,
    /// its pid written where its `stall:` argument says, when it has one.
    #[test]
    fn pdf_reader_child() {
        let args: Vec<OsString> = std::env::args_os().collect();
        let Some(at) = args.iter().position(|arg| arg == PDF_READER_ARG) else {
            return;
        };
        let stall = args
            .iter()
            .find_map(|arg| arg.to_str()?.strip_prefix(STALL).map(PathBuf::from));
        if let Some(pid) = stall {
            std::fs::write(pid, std::process::id().to_string()).expect("pid");
            loop {
                std::thread::sleep(Duration::from_secs(60));
            }
        }
        std::process::exit(served_as_pdf_reader(args[at..].iter().cloned()).expect("argv"));
    }

    /// The argument that makes a test reader stall, naming its pid's file.
    const STALL: &str = "stall:";

    /// A PDF whose one content stream inflates to `inflated` zero bytes
    /// from about 1/160 of that: a decompression bomb, made here, not
    /// committed. The zlib stream is written by hand — one fixed-Huffman
    /// block of a zero then copies of 258 bytes from one back — so making it
    /// costs nothing.
    fn bomb(inflated: usize) -> Vec<u8> {
        struct Bits {
            out: Vec<u8>,
            held: u64,
            count: u32,
        }
        impl Bits {
            fn put(&mut self, value: u64, count: u32) {
                self.held |= value << self.count;
                self.count += count;
                while self.count >= 8 {
                    self.out.push(self.held as u8);
                    self.held >>= 8;
                    self.count -= 8;
                }
            }
            /// A Huffman code, its first bit its highest.
            fn code(&mut self, code: u64, count: u32) {
                let reversed =
                    (0..count).fold(0, |r, i| r | (((code >> i) & 1) << (count - 1 - i)));
                self.put(reversed, count);
            }
        }
        let copies = (inflated - 1) / 258;
        let mut bits = Bits {
            out: vec![0x78, 0x01],
            held: 0,
            count: 0,
        };
        bits.put(1, 1); // the last block
        bits.put(1, 2); // fixed Huffman codes
        bits.code(0x30, 8); // the literal 0
        for _ in 0..copies {
            bits.code(0xc5, 8); // length 258
            bits.code(0, 5); // distance 1
        }
        bits.code(0, 7); // the block's end
        if bits.count > 0 {
            bits.out.push(bits.held as u8);
        }
        let total = (1 + copies * 258) as u64;
        let adler = (((total % 65521) as u32) << 16) | 1;
        bits.out.extend_from_slice(&adler.to_be_bytes());
        let stream = bits.out;
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        let mut object = |pdf: &mut Vec<u8>, body: &[u8]| {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
            pdf.extend_from_slice(body);
            pdf.extend_from_slice(b"\nendobj\n");
        };
        object(&mut pdf, b"<< /Type /Catalog /Pages 2 0 R >>");
        object(&mut pdf, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        object(
            &mut pdf,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>",
        );
        let mut contents = format!(
            "<< /Length {} /Filter /FlateDecode >>\nstream\n",
            stream.len()
        )
        .into_bytes();
        contents.extend_from_slice(&stream);
        contents.extend_from_slice(b"\nendstream");
        object(&mut pdf, &contents);
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
        );
        for offset in &offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                offsets.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    /// Whether the process `pid` is still there, a zombie too.
    fn alive(pid: &str) -> bool {
        std::process::Command::new("ps")
            .args(["-p", pid, "-o", "pid="])
            .output()
            .is_ok_and(|ps| !String::from_utf8_lossy(&ps.stdout).trim().is_empty())
    }

    /// R444/03: the reader reads a PDF's text in its own process, says when
    /// one has none, and calls bytes that are no PDF unreadable.
    #[tokio::test]
    async fn the_reader_reads_text_in_a_process_of_its_own() {
        let (_handle, stop) = keeper_core::bots::chat::cancellation();
        let reader = reader(Duration::from_secs(30), PDF_MEMORY, None);
        let fixtures =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/attachments");
        let quarterly = std::fs::read(fixtures.join("quarterly.pdf")).expect("fixture");
        match reader.read(quarterly, &stop).await {
            Some(PdfText::Text(text)) => assert!(text.contains("Quarterly total 4217"), "{text}"),
            other => panic!("no text: {other:?}"),
        }
        let scan = std::fs::read(fixtures.join("scan.pdf")).expect("fixture");
        assert_eq!(reader.read(scan, &stop).await, Some(PdfText::NoText));
        assert!(matches!(
            reader.read(b"not a pdf".to_vec(), &stop).await,
            Some(PdfText::Unreadable(_))
        ));
    }

    /// R444/03, R454: a decompression bomb — a small PDF inflating far past
    /// the reader's memory — ends its reader, not the host. Everywhere the
    /// clock and the CPU bound it, so the read is over in time; on Linux the
    /// address-space allowance is what ends it, so the PDF is unreadable. A
    /// Mac enforces no address-space limit, so there what it read is not
    /// asked.
    #[tokio::test]
    async fn a_decompression_bomb_ends_its_reader_not_the_host() {
        let (_handle, stop) = keeper_core::bots::chat::cancellation();
        let pdf = bomb(768 * 1024 * 1024);
        assert!(pdf.len() < 8 * 1024 * 1024, "{}", pdf.len());
        let reader = reader(Duration::from_secs(20), 256 * 1024 * 1024, None);
        let began = std::time::Instant::now();
        let read = reader.read(pdf, &stop).await;
        assert!(
            began.elapsed() < Duration::from_secs(25),
            "{read:?} after {:?}",
            began.elapsed()
        );
        #[cfg(target_os = "linux")]
        assert!(matches!(read, Some(PdfText::Unreadable(_))), "{read:?}");
    }

    /// R444/03: a reader that runs past its clock is killed and reaped —
    /// gone, not a zombie — and the PDF is unreadable; a turn that stops
    /// kills its reader the same way, and has no answer.
    #[tokio::test]
    async fn a_reader_past_its_clock_or_its_turn_is_killed_and_reaped() {
        let dir = tempfile::tempdir().expect("dir");
        for stopping in [false, true] {
            let pid_file = dir.path().join(format!("pid-{stopping}"));
            let (handle, stop) = keeper_core::bots::chat::cancellation();
            let within = Duration::from_millis(if stopping { 30_000 } else { 1_500 });
            let reader = reader(within, PDF_MEMORY, Some(&pid_file));
            let stopper = async {
                if stopping {
                    while !pid_file.exists() {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                    handle.cancel();
                }
            };
            let began = std::time::Instant::now();
            let (read, ()) = tokio::join!(reader.read(b"%PDF-1.4".to_vec(), &stop), stopper);
            assert!(
                began.elapsed() < Duration::from_secs(15),
                "{:?}",
                began.elapsed()
            );
            if stopping {
                assert_eq!(read, None);
            } else {
                assert!(matches!(read, Some(PdfText::Unreadable(_))), "{read:?}");
            }
            let pid = std::fs::read_to_string(&pid_file).expect("the reader ran");
            assert!(!alive(pid.trim()), "reader {pid} outlived its read");
        }
    }
}
